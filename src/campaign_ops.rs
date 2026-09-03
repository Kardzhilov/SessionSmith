//! Filesystem-safe operations for creating, forking, and renaming campaigns.
//!
//! These helpers have no TUI dependency so their collision and rollback behavior
//! can be tested against temporary directories.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut};
use walkdir::WalkDir;

use crate::config::{
    Campaign, CampaignConfig, OutputsConfig, Player, PromptOverrides, SystemRef,
    TranscriptionConfig,
};

const FORK_EXCLUDES: &[&str] = &["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"];
const RENAME_JOURNAL: &str = ".campaign-rename-journal.json";
const RENAME_JOURNAL_TMP: &str = ".campaign-rename-journal.tmp";
const SESSION_TRANSCRIPT_SUFFIXES: &[&str] = &[
    ".txt",
    ".srt",
    ".vtt",
    ".json",
    ".tsv",
    ".ssmeta.json",
    ".diarized.txt",
    ".diarized.srt",
    ".diarized.vtt",
];

/// Result of a successful campaign fork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkOutcome {
    pub config_path: PathBuf,
    pub files_copied: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRenameOutcome {
    pub old_stem: String,
    pub new_stem: String,
    pub moved_transcript_files: usize,
    pub moved_notes: bool,
    pub campaign_log_rebuild_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignRenameOutcome {
    pub old_campaign_id: String,
    pub new_campaign_id: String,
    pub old_name: String,
    pub new_name: String,
    pub old_config_path: PathBuf,
    pub new_config_path: PathBuf,
    pub old_output_root: PathBuf,
    pub new_output_root: PathBuf,
    pub index_migrated: bool,
    pub campaign_log_rebuild_required: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CampaignRenameJournal {
    version: u8,
    old_campaign_id: String,
    new_campaign_id: String,
    old_name: String,
    new_name: String,
    old_config_path: PathBuf,
    new_config_path: PathBuf,
    old_output_root: PathBuf,
    new_output_root: PathBuf,
    output_staging_root: PathBuf,
    old_index_path: PathBuf,
    new_index_path: PathBuf,
    had_output: bool,
}

#[derive(Debug)]
struct SessionRenamePlan {
    transcript_moves: Vec<(PathBuf, PathBuf)>,
    notes_move: Option<(PathBuf, PathBuf)>,
}

/// Build the standard initial campaign config used by both the CLI and TUI.
pub fn new_campaign_config(
    name: String,
    gm: String,
    setting: String,
    players: Vec<Player>,
    preset: String,
) -> CampaignConfig {
    let notes = fs::read_to_string("campaign.txt").unwrap_or_default();
    new_campaign_config_with_notes(name, gm, setting, players, preset, notes)
}

/// Build an initial campaign config using an explicit notes seed.
pub fn new_campaign_config_with_notes(
    name: String,
    gm: String,
    setting: String,
    players: Vec<Player>,
    preset: String,
    notes: String,
) -> CampaignConfig {
    CampaignConfig {
        source_path: None,
        campaign: Campaign {
            name,
            gm,
            setting,
            notes,
        },
        backend: Default::default(),
        asr: Default::default(),
        players,
        transcription: TranscriptionConfig::default(),
        system: SystemRef {
            preset,
            overrides: String::new(),
        },
        outputs: OutputsConfig::default(),
        prompts: PromptOverrides::default(),
    }
}

/// Return the canonical file location for a campaign name within `campaigns_dir`.
pub fn campaign_path(campaigns_dir: &Path, name: &str) -> PathBuf {
    campaigns_dir.join(format!("{}.toml", crate::util::slugify(name)))
}

/// Create a campaign config without overwriting a config or output history.
pub fn create_campaign(
    campaigns_dir: &Path,
    output_dir: &Path,
    config: &CampaignConfig,
) -> Result<PathBuf> {
    validate_campaign_name(&config.campaign.name)?;
    let config_path = campaign_path(campaigns_dir, &config.campaign.name);
    let output_root = output_dir.join(config.slug());
    if config_path.exists() {
        bail!("campaign already exists: {}", config_path.display());
    }
    if output_root.exists() {
        bail!(
            "campaign output already exists: {}; choose another name or move the existing output",
            output_root.display()
        );
    }
    fs::create_dir_all(campaigns_dir)
        .with_context(|| format!("creating campaign directory {}", campaigns_dir.display()))?;
    config
        .save(&config_path)
        .with_context(|| format!("saving campaign {}", config_path.display()))?;
    Ok(config_path)
}

/// Copy a campaign config and generated output into a separately named campaign.
pub fn fork_campaign(
    campaigns_dir: &Path,
    output_dir: &Path,
    source: &CampaignConfig,
    new_name: &str,
) -> Result<ForkOutcome> {
    validate_campaign_name(new_name)?;
    let mut copied = source.clone();
    copied.campaign.name = new_name.trim().to_string();
    copied.source_path = None;

    let config_path = campaign_path(campaigns_dir, &copied.campaign.name);
    let source_root = output_dir.join(source.slug());
    let destination_root = output_dir.join(copied.slug());
    if config_path.exists() {
        bail!("campaign already exists: {}", config_path.display());
    }
    if destination_root.exists() {
        bail!(
            "campaign output already exists: {}; choose another name or move the existing output",
            destination_root.display()
        );
    }

    let files_copied = match copy_dir_recursive(&source_root, &destination_root, FORK_EXCLUDES) {
        Ok(count) => count,
        Err(error) => {
            remove_dir_if_present(&destination_root);
            return Err(error);
        }
    };

    if let Err(error) = fs::create_dir_all(campaigns_dir)
        .with_context(|| format!("creating campaign directory {}", campaigns_dir.display()))
        .and_then(|()| {
            copied
                .save(&config_path)
                .with_context(|| format!("saving campaign {}", config_path.display()))
        })
    {
        remove_dir_if_present(&destination_root);
        return Err(error);
    }

    Ok(ForkOutcome {
        config_path,
        files_copied,
    })
}

/// Move a campaign output root without merging it into an existing history.
pub fn migrate_output_root(old_root: &Path, new_root: &Path) -> Result<()> {
    if old_root == new_root {
        return Ok(());
    }
    if new_root.exists() {
        bail!(
            "campaign output already exists: {}; refusing to merge histories",
            new_root.display()
        );
    }
    if !old_root.exists() {
        return Ok(());
    }
    if let Some(parent) = new_root.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    if fs::rename(old_root, new_root).is_ok() {
        return Ok(());
    }

    match copy_dir_recursive(old_root, new_root, &[]) {
        Ok(_) => {
            fs::remove_dir_all(old_root)
                .with_context(|| format!("removing moved output {}", old_root.display()))?;
            Ok(())
        }
        Err(error) => {
            remove_dir_if_present(new_root);
            Err(error)
        }
    }
}

fn migrate_output_root_staged(old_root: &Path, new_root: &Path, staging_root: &Path) -> Result<()> {
    if old_root == new_root || !old_root.exists() {
        return Ok(());
    }
    if new_root.exists() || staging_root.exists() {
        bail!(
            "campaign output destination or staging path already exists: {}; refusing to merge histories",
            new_root.display()
        );
    }
    if let Some(parent) = new_root.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    if fs::rename(old_root, new_root).is_ok() {
        sync_parent(new_root)?;
        return Ok(());
    }

    let copy_result = (|| -> Result<()> {
        copy_dir_recursive(old_root, staging_root, &[])?;
        sync_tree(staging_root)?;
        fs::rename(staging_root, new_root)
            .with_context(|| format!("installing staged campaign output {}", new_root.display()))?;
        sync_parent(new_root)?;
        fs::remove_dir_all(old_root)
            .with_context(|| format!("removing moved output {}", old_root.display()))?;
        sync_parent(old_root)?;
        Ok(())
    })();
    if copy_result.is_err() && old_root.exists() && !new_root.exists() {
        remove_dir_if_present(staging_root);
    }
    copy_result
}

/// Recover an interrupted campaign identity rename recorded under `campaigns_dir`.
///
/// An existing old config is authoritative and causes rollback. If only the
/// valid new config exists, cleanup is completed as a committed rename.
pub fn recover_campaign_renames(campaigns_dir: &Path, output_dir: &Path) -> Result<()> {
    let cache_root = dirs::cache_dir().unwrap_or_else(|| output_dir.to_path_buf());
    recover_campaign_renames_at(campaigns_dir, &cache_root)
}

pub fn recover_campaign_renames_at(campaigns_dir: &Path, cache_root: &Path) -> Result<()> {
    let journal_path = campaigns_dir.join(RENAME_JOURNAL);
    let temporary_path = campaigns_dir.join(RENAME_JOURNAL_TMP);
    if !journal_path.exists() {
        if temporary_path.exists() {
            fs::remove_file(&temporary_path).with_context(|| {
                format!(
                    "removing incomplete campaign rename journal {}",
                    temporary_path.display()
                )
            })?;
            sync_directory(campaigns_dir)?;
        }
        return Ok(());
    }

    let bytes = fs::read(&journal_path)
        .with_context(|| format!("reading campaign rename journal {}", journal_path.display()))?;
    let journal: CampaignRenameJournal = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing campaign rename journal {}", journal_path.display()))?;
    validate_rename_journal(&journal, campaigns_dir, cache_root)?;

    let same_config_path = journal.old_config_path == journal.new_config_path;
    let old_exists = journal.old_config_path.is_file();
    let new_exists = journal.new_config_path.is_file();
    let rollback = if same_config_path {
        let config = load_expected_rename_config(
            &journal.old_config_path,
            &[&journal.old_name, &journal.new_name],
        )?;
        config.campaign.name == journal.old_name
    } else if old_exists {
        load_expected_rename_config(&journal.old_config_path, &[&journal.old_name])?;
        true
    } else if new_exists {
        load_expected_rename_config(&journal.new_config_path, &[&journal.new_name])?;
        false
    } else {
        bail!(
            "campaign rename recovery found neither config; preserving output and indexes for manual recovery"
        );
    };

    if rollback {
        rollback_rename_journal(&journal)?;
    } else {
        commit_rename_journal(&journal)?;
    }
    remove_rename_journal(&journal_path)
}

fn load_expected_rename_config(path: &Path, expected_names: &[&String]) -> Result<CampaignConfig> {
    let config = CampaignConfig::load(path)
        .with_context(|| format!("validating recovery config {}", path.display()))?;
    if !expected_names
        .iter()
        .any(|expected| config.campaign.name == ***expected)
    {
        bail!(
            "campaign rename recovery found an unexpected identity in {}; preserving all copies",
            path.display()
        );
    }
    Ok(config)
}

fn rollback_rename_journal(journal: &CampaignRenameJournal) -> Result<()> {
    if journal.old_config_path != journal.new_config_path && journal.new_config_path.exists() {
        fs::remove_file(&journal.new_config_path).with_context(|| {
            format!(
                "removing staged campaign config {}",
                journal.new_config_path.display()
            )
        })?;
        sync_parent(&journal.new_config_path)?;
    }
    if journal.old_index_path != journal.new_index_path {
        remove_index_tree(&journal.new_index_path)?;
    }
    restore_old_output(journal)
}

fn restore_old_output(journal: &CampaignRenameJournal) -> Result<()> {
    if journal.old_output_root == journal.new_output_root {
        remove_dir_if_present(&journal.output_staging_root);
        return Ok(());
    }
    let old_exists = journal.old_output_root.exists();
    let new_exists = journal.new_output_root.exists();
    if !journal.had_output {
        if old_exists || new_exists || journal.output_staging_root.exists() {
            bail!(
                "campaign rename recovery found unexpected output copies; preserving them for manual recovery"
            );
        }
        return Ok(());
    }
    match (old_exists, new_exists) {
        (true, false) => {
            remove_dir_if_present(&journal.output_staging_root);
            Ok(())
        }
        (false, true) => {
            fs::rename(&journal.new_output_root, &journal.old_output_root).with_context(|| {
                format!(
                    "restoring campaign output {}",
                    journal.old_output_root.display()
                )
            })?;
            sync_parent(&journal.old_output_root)
        }
        (true, true) => {
            fs::remove_dir_all(&journal.old_output_root).with_context(|| {
                format!(
                    "removing incomplete old campaign output {}",
                    journal.old_output_root.display()
                )
            })?;
            fs::rename(&journal.new_output_root, &journal.old_output_root).with_context(|| {
                format!(
                    "restoring complete campaign output {}",
                    journal.old_output_root.display()
                )
            })?;
            remove_dir_if_present(&journal.output_staging_root);
            sync_parent(&journal.old_output_root)
        }
        (false, false) if journal.output_staging_root.exists() => bail!(
            "campaign rename recovery found only an incomplete staging output; preserving it for manual recovery"
        ),
        (false, false) => bail!(
            "campaign rename recovery could not find the campaign output; preserving remaining state"
        ),
    }
}

fn commit_rename_journal(journal: &CampaignRenameJournal) -> Result<()> {
    if journal.old_output_root != journal.new_output_root && journal.had_output {
        match (
            journal.old_output_root.exists(),
            journal.new_output_root.exists(),
        ) {
            (_, true) => {
                if journal.old_output_root.exists() {
                    fs::remove_dir_all(&journal.old_output_root).with_context(|| {
                        format!(
                            "removing old campaign output {}",
                            journal.old_output_root.display()
                        )
                    })?;
                }
            }
            (true, false) => {
                fs::rename(&journal.old_output_root, &journal.new_output_root).with_context(
                    || {
                        format!(
                            "finishing campaign output migration {}",
                            journal.new_output_root.display()
                        )
                    },
                )?;
            }
            (false, false) => bail!(
                "committed campaign rename has no complete output copy; preserving remaining state"
            ),
        }
        sync_parent(&journal.new_output_root)?;
    }
    if journal.output_staging_root.exists() {
        if !journal.new_output_root.exists() && journal.had_output {
            bail!(
                "committed campaign rename has only a staging output; preserving it for manual recovery"
            );
        }
        fs::remove_dir_all(&journal.output_staging_root).with_context(|| {
            format!(
                "removing campaign output staging {}",
                journal.output_staging_root.display()
            )
        })?;
    }
    if journal.old_index_path != journal.new_index_path {
        remove_index_tree(&journal.old_index_path)?;
    }
    Ok(())
}

fn remove_index_tree(index_path: &Path) -> Result<()> {
    let Some(parent) = index_path.parent() else {
        return Ok(());
    };
    if parent.exists() {
        fs::remove_dir_all(parent)
            .with_context(|| format!("removing campaign index {}", parent.display()))?;
        sync_parent(parent)?;
    }
    Ok(())
}

fn validate_rename_journal(
    journal: &CampaignRenameJournal,
    campaigns_dir: &Path,
    cache_root: &Path,
) -> Result<()> {
    if journal.version != 1 {
        bail!(
            "unsupported campaign rename journal version {}",
            journal.version
        );
    }
    let campaigns_dir = absolute_path(campaigns_dir)?;
    let cache_indexes = absolute_path(cache_root)?.join("sessionsmith/indexes");
    for path in [&journal.old_config_path, &journal.new_config_path] {
        if path.parent() != Some(campaigns_dir.as_path()) {
            bail!(
                "campaign rename journal config path is outside the campaigns directory: {}",
                path.display()
            );
        }
    }
    for path in [&journal.old_index_path, &journal.new_index_path] {
        if !path.starts_with(&cache_indexes)
            || path.file_name().and_then(|name| name.to_str()) != Some("index.sqlite")
        {
            bail!(
                "campaign rename journal index path is outside the cache root: {}",
                path.display()
            );
        }
    }
    if journal.output_staging_root != output_staging_path(&journal.new_output_root)? {
        bail!("campaign rename journal has an invalid output staging path");
    }
    Ok(())
}

fn write_rename_journal(campaigns_dir: &Path, journal: &CampaignRenameJournal) -> Result<PathBuf> {
    let journal_path = campaigns_dir.join(RENAME_JOURNAL);
    let temporary_path = campaigns_dir.join(RENAME_JOURNAL_TMP);
    let bytes = serde_json::to_vec(journal).context("serializing campaign rename journal")?;
    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .with_context(|| {
                format!(
                    "creating campaign rename journal {}",
                    temporary_path.display()
                )
            })?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary_path, &journal_path).with_context(|| {
            format!(
                "installing campaign rename journal {}",
                journal_path.display()
            )
        })?;
        sync_directory(campaigns_dir)
    })();
    if write_result.is_err() && temporary_path.exists() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result?;
    Ok(journal_path)
}

