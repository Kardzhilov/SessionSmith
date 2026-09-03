use serde::Serialize;
use sessionsmith::{
    audio,
    config::{self, CampaignConfig, GlobalConfig},
    meta,
    prompts::ALL_ARTIFACTS,
    speakers,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignSummary {
    pub id: String,
    pub name: String,
    pub gm: String,
    pub setting: String,
    pub preset_id: String,
    pub backend_kind: String,
    #[specta(type = specta_typescript::Number)]
    pub session_count: usize,
    pub has_campaign_log: bool,
    pub load_error: Option<String>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    pub app_name: &'static str,
    pub version: &'static str,
    pub workspace_path: String,
    pub campaigns: Vec<CampaignSummary>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub stem: String,
    pub has_audio: bool,
    pub has_transcript: bool,
    pub artifacts: Vec<String>,
    pub stage: &'static str,
    #[specta(type = Option<specta_typescript::Number>)]
    pub modified_at: Option<u64>,
    #[specta(type = specta_typescript::Number)]
    pub unmapped_speaker_count: usize,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InboxAudio {
    pub name: String,
    pub path: String,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: u64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub modified_at: Option<u64>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignLibrary {
    pub campaign: CampaignSummary,
    pub sessions: Vec<SessionSummary>,
    pub inbox: Vec<InboxAudio>,
}

struct CampaignRecord {
    summary: CampaignSummary,
    config: Option<CampaignConfig>,
}

#[specta::specta]
#[tauri::command]
pub fn app_bootstrap() -> Result<AppBootstrap, String> {
    let (root, global) = load_context()?;
    Ok(AppBootstrap {
        app_name: "SessionSmith",
        version: env!("CARGO_PKG_VERSION"),
        workspace_path: root.display().to_string(),
        campaigns: campaign_records(&root, &global)
            .into_iter()
            .map(|record| record.summary)
            .collect(),
    })
}

pub(crate) fn recover_campaign_renames() -> Result<(), String> {
    let root = workspace_root();
    let output_root = resolve_workspace_path(&root, &config::output_dir());
    sessionsmith::campaign_ops::recover_campaign_renames(&root.join("campaigns"), &output_root)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn campaign_library(campaign_id: String) -> Result<CampaignLibrary, String> {
    let (root, global) = load_context()?;
    let record = campaign_records(&root, &global)
        .into_iter()
        .find(|record| record.summary.id == campaign_id)
        .ok_or_else(|| format!("Campaign '{campaign_id}' was not found."))?;

    let config = record.config.ok_or_else(|| {
        record
            .summary
            .load_error
            .clone()
            .unwrap_or_else(|| "Campaign could not be loaded.".into())
    })?;
    let paths = campaign_paths(&root, &config);
    let stems = session_stems(&paths.transcripts, &paths.notes);
    let sessions = session_summaries(&stems, &paths, &config.transcription.speakers);
    let inbox = inbox_audio(&stems, &paths);

    Ok(CampaignLibrary {
        campaign: record.summary,
        sessions,
        inbox,
    })
}

fn load_context() -> Result<(PathBuf, GlobalConfig), String> {
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    Ok((workspace_root(), global))
}

fn workspace_root() -> PathBuf {
    if let Some(path) = std::env::var_os("SESSIONSMITH_WORKSPACE") {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return path;
        }
    }

    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("desktop app must live at apps/desktop/src-tauri")
        .to_path_buf()
}

fn campaign_records(root: &Path, global: &GlobalConfig) -> Vec<CampaignRecord> {
    let mut records: Vec<_> = campaign_config_paths(root)
        .into_iter()
        .map(|path| campaign_record(root, global, path))
        .collect();

    records.sort_by_key(|record| {
        let saved_position = global
            .ui
            .campaign_order
            .iter()
            .position(|id| id == &record.summary.id)
            .unwrap_or(usize::MAX);
        (saved_position, record.summary.name.to_lowercase())
    });
    records
}

fn campaign_config_paths(root: &Path) -> Vec<PathBuf> {
    let campaigns_dir = root.join("campaigns");
    let mut paths: Vec<_> = fs::read_dir(campaigns_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("toml")
                && !path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('.'))
        })
        .collect();
    paths.sort();

    if paths.is_empty() {
        let root_campaign = root.join("campaign.toml");
        if root_campaign.is_file() {
            paths.push(root_campaign);
        }
    }
    paths
}

fn campaign_record(root: &Path, global: &GlobalConfig, path: PathBuf) -> CampaignRecord {
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("campaign")
        .to_string();

    match CampaignConfig::load(&path) {
        Ok(config) => {
            let paths = campaign_paths(root, &config);
            let session_count = session_stems(&paths.transcripts, &paths.notes).len();
            let summary = CampaignSummary {
                id,
                name: config.campaign.name.clone(),
                gm: config.campaign.gm.clone(),
                setting: config.campaign.setting.clone(),
                preset_id: config.system.preset.clone(),
                backend_kind: config
                    .backend
                    .kind
                    .clone()
                    .unwrap_or_else(|| global.backend.kind.clone()),
                session_count,
                has_campaign_log: paths.notes.join("_campaign-log.md").is_file(),
                load_error: None,
            };
            CampaignRecord {
                summary,
                config: Some(config),
            }
        }
        Err(error) => CampaignRecord {
            summary: CampaignSummary {
                name: id.clone(),
                id,
                gm: String::new(),
                setting: String::new(),
                preset_id: String::new(),
                backend_kind: global.backend.kind.clone(),
                session_count: 0,
                has_campaign_log: false,
                load_error: Some(error.to_string()),
            },
            config: None,
        },
    }
}

struct CampaignPaths {
    audio: PathBuf,
    transcripts: PathBuf,
    notes: PathBuf,
}

fn campaign_paths(root: &Path, campaign: &CampaignConfig) -> CampaignPaths {
    let output_root = resolve_workspace_path(root, &config::output_dir()).join(campaign.slug());
    CampaignPaths {
        audio: resolve_workspace_path(root, &config::audio_dir()),
        transcripts: output_root.join("transcripts"),
        notes: output_root.join("notes"),
    }
}

fn resolve_workspace_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn session_stems(transcripts: &Path, notes: &Path) -> BTreeSet<String> {
    let mut stems = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(transcripts) {
        for path in entries.flatten().map(|entry| entry.path()) {
            if path.extension().and_then(|extension| extension.to_str()) == Some("txt") {
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    if !speakers::is_raw_diarized_stem(stem) {
                        stems.insert(stem.to_string());
                    }
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(notes) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(stem) = entry.file_name().to_str() {
                    if !stem.starts_with('_') && !stem.starts_with('.') {
                        stems.insert(stem.to_string());
                    }
                }
            }
        }
    }
    stems
}

fn session_summaries(
    stems: &BTreeSet<String>,
    paths: &CampaignPaths,
    speaker_defaults: &BTreeMap<String, String>,
) -> Vec<SessionSummary> {
    let mut sessions: Vec<_> = stems
        .iter()
        .map(|stem| {
            let transcript = paths.transcripts.join(format!("{stem}.txt"));
            let note_dir = paths.notes.join(stem);
            let artifacts: Vec<_> = ALL_ARTIFACTS
                .iter()
                .filter(|artifact| note_dir.join(artifact.filename()).is_file())
                .map(|artifact| artifact.id().to_string())
                .collect();
            let has_transcript = transcript.is_file();
            let has_audio = audio::find_by_stem(&paths.audio, stem).is_some();
            let mut m_at = modified_at(&transcript);
            for artifact in ALL_ARTIFACTS {
                m_at = latest(m_at, modified_at(&note_dir.join(artifact.filename())));
            }

            SessionSummary {
                stem: stem.clone(),
                has_audio,
                has_transcript,
                stage: session_stage(has_audio, has_transcript, &artifacts),
                artifacts,
                modified_at: m_at,
                unmapped_speaker_count: unmapped_speaker_count(
                    &paths.transcripts,
                    stem,
                    speaker_defaults,
                ),
            }
        })
        .collect();
    sessions.sort_by_key(|session| std::cmp::Reverse(session.modified_at.unwrap_or_default()));
    sessions
}

fn unmapped_speaker_count(
    transcripts: &Path,
    stem: &str,
    speaker_defaults: &BTreeMap<String, String>,
) -> usize {
    let raw = transcripts.join(format!("{stem}.diarized.txt"));
    let live = transcripts.join(format!("{stem}.txt"));
    let source = if raw.is_file() { raw } else { live };
    let Ok(text) = fs::read_to_string(source) else {
        return 0;
    };
    let mapped = meta::load(transcripts, stem)
        .and_then(|session| session.speaker_map)
        .unwrap_or_else(|| speaker_defaults.clone());
    speakers::labels(&text)
        .into_iter()
        .filter(|label| !mapped.contains_key(label))
        .count()
}

fn inbox_audio(stems: &BTreeSet<String>, paths: &CampaignPaths) -> Vec<InboxAudio> {
    audio::scan(&paths.audio, &paths.transcripts)
        .unwrap_or_default()
        .into_iter()
        .filter(|audio| !stems.contains(&audio.stem()))
        .map(|audio| InboxAudio {
            name: audio
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("audio")
                .to_string(),
            path: audio.path.display().to_string(),
            size_bytes: audio.size_bytes,
            modified_at: system_time_epoch(audio.mtime),
        })
        .collect()
}

fn session_stage(has_audio: bool, has_transcript: bool, artifacts: &[String]) -> &'static str {
    if !artifacts.is_empty() {
        "notes"
    } else if has_transcript {
        "transcript"
    } else if has_audio {
        "audio"
    } else {
        "unknown"
    }
}

