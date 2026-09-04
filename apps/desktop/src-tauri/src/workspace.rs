use crate::{
    commands::{self, CampaignSummary, SessionSummary},
    transcript,
};
use serde::Serialize;
use sessionsmith::{
    candidates,
    config::{self, CampaignConfig},
    llm::{self, ChatMessage, ChatOptions, Role},
    meta, pipeline, presets,
    prompts::{Artifact, ALL_ARTIFACTS},
    speakers, util,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSummary {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub candidate_available: bool,
    #[specta(type = Option<specta_typescript::Number>)]
    pub modified_at: Option<u64>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedArtifactSummary {
    pub artifact_id: String,
    pub filename: String,
    pub label: String,
    #[specta(type = Option<specta_typescript::Number>)]
    pub modified_at: Option<u64>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSummary {
    #[specta(type = specta_typescript::Number)]
    pub total_lines: usize,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionProvenance {
    pub model: String,
    pub engine: String,
    pub language: String,
    pub vad: bool,
    pub session_date: Option<String>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub created_at: Option<u64>,
    pub source_audio: Option<String>,
    pub source_files: Vec<String>,
    #[specta(type = specta_typescript::Number)]
    pub mapped_speakers: usize,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionWorkspace {
    pub campaign: CampaignSummary,
    pub session: SessionSummary,
    pub artifacts: Vec<ArtifactSummary>,
    pub saved_artifacts: Vec<SavedArtifactSummary>,
    pub provenance: Option<SessionProvenance>,
    pub transcript: Option<TranscriptSummary>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDocument {
    pub id: String,
    pub label: String,
    pub markdown: String,
    pub candidate: bool,
    #[specta(type = Option<specta_typescript::Number>)]
    pub modified_at: Option<u64>,
    pub revision: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactWriteResult {
    #[specta(type = Option<specta_typescript::Number>)]
    pub modified_at: Option<u64>,
    pub revision: String,
    pub index_warning: Option<String>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptLine {
    #[specta(type = specta_typescript::Number)]
    pub line_number: usize,
    pub text: String,
    pub t0: Option<f64>,
    pub t1: Option<f64>,
    pub speaker: Option<String>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPage {
    #[specta(type = specta_typescript::Number)]
    pub offset_line: usize,
    #[specta(type = specta_typescript::Number)]
    pub total_lines: usize,
    pub lines: Vec<TranscriptLine>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptFollowLocation {
    #[specta(type = Option<specta_typescript::Number>)]
    pub offset_line: Option<usize>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub line_number: Option<usize>,
    pub filtered_out: bool,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignLogDocument {
    pub markdown: String,
    #[specta(type = Option<specta_typescript::Number>)]
    pub modified_at: Option<u64>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerReview {
    pub stem: String,
    pub speakers: Vec<SpeakerReviewEntry>,
    pub suggested_names: Vec<String>,
    pub can_reset: bool,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerReviewEntry {
    pub label: String,
    pub samples: Vec<SpeakerReviewSample>,
    pub mapped_to: Option<String>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerReviewSample {
    pub text: String,
    #[specta(type = Option<specta_typescript::Number>)]
    pub start_ms: Option<u64>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub end_ms: Option<u64>,
}

pub(crate) fn session_workspace(
    campaign_id: String,
    stem: String,
) -> Result<SessionWorkspace, String> {
    let (campaign, session, paths) = load_session_context(&campaign_id, &stem)?;
    let transcript_path = paths.transcripts.join(format!("{stem}.txt"));

    Ok(SessionWorkspace {
        campaign,
        session,
        artifacts: artifact_summaries(&stem, &paths),
        saved_artifacts: saved_artifact_summaries(&stem, &paths),
        provenance: session_provenance(&paths, &stem),
        transcript: transcript_path.is_file().then(|| TranscriptSummary {
            total_lines: count_lines(&transcript_path),
        }),
    })
}

pub(crate) fn artifact_read(
    campaign_id: String,
    stem: String,
    artifact_id: String,
    candidate: bool,
    alternate_name: Option<String>,
) -> Result<ArtifactDocument, String> {
    let (_, _, paths) = load_session_context(&campaign_id, &stem)?;
    let artifact = Artifact::from_id(&artifact_id)
        .ok_or_else(|| format!("Unknown artifact '{artifact_id}'."))?;
    if candidate && alternate_name.is_some() {
        return Err("A saved version cannot also be a candidate artifact.".into());
    }
    let (path, label) = match alternate_name {
        Some(name) => (
            saved_artifact_path(&paths, &stem, artifact, &name)?,
            saved_artifact_label(artifact, &name),
        ),
        None => (
            artifact_path(&paths, &stem, artifact, candidate),
            artifact.label().to_string(),
        ),
    };
    let markdown = fs::read_to_string(&path).map_err(|error| {
        format!(
            "Could not read {} for session '{stem}': {error}",
            if candidate {
                "the candidate artifact"
            } else {
                "the artifact"
            }
        )
    })?;

    Ok(ArtifactDocument {
        id: artifact.id().into(),
        label,
        revision: util::content_revision(markdown.as_bytes()),
        markdown,
        candidate,
        modified_at: modified_at(&path),
    })
}

pub(crate) fn artifact_write(
    campaign_id: String,
    stem: String,
    artifact_id: String,
    markdown: String,
    expected_revision: String,
) -> Result<ArtifactWriteResult, String> {
    let campaign_id = campaign_id.trim();
    if !is_simple_identifier(campaign_id) {
        return Err("Select a valid campaign before editing a document.".into());
    }
    let stem = stem.trim();
    if !is_simple_identifier(stem) {
        return Err("Select a valid session document to edit.".into());
    }
    let artifact = Artifact::from_id(artifact_id.trim())
        .ok_or_else(|| "Select a known generated document to edit.".to_string())?;
    validate_artifact_write(&markdown, &expected_revision)?;

    let (_, _, paths) = load_session_context(campaign_id, stem)?;
    let path = artifact_path(&paths, stem, artifact, false);
    if !path.is_file() {
        return Err("The selected generated document no longer exists.".into());
    }
    util::atomic_replace_if_revision(&path, &expected_revision, markdown.as_bytes()).map_err(
        |error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                "This document changed on disk. Reload it before saving.".to_string()
            } else {
                format!("Could not save {}: {error}", artifact.label())
            }
        },
    )?;

    Ok(ArtifactWriteResult {
        modified_at: modified_at(&path),
        revision: util::content_revision(markdown.as_bytes()),
        index_warning: refresh_session_index(campaign_id, stem, &paths),
    })
}

pub(crate) fn transcript_read(
    campaign_id: String,
    stem: String,
    offset_line: usize,
    limit: usize,
    query: Option<String>,
) -> Result<TranscriptPage, String> {
    let (_, _, paths) = load_session_context(&campaign_id, &stem)?;
    let query = validate_transcript_query(query)?;
    let speaker_map = meta::load(&paths.transcripts, &stem)
        .and_then(|metadata| metadata.speaker_map)
        .unwrap_or_default();
    let transcript = transcript::filter(
        transcript::read(&paths.transcripts, &stem, &speaker_map)?,
        query.as_deref(),
    );
    let total_lines = transcript.len();
    let limit = transcript::bounded_page_size(limit);
    let lines = transcript
        .into_iter()
        .skip(offset_line)
        .take(limit)
        .enumerate()
        .map(|(index, entry)| TranscriptLine {
            line_number: offset_line + index + 1,
            text: entry.text,
            t0: entry.t0,
            t1: entry.t1,
            speaker: entry.speaker,
        })
        .collect();

    Ok(TranscriptPage {
        offset_line,
        total_lines,
        lines,
    })
}

pub(crate) fn transcript_locate(
    campaign_id: String,
    stem: String,
    position_ms: u32,
    page_size: usize,
    query: Option<String>,
) -> Result<TranscriptFollowLocation, String> {
    let (_, _, paths) = load_session_context(&campaign_id, &stem)?;
    let query = validate_transcript_query(query)?;
    let speaker_map = meta::load(&paths.transcripts, &stem)
        .and_then(|metadata| metadata.speaker_map)
        .unwrap_or_default();
    let transcript = transcript::read(&paths.transcripts, &stem, &speaker_map)?;
    let page_size = transcript::bounded_page_size(page_size);

    Ok(
        match transcript::locate_for_playback(
            &transcript,
            f64::from(position_ms) / 1_000.0,
            page_size,
            query.as_deref(),
        ) {
            transcript::FollowLocation::Match(location) => TranscriptFollowLocation {
                offset_line: Some(location.offset_line),
                line_number: Some(location.line_number),
                filtered_out: false,
            },
            transcript::FollowLocation::FilteredOut => TranscriptFollowLocation {
                offset_line: None,
                line_number: None,
                filtered_out: true,
            },
            transcript::FollowLocation::Unavailable => TranscriptFollowLocation {
                offset_line: None,
                line_number: None,
                filtered_out: false,
            },
        },
    )
}

fn validate_transcript_query(query: Option<String>) -> Result<Option<String>, String> {
    const MAX_QUERY_CHARS: usize = 160;

    let Some(query) = query else {
        return Ok(None);
    };
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }
    if query.chars().count() > MAX_QUERY_CHARS || query.chars().any(char::is_control) {
        return Err("Use a printable transcript filter of at most 160 characters.".into());
    }
    Ok(Some(query.into()))
}

pub(crate) fn campaign_log_read(campaign_id: String) -> Result<CampaignLogDocument, String> {
    commands::campaign_library(campaign_id.clone())?;
    let paths = campaign_paths(&campaign_id)?;
    let path = paths.notes.join("_campaign-log.md");
    let markdown = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read the campaign log: {error}"))?;

    Ok(CampaignLogDocument {
        markdown,
        modified_at: modified_at(&path),
    })
}

pub(crate) fn speaker_review(campaign_id: String, stem: String) -> Result<SpeakerReview, String> {
    let (_, _, paths) = load_session_context(&campaign_id, &stem)?;
    let campaign_path = campaign_config_path(&workspace_root(), &campaign_id)?;
    let campaign = CampaignConfig::load(&campaign_path).map_err(|error| error.to_string())?;
    let raw_txt = paths.transcripts.join(format!("{stem}.diarized.txt"));
    let txt = paths.transcripts.join(format!("{stem}.txt"));
    let source = if raw_txt.is_file() { raw_txt } else { txt };
    let text = fs::read_to_string(&source)
        .map_err(|error| format!("Could not read the transcript for speaker review: {error}"))?;
    let labels = speakers::labels(&text);
    let mut map = meta::load(&paths.transcripts, &stem)
        .and_then(|session| session.speaker_map)
        .unwrap_or_else(|| campaign.transcription.speakers.clone());
    map.retain(|label, _| labels.contains(label));
    let raw_srt = paths.transcripts.join(format!("{stem}.diarized.srt"));
    let srt = paths.transcripts.join(format!("{stem}.srt"));
    let timed_source = if raw_srt.is_file() { raw_srt } else { srt };
    let mut samples = BTreeMap::<String, Vec<SpeakerReviewSample>>::new();
    if let Ok(captions) = fs::read_to_string(timed_source) {
        for sample in speakers::detect_timed_samples(&captions) {
            samples
                .entry(sample.label)
                .or_default()
                .push(SpeakerReviewSample {
                    text: sample.text,
                    start_ms: Some((sample.start * 1_000.0).round() as u64),
                    end_ms: Some((sample.end * 1_000.0).round() as u64),
                });
        }
    }
    if samples.is_empty() {
        for sample in speakers::detect_samples(&text) {
            samples
                .entry(sample.label)
                .or_default()
                .push(SpeakerReviewSample {
                    text: sample.text,
                    start_ms: None,
                    end_ms: None,
                });
        }
    }

    let can_reset = ["txt", "srt", "vtt"].iter().any(|extension| {
        paths
            .transcripts
            .join(format!("{stem}.diarized.{extension}"))
            .is_file()
    });
    Ok(SpeakerReview {
        stem,
        can_reset,
        speakers: labels
            .into_iter()
            .map(|label| SpeakerReviewEntry {
                mapped_to: map.get(&label).cloned(),
                samples: samples.remove(&label).unwrap_or_default(),
                label,
            })
            .collect(),
        suggested_names: speaker_names(&campaign),
    })
}

pub(crate) async fn session_name_suggest(
    campaign_id: String,
    stem: String,
) -> Result<Vec<String>, String> {
    let (_, _, paths) = load_session_context(&campaign_id, &stem)?;
    let campaign_path = campaign_config_path(&workspace_root(), &campaign_id)?;
    let campaign = CampaignConfig::load(&campaign_path).map_err(|error| error.to_string())?;
    let source = session_name_source(&paths, &stem)?;
    let global = config::GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    let effective = config::effective(&global, &campaign);
    let preset = presets::load(&campaign.system.preset).map_err(|error| error.to_string())?;
    let backend = llm::build(&effective).map_err(|error| error.to_string())?;
    let model = effective
        .backend
        .model
        .clone()
        .ok_or_else(|| "No LLM model is configured for note generation.".to_string())?;
    let response = llm::collect(
        backend.as_ref(),
        vec![
            ChatMessage {
                role: Role::System,
                content: sessionsmith::prompts::session_name_suggest_system(&campaign, &preset),
            },
            ChatMessage {
                role: Role::User,
                content: sessionsmith::prompts::user_session_name_suggest(&source),
            },
        ],
        ChatOptions {
            model,
            temperature: Some(0.4),
            max_tokens: Some(256),
            timeout: Duration::from_secs(effective.runtime.timeout_secs),
            think: effective.runtime.think,
            num_ctx: effective.effective_num_ctx(),
            format: Some(serde_json::json!({
                "type": "array",
                "items": { "type": "string" },
                "minItems": 5,
                "maxItems": 5
            })),
        },
        None,
    )
    .await
    .map_err(|error| error.to_string())?;
    let suggestions = sessionsmith::prompts::parse_session_name_suggestions(&response);
    if suggestions.is_empty() {
        return Err("The notes model did not return any valid session names.".into());
    }
    Ok(suggestions)
}

fn session_name_source(paths: &CampaignPaths, stem: &str) -> Result<String, String> {
    for filename in ["summary.md", "bullets.md"] {
        let path = paths.notes.join(stem).join(filename);
        if let Ok(content) = fs::read_to_string(&path) {
            if !content.trim().is_empty() {
                return Ok(content);
            }
        }
    }
    let transcript = paths.transcripts.join(format!("{stem}.txt"));
    let content = fs::read_to_string(&transcript).map_err(|error| {
        format!("Could not read session material for name suggestions: {error}")
    })?;
    let excerpt = content.chars().take(4_000).collect::<String>();
    if excerpt.trim().is_empty() {
        return Err("This session has no summary, bullets, or transcript text to name.".into());
    }
    Ok(excerpt)
}

fn load_session_context(
    campaign_id: &str,
    stem: &str,
) -> Result<(CampaignSummary, SessionSummary, CampaignPaths), String> {
    let library = commands::campaign_library(campaign_id.to_string())?;
    let session = library
        .sessions
        .into_iter()
        .find(|session| session.stem == stem)
        .ok_or_else(|| format!("Session '{stem}' was not found."))?;
    let paths = campaign_paths(campaign_id)?;

    Ok((library.campaign, session, paths))
}

fn artifact_summaries(stem: &str, paths: &CampaignPaths) -> Vec<ArtifactSummary> {
    ALL_ARTIFACTS
        .iter()
        .map(|artifact| {
            let path = artifact_path(paths, stem, *artifact, false);
            let candidate_path = artifact_path(paths, stem, *artifact, true);
            ArtifactSummary {
                id: artifact.id().into(),
                label: artifact.label().into(),
                available: path.is_file(),
                candidate_available: candidate_path.is_file(),
                modified_at: modified_at(&path),
            }
        })
        .collect()
}

fn artifact_path(
    paths: &CampaignPaths,
    stem: &str,
    artifact: Artifact,
    candidate: bool,
) -> PathBuf {
    paths
        .notes
        .join(stem)
        .join(pipeline::artifact_file(artifact, candidate))
}

fn validate_artifact_write(markdown: &str, expected_revision: &str) -> Result<(), String> {
    const MAX_ARTIFACT_BYTES: usize = 4 * 1024 * 1024;

    if markdown.len() > MAX_ARTIFACT_BYTES {
        return Err("A document edit cannot exceed 4 MiB.".into());
    }
    if markdown
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("Document edits cannot contain control characters.".into());
    }
    if expected_revision.len() != 64
        || !expected_revision
            .bytes()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err("Reload the document before saving it.".into());
    }
    Ok(())
}

fn is_simple_identifier(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && !value.chars().any(char::is_control)
        && Path::new(value).file_name().and_then(|name| name.to_str()) == Some(value)
}

fn refresh_session_index(campaign_id: &str, stem: &str, paths: &CampaignPaths) -> Option<String> {
    let result = (|| -> Result<(), String> {
        let campaign_path = campaign_config_path(&workspace_root(), campaign_id)?;
        let campaign = CampaignConfig::load(&campaign_path).map_err(|error| error.to_string())?;
        let global = config::GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
        if !config::effective(&global, &campaign).runtime.index {
            return Ok(());
        }
        sessionsmith::index::record_session(&campaign, stem, &paths.notes.join(stem))
            .map_err(|error| error.to_string())
    })();
    result
        .err()
        .map(|error| format!("Search index needs rebuilding: {error}"))
}

fn saved_artifact_summaries(stem: &str, paths: &CampaignPaths) -> Vec<SavedArtifactSummary> {
    let notes_dir = paths.notes.join(stem);
    let mut saved_artifacts = fs::read_dir(notes_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let filename = path.file_name()?.to_str()?.to_string();
            let artifact = ALL_ARTIFACTS.iter().copied().find(|artifact| {
                candidates::is_alternate_filename(artifact.filename(), &filename)
            })?;
            path.is_file().then(|| SavedArtifactSummary {
                artifact_id: artifact.id().to_string(),
                label: saved_artifact_label(artifact, &filename),
                filename,
                modified_at: modified_at(&path),
            })
        })
        .collect::<Vec<_>>();
    saved_artifacts.sort_by(|left, right| left.filename.cmp(&right.filename));
    saved_artifacts
}

fn saved_artifact_path(
    paths: &CampaignPaths,
    stem: &str,
    artifact: Artifact,
    name: &str,
) -> Result<PathBuf, String> {
    if !candidates::is_alternate_filename(artifact.filename(), name) {
        return Err("Select a saved version generated for this artifact.".into());
    }
    let path = paths.notes.join(stem).join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err("The selected saved version no longer exists.".into())
    }
}

fn saved_artifact_label(artifact: Artifact, name: &str) -> String {
    let filename = artifact.filename();
    let (stem, extension) = filename
        .rsplit_once('.')
        .expect("built-in artifact filenames have extensions");
    let prefix = format!("{stem}-alt");
    let numbered_suffix = name
        .strip_prefix(&format!("{prefix}-"))
        .and_then(|suffix| suffix.strip_suffix(&format!(".{extension}")));
    match numbered_suffix {
        Some(number) => format!("{} saved version {number}", artifact.label()),
        None => format!("{} saved version", artifact.label()),
    }
}

fn session_provenance(paths: &CampaignPaths, stem: &str) -> Option<SessionProvenance> {
    meta::load(&paths.transcripts, stem).map(|metadata| SessionProvenance {
        model: sanitize_provenance_text(&metadata.model, 160),
        engine: sanitize_provenance_text(&metadata.engine, 160),
        language: sanitize_provenance_text(&metadata.language, 64),
        vad: metadata.vad,
        session_date: metadata
            .session_date
            .as_deref()
            .map(|date| sanitize_provenance_text(date, 32))
            .filter(|date| !date.is_empty()),
        created_at: u64::try_from(metadata.created).ok(),
        source_audio: metadata.source_audio.as_deref().and_then(display_file_name),
        source_files: metadata
            .source_files
            .iter()
            .filter_map(|path| display_file_name(path))
            .take(20)
            .collect(),
        mapped_speakers: metadata.speaker_map.as_ref().map_or(0, BTreeMap::len),
    })
}

fn display_file_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    let name = sanitize_provenance_text(&name, 180);
    (!name.is_empty()).then_some(name)
}

fn sanitize_provenance_text(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter_map(|character| {
            if character.is_control() {
                character.is_whitespace().then_some(' ')
            } else {
                Some(character)
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

struct CampaignPaths {
    transcripts: PathBuf,
    notes: PathBuf,
}

fn campaign_paths(campaign_id: &str) -> Result<CampaignPaths, String> {
    let root = workspace_root();
    let campaign_path = campaign_config_path(&root, campaign_id)?;
    let campaign = CampaignConfig::load(&campaign_path).map_err(|error| error.to_string())?;
    let output_root = resolve_workspace_path(&root, &config::output_dir()).join(campaign.slug());

    Ok(CampaignPaths {
        transcripts: output_root.join("transcripts"),
        notes: output_root.join("notes"),
    })
}

pub(crate) fn campaign_config_path(root: &Path, campaign_id: &str) -> Result<PathBuf, String> {
    let campaigns_dir = resolve_workspace_path(root, &config::campaigns_dir());
    let campaign_path = fs::read_dir(&campaigns_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("toml")
                && path.file_stem().and_then(|stem| stem.to_str()) == Some(campaign_id)
        })
        .or_else(|| {
            let root_campaign = root.join("campaign.toml");
            (root_campaign.is_file()
                && root_campaign.file_stem().and_then(|stem| stem.to_str()) == Some(campaign_id))
            .then_some(root_campaign)
        });

    campaign_path.ok_or_else(|| format!("Campaign '{campaign_id}' was not found."))
}

pub(crate) fn workspace_root() -> PathBuf {
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

fn resolve_workspace_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn count_lines(path: &Path) -> usize {
    fs::File::open(path)
        .ok()
        .map(BufReader::new)
        .map(|reader| reader.lines().count())
        .unwrap_or_default()
}

fn modified_at(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_epoch)
}

fn speaker_names(campaign: &CampaignConfig) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = BTreeSet::new();
    let gm = campaign.campaign.gm.trim();
    if !gm.is_empty() && seen.insert(gm.to_lowercase()) {
        names.push(gm.to_string());
    }
    for player in &campaign.players {
        let name = player.player.trim();
        if !name.is_empty() && seen.insert(name.to_lowercase()) {
            names.push(name.to_string());
        }
    }
    names
}

fn system_time_epoch(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::{
        artifact_write, sanitize_provenance_text, saved_artifact_label, saved_artifact_path,
        session_name_source, speaker_names, validate_artifact_write, validate_transcript_query,
        CampaignPaths,
    };
    use sessionsmith::config::{CampaignConfig, Player};
    use sessionsmith::prompts::Artifact;

    #[test]
    fn speaker_suggestions_are_the_gm_and_player_names_in_campaign_order() {
        let mut campaign = CampaignConfig::default();
        campaign.campaign.gm = " Michael ".into();
        campaign.players = vec![
            Player {
                player: "Emilie".into(),
                character: "Fatethrial".into(),
                ..Player::default()
            },
            Player {
                player: "Ravn".into(),
                character: "Jan Simen".into(),
                ..Player::default()
            },
        ];

        assert_eq!(speaker_names(&campaign), ["Michael", "Emilie", "Ravn"]);
    }

    #[test]
    fn saved_artifacts_only_accept_known_alternate_filenames() {
        let root = std::env::temp_dir().join(format!(
            "sessionsmith-desktop-saved-artifact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let notes = root.join("notes");
        let session = notes.join("session-1");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("summary-alt.md"), "saved").unwrap();
        let paths = CampaignPaths {
            transcripts: root.join("transcripts"),
            notes,
        };

        assert!(
            saved_artifact_path(&paths, "session-1", Artifact::Summary, "summary-alt.md").is_ok()
        );
        assert!(
            saved_artifact_path(&paths, "session-1", Artifact::Summary, "recap-alt.md").is_err()
        );
        assert!(
            saved_artifact_path(&paths, "session-1", Artifact::Summary, "../summary-alt.md")
                .is_err()
        );
        assert_eq!(
            saved_artifact_label(Artifact::Summary, "summary-alt-2.md"),
            "Summary saved version 2"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn provenance_display_text_is_bounded_and_path_free() {
        assert_eq!(
            sanitize_provenance_text("  engine\nname\u{0000}  ", 20),
            "engine name"
        );
        assert_eq!(sanitize_provenance_text("abcdef", 4), "abcd");
    }

    #[test]
    fn session_name_source_prefers_notes_and_bounds_transcript_fallback() {
        let root = std::env::temp_dir().join(format!(
            "sessionsmith-desktop-session-name-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let paths = CampaignPaths {
            transcripts: root.join("transcripts"),
            notes: root.join("notes"),
        };
        std::fs::create_dir_all(paths.notes.join("session-1")).unwrap();
        std::fs::create_dir_all(&paths.transcripts).unwrap();
        std::fs::write(paths.transcripts.join("session-1.txt"), "x".repeat(5_000)).unwrap();
        assert_eq!(
            session_name_source(&paths, "session-1")
                .unwrap()
                .chars()
                .count(),
            4_000
        );

        std::fs::write(paths.notes.join("session-1/bullets.md"), "bullet source").unwrap();
        assert_eq!(
            session_name_source(&paths, "session-1").unwrap(),
            "bullet source"
        );
        std::fs::write(paths.notes.join("session-1/summary.md"), "summary source").unwrap();
        assert_eq!(
            session_name_source(&paths, "session-1").unwrap(),
            "summary source"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artifact_write_input_requires_a_content_revision_and_safe_markdown() {
        assert!(validate_artifact_write("# Edited", &"a".repeat(64)).is_ok());
        assert!(validate_artifact_write("# Edited", "bad-revision").is_err());
        assert!(validate_artifact_write("edited\u{0000}", &"a".repeat(64)).is_err());
    }

    #[test]
    fn artifact_write_rejects_untrusted_identity_before_resolving_paths() {
        let revision = "a".repeat(64);

        assert!(artifact_write(
            "../campaign".into(),
            "session-1".into(),
            "summary".into(),
            "edited".into(),
            revision.clone(),
        )
        .is_err());
        assert!(artifact_write(
            "campaign".into(),
            "../session".into(),
            "summary".into(),
            "edited".into(),
            revision.clone(),
        )
        .is_err());
        assert!(artifact_write(
            "campaign".into(),
            "session-1".into(),
            "not-an-artifact".into(),
            "edited".into(),
            revision,
        )
        .is_err());
    }

    #[test]
    fn transcript_filter_is_bounded_and_printable() {
        assert_eq!(
            validate_transcript_query(Some("  Alice  ".into())).unwrap(),
            Some("Alice".into())
        );
        assert!(validate_transcript_query(Some("invalid\u{0000}".into())).is_err());
        assert!(validate_transcript_query(Some("a".repeat(161))).is_err());
    }
}