fn remove_rename_journal(journal_path: &Path) -> Result<()> {
    fs::remove_file(journal_path).with_context(|| {
        format!(
            "removing campaign rename journal {}",
            journal_path.display()
        )
    })?;
    sync_parent(journal_path)
}

fn output_staging_path(new_output_root: &Path) -> Result<PathBuf> {
    let name = new_output_root
        .file_name()
        .and_then(|name| name.to_str())
        .context("campaign output path must have a UTF-8 directory name")?;
    Ok(new_output_root.with_file_name(format!(".{name}.rename-staging")))
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn sync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    fs::File::open(path)
        .with_context(|| format!("opening directory for sync {}", path.display()))?
        .sync_all()
        .with_context(|| format!("syncing directory {}", path.display()))
}

#[cfg(not(unix))]
fn sync_directory(_: &Path) -> Result<()> {
    Ok(())
}

fn sync_tree(root: &Path) -> Result<()> {
    for entry in WalkDir::new(root).contents_first(true) {
        let entry = entry.with_context(|| format!("walking staged output {}", root.display()))?;
        fs::File::open(entry.path())
            .with_context(|| format!("opening staged output {}", entry.path().display()))?
            .sync_all()
            .with_context(|| format!("syncing staged output {}", entry.path().display()))?;
    }
    Ok(())
}