fn modified_at(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_epoch)
}

fn system_time_epoch(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn latest(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{session_stage, session_stems, unmapped_speaker_count};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;

    #[test]
    fn notes_take_precedence_over_transcript_and_audio() {
        assert_eq!(session_stage(true, true, &["summary".into()]), "notes");
    }

    #[test]
    fn stage_describes_the_first_missing_pipeline_step() {
        assert_eq!(session_stage(true, false, &[]), "audio");
        assert_eq!(session_stage(false, true, &[]), "transcript");
        assert_eq!(session_stage(false, false, &[]), "unknown");
    }

    #[test]
    fn raw_diarized_backups_are_not_library_sessions() {
        let directory = std::env::temp_dir().join(format!(
            "sessionsmith-library-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let transcripts = directory.join("transcripts");
        let notes = directory.join("notes");
        fs::create_dir_all(&transcripts).unwrap();
        fs::create_dir_all(&notes).unwrap();
        fs::write(transcripts.join("session.txt"), "Alice: Hello").unwrap();
        fs::write(
            transcripts.join("session.diarized.txt"),
            "SPEAKER_00: Hello\nSPEAKER_01: Hi",
        )
        .unwrap();

        assert_eq!(
            session_stems(&transcripts, &notes),
            BTreeSet::from(["session".to_string()])
        );
        assert_eq!(
            unmapped_speaker_count(&transcripts, "session", &BTreeMap::new()),
            2
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
