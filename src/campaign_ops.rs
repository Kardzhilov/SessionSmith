//! Filesystem-safe operations for creating, forking, and renaming campaigns.
//!
//! These helpers have no TUI dependency so their collision and rollback behavior
//! can be tested against temporary directories.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::config::{
    Campaign, CampaignConfig, OutputsConfig, Player, PromptOverrides, SystemRef,
    TranscriptionConfig,
};

const FORK_EXCLUDES: &[&str] = &["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"];

/// Result of a successful campaign fork.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkOutcome {
    pub config_path: PathBuf,
    pub files_copied: usize,
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

fn validate_campaign_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("campaign name cannot be empty");
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
}