/// Rename a campaign and migrate every path coupled to its persisted identity.
pub fn rename_campaign(
    campaigns_dir: &Path,
    output_dir: &Path,
    old_config_path: &Path,
    new_name: &str,
    expected_revision: &str,
) -> Result<CampaignRenameOutcome> {
    let cache_root = dirs::cache_dir().unwrap_or_else(|| output_dir.to_path_buf());
    rename_campaign_at(
        campaigns_dir,
        output_dir,
        &cache_root,
        old_config_path,
        new_name,
        expected_revision,
    )
}

/// Rename a campaign using an explicit cache root for deterministic hosts and tests.
pub fn rename_campaign_at(
    campaigns_dir: &Path,
    output_dir: &Path,
    cache_root: &Path,
    old_config_path: &Path,
    new_name: &str,
    expected_revision: &str,
) -> Result<CampaignRenameOutcome> {
    recover_campaign_renames_at(campaigns_dir, cache_root)
        .context("recovering an interrupted campaign rename")?;
    validate_campaign_name(new_name)?;
    let new_name = new_name.trim();
    let original = fs::read(old_config_path)
        .with_context(|| format!("reading campaign config {}", old_config_path.display()))?;
    if crate::util::content_revision(&original) != expected_revision {
        bail!("campaign settings changed since they were read");
    }
    let original_text = std::str::from_utf8(&original).with_context(|| {
        format!(
            "campaign config is not UTF-8: {}",
            old_config_path.display()
        )
    })?;
    let mut old_campaign: CampaignConfig = toml::from_str(original_text)
        .with_context(|| format!("parsing {}", old_config_path.display()))?;
    old_campaign.source_path = fs::canonicalize(old_config_path)
        .ok()
        .or_else(|| Some(old_config_path.to_path_buf()));
    if old_campaign.campaign.name == new_name {
        bail!("new campaign name must differ from the current name");
    }

    let old_campaign_id = old_config_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .context("campaign config must have a UTF-8 filename")?
        .to_string();
    let new_campaign_id = crate::util::slugify(new_name);
    let new_config_path = campaigns_dir.join(format!("{new_campaign_id}.toml"));
    let config_path_changes = old_config_path != new_config_path;
    if config_path_changes && new_config_path.exists() {
        bail!("campaign already exists: {}", new_config_path.display());
    }

    let old_output_root = output_dir.join(old_campaign.slug());
    let new_output_root = output_dir.join(&new_campaign_id);
    if old_output_root != new_output_root && new_output_root.exists() {
        bail!(
            "campaign output already exists: {}; refusing to merge histories",
            new_output_root.display()
        );
    }

    let mut document = original_text
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", old_config_path.display()))?;
    let campaign = document
        .as_table_mut()
        .get_mut("campaign")
        .and_then(|item| item.as_table_mut())
        .context("[campaign] must be a TOML table to rename the campaign")?;
    campaign.insert("name", value(new_name));
    let updated = document.to_string();
    let mut new_campaign: CampaignConfig = toml::from_str(&updated)
        .with_context(|| format!("validating renamed campaign {}", new_config_path.display()))?;
    new_campaign.source_path = Some(future_source_path(&new_config_path));

    let moved_output = old_output_root != new_output_root && old_output_root.exists();
    let journal = CampaignRenameJournal {
        version: 1,
        old_campaign_id: old_campaign_id.clone(),
        new_campaign_id: new_campaign_id.clone(),
        old_name: old_campaign.campaign.name.clone(),
        new_name: new_name.to_string(),
        old_config_path: absolute_path(old_config_path)?,
        new_config_path: absolute_path(&new_config_path)?,
        old_output_root: absolute_path(&old_output_root)?,
        new_output_root: absolute_path(&new_output_root)?,
        output_staging_root: output_staging_path(&absolute_path(&new_output_root)?)?,
        old_index_path: absolute_path(&crate::index::campaign_index_path_at(
            cache_root,
            &old_campaign,
        ))?,
        new_index_path: absolute_path(&crate::index::campaign_index_path_at(
            cache_root,
            &new_campaign,
        ))?,
        had_output: old_output_root.exists(),
    };
    let journal_path = write_rename_journal(campaigns_dir, &journal)?;
    if moved_output {
        if let Err(error) = migrate_output_root_staged(
            &old_output_root,
            &new_output_root,
            &journal.output_staging_root,
        ) {
            let recovery = recover_campaign_renames_at(campaigns_dir, cache_root);
            return Err(with_rollback_error(
                error,
                recovery,
                "moving campaign output",
            ));
        }
    }

    let index_migrated = match crate::index::migrate_campaign_index_at(
        cache_root,
        &old_campaign,
        &new_campaign,
        &old_output_root,
        &new_output_root,
    ) {
        Ok(migrated) => migrated,
        Err(error) => {
            let rollback = recover_campaign_renames_at(campaigns_dir, cache_root);
            return Err(with_rollback_error(
                error,
                rollback,
                "migrating campaign search index",
            ));
        }
    };

    let install_result = if config_path_changes {
        install_renamed_config(old_config_path, &new_config_path, updated.as_bytes())
    } else {
        crate::util::atomic_replace_if_revision(
            old_config_path,
            expected_revision,
            updated.as_bytes(),
        )
        .with_context(|| format!("writing renamed campaign {}", old_config_path.display()))
    };
    if let Err(error) = install_result {
        let rollback = recover_campaign_renames_at(campaigns_dir, cache_root);
        if rollback.is_ok() {
            return Err(error).context("installing renamed campaign; migration was rolled back");
        }
        bail!(
            "installing renamed campaign failed: {error:#}; rollback also failed: {:#}",
            rollback.unwrap_err()
        );
    }

    commit_rename_journal(&journal).context("finishing campaign rename cleanup")?;
    remove_rename_journal(&journal_path)?;

    Ok(CampaignRenameOutcome {
        old_campaign_id,
        new_campaign_id,
        old_name: old_campaign.campaign.name,
        new_name: new_name.to_string(),
        old_config_path: old_config_path.to_path_buf(),
        new_config_path,
        old_output_root,
        new_output_root,
        index_migrated,
        campaign_log_rebuild_required: false,
    })
}

fn future_source_path(path: &Path) -> PathBuf {
    let Some(parent) = path.parent() else {
        return path.to_path_buf();
    };
    parent
        .canonicalize()
        .map(|parent| parent.join(path.file_name().unwrap_or_default()))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn install_renamed_config(old_path: &Path, new_path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = new_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating campaign directory {}", parent.display()))?;
    }
    let permissions = fs::metadata(old_path)?.permissions();
    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(new_path)
            .with_context(|| format!("creating renamed campaign {}", new_path.display()))?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::set_permissions(new_path, permissions)?;
        fs::remove_file(old_path)
            .with_context(|| format!("removing old campaign config {}", old_path.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(new_path);
    }
    write_result
}

fn with_rollback_error(error: anyhow::Error, rollback: Result<()>, action: &str) -> anyhow::Error {
    match rollback {
        Ok(()) => error.context(format!("{action}; campaign output was restored")),
        Err(rollback) => anyhow::anyhow!(
            "{action} failed: {error:#}; restoring campaign output also failed: {rollback:#}"
        ),
    }
}

/// Rebuild search records from copied or moved session note directories.
///
/// Index errors are returned to the caller so the UI can report them without
/// discarding an otherwise successful create, fork, or rename.
pub fn reindex_campaign(campaign: &CampaignConfig) -> Result<usize> {
    let notes_dir = campaign.notes_dir();
    let entries = match fs::read_dir(&notes_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading notes directory {}", notes_dir.display()))
        }
    };

    let mut indexed = 0;
    for entry in entries {
        let entry = entry.with_context(|| "reading campaign notes entry")?;
        let path = entry.path();
        if !path.is_dir()
            || entry
                .file_name()
                .to_str()
                .map(|name| name.starts_with('_'))
                .unwrap_or(true)
        {
            continue;
        }
        let stem = entry.file_name().to_string_lossy().to_string();
        crate::index::record_session(campaign, &stem, &path)
            .with_context(|| format!("indexing campaign session {stem}"))?;
        indexed += 1;
    }
    Ok(indexed)
}

pub fn rename_session(
    campaign: &CampaignConfig,
    old_stem: &str,
    new_stem: &str,
) -> Result<SessionRenameOutcome> {
    rename_session_at(
        campaign,
        &campaign.transcripts_dir(),
        &campaign.notes_dir(),
        old_stem,
        new_stem,
    )
}

pub fn check_session_rename_at(
    transcripts_dir: &Path,
    notes_dir: &Path,
    old_stem: &str,
    new_stem: &str,
) -> Result<()> {
    plan_session_rename(transcripts_dir, notes_dir, old_stem, new_stem).map(|_| ())
}

pub fn rename_session_at(
    campaign: &CampaignConfig,
    transcripts_dir: &Path,
    notes_dir: &Path,
    old_stem: &str,
    new_stem: &str,
) -> Result<SessionRenameOutcome> {
    let plan = plan_session_rename(transcripts_dir, notes_dir, old_stem, new_stem)?;
    apply_session_rename(&plan)?;

    let new_notes_dir = notes_dir.join(new_stem);
    let new_transcript = transcripts_dir.join(format!("{new_stem}.txt"));
    let index_result = crate::index::delete_session(campaign, old_stem).and_then(|()| {
        crate::index::record_session_at(campaign, new_stem, &new_notes_dir, &new_transcript)
    });
    if let Err(index_error) = index_result {
        let mut rollback_errors = Vec::new();
        if let Err(error) = crate::index::delete_session(campaign, new_stem) {
            rollback_errors.push(format!("removing new search records: {error:#}"));
        }
        if let Err(error) = rollback_session_rename(&plan) {
            rollback_errors.push(format!("restoring renamed files: {error:#}"));
        }
        let old_notes_dir = notes_dir.join(old_stem);
        let old_transcript = transcripts_dir.join(format!("{old_stem}.txt"));
        if let Err(error) =
            crate::index::record_session_at(campaign, old_stem, &old_notes_dir, &old_transcript)
        {
            rollback_errors.push(format!("restoring old search records: {error:#}"));
        }
        if rollback_errors.is_empty() {
            return Err(index_error)
                .context("updating search index; session rename was rolled back");
        }
        bail!(
            "updating search index failed: {index_error:#}; rollback also failed: {}",
            rollback_errors.join("; ")
        );
    }

    Ok(SessionRenameOutcome {
        old_stem: old_stem.to_string(),
        new_stem: new_stem.to_string(),
        moved_transcript_files: plan.transcript_moves.len(),
        moved_notes: plan.notes_move.is_some(),
        campaign_log_rebuild_required: true,
    })
}

pub fn validate_session_stem(stem: &str) -> Result<()> {
    if stem.is_empty() {
        bail!("session name cannot be empty");
    }
    if stem.starts_with('.') {
        bail!("session name cannot start with a dot");
    }
    if stem.len() > 100 {
        bail!("session name cannot exceed 100 bytes");
    }
    if stem.chars().any(char::is_control) {
        bail!("session name cannot contain control characters");
    }
    if stem.contains(['/', '\\']) {
        bail!("session name cannot contain path separators");
    }
    if Path::new(stem).file_name().and_then(|name| name.to_str()) != Some(stem) {
        bail!("session name must be a single path component");
    }
    if stem.ends_with(".diarized") {
        bail!("session name cannot end with .diarized");
    }
    Ok(())
}

fn plan_session_rename(
    transcripts_dir: &Path,
    notes_dir: &Path,
    old_stem: &str,
    new_stem: &str,
) -> Result<SessionRenamePlan> {
    validate_session_stem(old_stem).context("invalid current session name")?;
    validate_session_stem(new_stem).context("invalid new session name")?;
    if old_stem == new_stem {
        bail!("new session name must differ from the current name");
    }

    let mut transcript_moves = Vec::new();
    for suffix in SESSION_TRANSCRIPT_SUFFIXES {
        let source = transcripts_dir.join(format!("{old_stem}{suffix}"));
        let destination = transcripts_dir.join(format!("{new_stem}{suffix}"));
        if destination.exists() {
            bail!("session artifact already exists: {}", destination.display());
        }
        if source.is_file() {
            transcript_moves.push((source, destination));
        }
    }

    let old_notes = notes_dir.join(old_stem);
    let new_notes = notes_dir.join(new_stem);
    if new_notes.exists() {
        bail!("session notes already exist: {}", new_notes.display());
    }
    let notes_move = old_notes.is_dir().then_some((old_notes, new_notes));
    if transcript_moves.is_empty() && notes_move.is_none() {
        bail!("session does not exist: {old_stem}");
    }
    Ok(SessionRenamePlan {
        transcript_moves,
        notes_move,
    })
}

fn apply_session_rename(plan: &SessionRenamePlan) -> Result<()> {
    for (moved_files, (source, destination)) in plan.transcript_moves.iter().enumerate() {
        if let Err(error) = fs::rename(source, destination)
            .with_context(|| format!("renaming session artifact {}", source.display()))
        {
            let partial = SessionRenamePlan {
                transcript_moves: plan.transcript_moves[..moved_files].to_vec(),
                notes_move: None,
            };
            let _ = rollback_session_rename(&partial);
            return Err(error);
        }
    }
    if let Some((source, destination)) = &plan.notes_move {
        if let Err(error) = fs::rename(source, destination)
            .with_context(|| format!("renaming session notes {}", source.display()))
        {
            let partial = SessionRenamePlan {
                transcript_moves: plan.transcript_moves.clone(),
                notes_move: None,
            };
            let _ = rollback_session_rename(&partial);
            return Err(error);
        }
    }
    Ok(())
}

fn rollback_session_rename(plan: &SessionRenamePlan) -> Result<()> {
    if let Some((source, destination)) = &plan.notes_move {
        if destination.exists() {
            fs::rename(destination, source)
                .with_context(|| format!("restoring session notes {}", source.display()))?;
        }
    }
    for (source, destination) in plan.transcript_moves.iter().rev() {
        if destination.exists() {
            fs::rename(destination, source)
                .with_context(|| format!("restoring session artifact {}", source.display()))?;
        }
    }
    Ok(())
}

fn validate_campaign_name(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("campaign name cannot be empty");
    }
    if name.len() > 100 {
        bail!("campaign name cannot exceed 100 bytes");
    }
    if name.chars().any(char::is_control) {
        bail!("campaign name cannot contain control characters");
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path, excludes: &[&str]) -> Result<usize> {
    if !source.exists() {
        return Ok(0);
    }
    let mut copied = 0;
    for entry in WalkDir::new(source) {
        let entry =
            entry.with_context(|| format!("walking campaign output {}", source.display()))?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .with_context(|| format!("resolving copied path {}", entry.path().display()))?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("creating copied directory {}", target.display()))?;
            continue;
        }
        if entry.file_type().is_symlink() {
            bail!(
                "refusing to copy symlink in campaign output: {}",
                entry.path().display()
            );
        }
        if excludes.iter().any(|name| entry.file_name() == *name) {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating copied directory {}", parent.display()))?;
        }
        fs::copy(entry.path(), &target).with_context(|| {
            format!(
                "copying campaign output {} to {}",
                entry.path().display(),
                target.display()
            )
        })?;
        copied += 1;
    }
    Ok(copied)
}

fn remove_dir_if_present(path: &Path) {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn config(name: &str) -> CampaignConfig {
        new_campaign_config_with_notes(
            name.into(),
            "GM".into(),
            "Setting".into(),
            vec![Player {
                player: "Player".into(),
                character: "Character".into(),
                ancestry: String::new(),
                class: String::new(),
            }],
            "generic".into(),
            String::new(),
        )
    }

    fn recovery_fixture(
        root: &Path,
    ) -> (
        PathBuf,
        PathBuf,
        PathBuf,
        CampaignConfig,
        CampaignConfig,
        CampaignRenameJournal,
    ) {
        let campaigns = root.join("campaigns");
        let output = root.join("output");
        let cache = root.join("cache");
        fs::create_dir_all(&campaigns).unwrap();
        let old_config_path = campaigns.join("old.toml");
        let new_config_path = campaigns.join("new.toml");
        config("Old").save(&old_config_path).unwrap();
        fs::create_dir_all(output.join("old")).unwrap();
        fs::write(output.join("old/history.txt"), "complete history").unwrap();
        let mut old_campaign = CampaignConfig::load(&old_config_path).unwrap();
        old_campaign.source_path = Some(future_source_path(&old_config_path));
        let mut new_campaign = old_campaign.clone();
        new_campaign.campaign.name = "New".into();
        new_campaign.source_path = Some(future_source_path(&new_config_path));
        let old_output_root = output.join("old");
        let new_output_root = output.join("new");
        let journal = CampaignRenameJournal {
            version: 1,
            old_campaign_id: "old".into(),
            new_campaign_id: "new".into(),
            old_name: "Old".into(),
            new_name: "New".into(),
            old_config_path: old_config_path.clone(),
            new_config_path: new_config_path.clone(),
            old_output_root: old_output_root.clone(),
            new_output_root: new_output_root.clone(),
            output_staging_root: output_staging_path(&new_output_root).unwrap(),
            old_index_path: crate::index::campaign_index_path_at(&cache, &old_campaign),
            new_index_path: crate::index::campaign_index_path_at(&cache, &new_campaign),
            had_output: true,
        };
        write_rename_journal(&campaigns, &journal).unwrap();
        (
            campaigns,
            output,
            cache,
            old_campaign,
            new_campaign,
            journal,
        )
    }

    fn create_test_index(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE artifacts (
                    id INTEGER PRIMARY KEY,
                    session TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    path TEXT NOT NULL,
                    content TEXT NOT NULL,
                    updated INTEGER NOT NULL,
                    UNIQUE(session, kind)
                 );
                 CREATE VIRTUAL TABLE artifacts_fts USING fts5(
                    session, kind, path UNINDEXED, content, tokenize = 'porter unicode61'
                 );
                 PRAGMA user_version = 2;",
            )
            .unwrap();
    }

    #[test]
    fn create_writes_config_and_rejects_slug_collisions() {
        let temp = tempdir().unwrap();
        let campaigns = temp.path().join("campaigns");
        let output = temp.path().join("output");
        let first = config("My Game");
        let path = create_campaign(&campaigns, &output, &first).unwrap();
        assert_eq!(
            CampaignConfig::load(&path).unwrap().campaign.name,
            "My Game"
        );

        let error = create_campaign(&campaigns, &output, &config("my game")).unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert_eq!(
            CampaignConfig::load(&path).unwrap().campaign.name,
            "My Game"
        );
    }

    #[test]
    fn fork_copies_output_but_excludes_legacy_index() {
        let temp = tempdir().unwrap();
        let campaigns = temp.path().join("campaigns");
        let output = temp.path().join("output");
        let source = config("Original");
        let source_root = output.join(source.slug());
        fs::create_dir_all(source_root.join("notes/session-one")).unwrap();
        fs::create_dir_all(source_root.join("transcripts")).unwrap();
        fs::write(source_root.join("notes/session-one/summary.md"), "summary").unwrap();
        fs::write(
            source_root.join("transcripts/session-one.txt"),
            "transcript",
        )
        .unwrap();
        fs::write(source_root.join("index.sqlite"), "database").unwrap();

        let outcome = fork_campaign(&campaigns, &output, &source, "Forked").unwrap();
        let copied = CampaignConfig::load(&outcome.config_path).unwrap();
        let destination = output.join(copied.slug());
        assert_eq!(copied.campaign.name, "Forked");
        assert_eq!(outcome.files_copied, 2);
        assert_eq!(
            fs::read_to_string(destination.join("notes/session-one/summary.md")).unwrap(),
            "summary"
        );
        assert_eq!(
            fs::read_to_string(destination.join("transcripts/session-one.txt")).unwrap(),
            "transcript"
        );
        assert!(!destination.join("index.sqlite").exists());
        assert_eq!(
            fs::read_to_string(source_root.join("index.sqlite")).unwrap(),
            "database"
        );
    }

    #[test]
    fn fork_of_campaign_without_output_succeeds() {
        let temp = tempdir().unwrap();
        let outcome = fork_campaign(
            &temp.path().join("campaigns"),
            &temp.path().join("output"),
            &config("Original"),
            "Forked",
        )
        .unwrap();
        assert!(outcome.config_path.exists());
        assert_eq!(outcome.files_copied, 0);
    }

    #[test]
    fn migrate_moves_output_and_refuses_a_merge() {
        let temp = tempdir().unwrap();
        let old = temp.path().join("old");
        let new = temp.path().join("new");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("entry.txt"), "entry").unwrap();
        migrate_output_root(&old, &new).unwrap();
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(new.join("entry.txt")).unwrap(), "entry");

        fs::create_dir_all(&old).unwrap();
        assert!(migrate_output_root(&old, &new).is_err());
        assert!(old.exists());
    }

    #[test]
    fn migrate_rejects_an_existing_destination_without_source_output() {
        let temp = tempdir().unwrap();
        let old = temp.path().join("missing");
        let new = temp.path().join("new");
        fs::create_dir_all(&new).unwrap();
        assert!(migrate_output_root(&old, &new).is_err());
    }

    #[test]
    fn campaign_rename_moves_config_output_and_preserves_toml_comments() {
        let temp = tempdir().unwrap();
        let campaigns = temp.path().join("campaigns");
        let output = temp.path().join("output");
        let cache = temp.path().join("cache");
        fs::create_dir_all(&campaigns).unwrap();
        let old_path = campaigns.join("old-name.toml");
        config("Old Name").save(&old_path).unwrap();
        let original = format!(
            "# keep this comment\n{}",
            fs::read_to_string(&old_path).unwrap()
        );
        fs::write(&old_path, &original).unwrap();
        let revision = crate::util::content_revision(original.as_bytes());
        fs::create_dir_all(output.join("old-name/notes/session1")).unwrap();
        fs::write(
            output.join("old-name/notes/session1/summary.md"),
            "preserved",
        )
        .unwrap();

        let outcome = rename_campaign_at(
            &campaigns, &output, &cache, &old_path, "New Name", &revision,
        )
        .unwrap();

        assert_eq!(outcome.old_campaign_id, "old-name");
        assert_eq!(outcome.new_campaign_id, "new-name");
        assert!(!old_path.exists());
        assert_eq!(
            CampaignConfig::load(&outcome.new_config_path)
                .unwrap()
                .campaign
                .name,
            "New Name"
        );
        assert!(fs::read_to_string(&outcome.new_config_path)
            .unwrap()
            .starts_with("# keep this comment"));
        assert_eq!(
            fs::read_to_string(output.join("new-name/notes/session1/summary.md")).unwrap(),
            "preserved"
        );
        assert!(!output.join("old-name").exists());
    }

    #[test]
    fn campaign_rename_supports_same_slug_display_name_change() {
        let temp = tempdir().unwrap();
        let campaigns = temp.path().join("campaigns");
        let output = temp.path().join("output");
        fs::create_dir_all(&campaigns).unwrap();
        let path = campaigns.join("my-game.toml");
        config("My Game").save(&path).unwrap();
        let original = fs::read(&path).unwrap();

        let outcome = rename_campaign_at(
            &campaigns,
            &output,
            &temp.path().join("cache"),
            &path,
            "my game",
            &crate::util::content_revision(&original),
        )
        .unwrap();

        assert_eq!(outcome.old_config_path, outcome.new_config_path);
        assert_eq!(
            CampaignConfig::load(&path).unwrap().campaign.name,
            "my game"
        );
    }

    #[test]
    fn campaign_rename_preflights_collisions_without_mutation() {
        let temp = tempdir().unwrap();
        let campaigns = temp.path().join("campaigns");
        let output = temp.path().join("output");
        fs::create_dir_all(&campaigns).unwrap();
        let old_path = campaigns.join("old.toml");
        config("Old").save(&old_path).unwrap();
        config("New").save(&campaigns.join("new.toml")).unwrap();
        fs::create_dir_all(output.join("old")).unwrap();
        let original = fs::read(&old_path).unwrap();

        let error = rename_campaign_at(
            &campaigns,
            &output,
            &temp.path().join("cache"),
            &old_path,
            "New",
            &crate::util::content_revision(&original),
        )
        .unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert!(old_path.exists());
        assert!(output.join("old").exists());
    }

    #[test]
    fn campaign_rename_rolls_back_output_when_index_migration_fails() {
        let temp = tempdir().unwrap();
        let campaigns = temp.path().join("campaigns");
        let output = temp.path().join("output");
        let cache = temp.path().join("cache");
        fs::create_dir_all(&campaigns).unwrap();
        let old_path = campaigns.join("old.toml");
        config("Old").save(&old_path).unwrap();
        let (mut old_campaign, revision) = CampaignConfig::load_with_revision(&old_path).unwrap();
        fs::create_dir_all(output.join("old")).unwrap();
        fs::write(output.join("old/artifact.txt"), "preserved").unwrap();
        let mut new_campaign = old_campaign.clone();
        new_campaign.campaign.name = "New".into();
        new_campaign.source_path = Some(future_source_path(&campaigns.join("new.toml")));
        old_campaign.source_path = Some(future_source_path(&old_path));
        for campaign in [&old_campaign, &new_campaign] {
            let index = crate::index::campaign_index_path_at(&cache, campaign);
            fs::create_dir_all(index.parent().unwrap()).unwrap();
            fs::write(index, []).unwrap();
        }

        let error = rename_campaign_at(&campaigns, &output, &cache, &old_path, "New", &revision)
            .unwrap_err();
        assert!(error.to_string().contains("search index"));
        assert!(old_path.exists());
        assert!(!campaigns.join("new.toml").exists());
        assert_eq!(
            fs::read_to_string(output.join("old/artifact.txt")).unwrap(),
            "preserved"
        );
        assert!(!output.join("new").exists());
    }

    #[test]
    fn campaign_rename_recovery_rolls_back_after_output_move() {
        let temp = tempdir().unwrap();
        let (campaigns, output, cache, _, _, journal) = recovery_fixture(temp.path());
        migrate_output_root_staged(
            &journal.old_output_root,
            &journal.new_output_root,
            &journal.output_staging_root,
        )
        .unwrap();

        recover_campaign_renames_at(&campaigns, &cache).unwrap();

        assert_eq!(
            fs::read_to_string(output.join("old/history.txt")).unwrap(),
            "complete history"
        );
        assert!(!output.join("new").exists());
        assert!(!campaigns.join(RENAME_JOURNAL).exists());
    }

    #[test]
    fn campaign_rename_recovery_rolls_back_after_index_copy() {
        let temp = tempdir().unwrap();
        let (campaigns, output, cache, old_campaign, new_campaign, journal) =
            recovery_fixture(temp.path());
        create_test_index(&journal.old_index_path);
        migrate_output_root_staged(
            &journal.old_output_root,
            &journal.new_output_root,
            &journal.output_staging_root,
        )
        .unwrap();
        assert!(crate::index::migrate_campaign_index_at(
            &cache,
            &old_campaign,
            &new_campaign,
            &journal.old_output_root,
            &journal.new_output_root,
        )
        .unwrap());

        recover_campaign_renames_at(&campaigns, &cache).unwrap();

        assert!(journal.old_index_path.exists());
        assert!(!journal.new_index_path.exists());
        assert!(output.join("old/history.txt").exists());
        assert!(!campaigns.join(RENAME_JOURNAL).exists());
    }

    #[test]
    fn campaign_rename_recovery_prefers_old_identity_when_both_configs_exist() {
        let temp = tempdir().unwrap();
        let (campaigns, output, cache, _, new_campaign, journal) = recovery_fixture(temp.path());
        migrate_output_root_staged(
            &journal.old_output_root,
            &journal.new_output_root,
            &journal.output_staging_root,
        )
        .unwrap();
        new_campaign.save(&journal.new_config_path).unwrap();

        recover_campaign_renames_at(&campaigns, &cache).unwrap();

        assert!(journal.old_config_path.exists());
        assert!(!journal.new_config_path.exists());
        assert!(output.join("old/history.txt").exists());
        assert!(!campaigns.join(RENAME_JOURNAL).exists());
    }

    #[test]
    fn campaign_rename_recovery_finishes_after_old_config_removal() {
        let temp = tempdir().unwrap();
        let (campaigns, output, cache, old_campaign, new_campaign, journal) =
            recovery_fixture(temp.path());
        create_test_index(&journal.old_index_path);
        migrate_output_root_staged(
            &journal.old_output_root,
            &journal.new_output_root,
            &journal.output_staging_root,
        )
        .unwrap();
        assert!(crate::index::migrate_campaign_index_at(
            &cache,
            &old_campaign,
            &new_campaign,
            &journal.old_output_root,
            &journal.new_output_root,
        )
        .unwrap());
        new_campaign.save(&journal.new_config_path).unwrap();
        fs::remove_file(&journal.old_config_path).unwrap();

        recover_campaign_renames_at(&campaigns, &cache).unwrap();

        assert!(journal.new_config_path.exists());
        assert!(!journal.old_config_path.exists());
        assert!(output.join("new/history.txt").exists());
        assert!(!output.join("old").exists());
        assert!(journal.new_index_path.exists());
        assert!(!journal.old_index_path.exists());
        assert!(!campaigns.join(RENAME_JOURNAL).exists());
    }

    #[test]
    fn session_rename_moves_known_transcripts_and_complete_notes_directory() {
        let temp = tempdir().unwrap();
        let transcripts = temp.path().join("transcripts");
        let notes = temp.path().join("notes");
        fs::create_dir_all(notes.join("old-name")).unwrap();
        fs::create_dir_all(&transcripts).unwrap();
        for suffix in SESSION_TRANSCRIPT_SUFFIXES {
            fs::write(
                transcripts.join(format!("old-name{suffix}")),
                format!("contents for {suffix}"),
            )
            .unwrap();
        }
        for artifact in [
            "summary.md",
            "summary.candidate.md",
            "dm-notes.json",
            "summary-alt.md",
            "future-artifact.bin",
        ] {
            fs::write(notes.join("old-name").join(artifact), artifact).unwrap();
        }

        let plan = plan_session_rename(&transcripts, &notes, "old-name", "new-name").unwrap();
        apply_session_rename(&plan).unwrap();

        assert_eq!(
            plan.transcript_moves.len(),
            SESSION_TRANSCRIPT_SUFFIXES.len()
        );
        assert!(!notes.join("old-name").exists());
        for suffix in SESSION_TRANSCRIPT_SUFFIXES {
            assert!(!transcripts.join(format!("old-name{suffix}")).exists());
            assert_eq!(
                fs::read_to_string(transcripts.join(format!("new-name{suffix}"))).unwrap(),
                format!("contents for {suffix}")
            );
        }
        assert_eq!(
            fs::read_to_string(notes.join("new-name/future-artifact.bin")).unwrap(),
            "future-artifact.bin"
        );
    }

    #[test]
    fn session_rename_rejects_invalid_names_and_destination_collisions() {
        for invalid in [
            "",
            ".hidden",
            "../escape",
            "nested/name",
            "nested\\name",
            "bad\nname",
            "raw.diarized",
        ] {
            assert!(
                validate_session_stem(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
        assert!(validate_session_stem(&"x".repeat(101)).is_err());

        let temp = tempdir().unwrap();
        let transcripts = temp.path().join("transcripts");
        let notes = temp.path().join("notes");
        fs::create_dir_all(&transcripts).unwrap();
        fs::create_dir_all(&notes).unwrap();
        fs::write(transcripts.join("old.txt"), "source").unwrap();
        fs::write(transcripts.join("new.diarized.vtt"), "collision").unwrap();

        let error = plan_session_rename(&transcripts, &notes, "old", "new").unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert_eq!(
            fs::read_to_string(transcripts.join("old.txt")).unwrap(),
            "source"
        );
    }
}
