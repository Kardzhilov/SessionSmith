//! Per-session transcription metadata sidecar.
//!
//! Written next to a transcript as `transcripts/<stem>.ssmeta.json`. It records
//! which ASR model produced the transcript (so a re-run can skip transcription
//! when the model is unchanged) and the source audio file (so the TUI audio
//! player can seek into it at a quote's timestamp).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadSpan {
    /// Start offset in the original audio, in seconds.
    pub start: f64,
    /// Amount of audio removed, in seconds.
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    /// ASR model id used (e.g. `large-v3-turbo`, `parakeet-v3`).
    pub model: String,
    /// Human label of the engine that ran it.
    #[serde(default)]
    pub engine: String,
    /// Language hint passed to ASR.
    #[serde(default)]
    pub language: String,
    /// The audio file that was transcribed (for playback at timestamps).
    #[serde(default)]
    pub source_audio: Option<PathBuf>,
    /// Original inputs in their session order.
    #[serde(default)]
    pub source_files: Vec<PathBuf>,
    /// Whether VAD was requested for this transcription.
    #[serde(default)]
    pub vad: bool,
    /// Silence intervals removed before ASR, expressed in original time.
    #[serde(default)]
    pub vad_removed_spans: Vec<VadSpan>,
    /// Names selected for diarized speaker labels in this session.
    #[serde(default)]
    pub speaker_map: Option<BTreeMap<String, String>>,
    /// Calendar date on which the session was played (`YYYY-MM-DD`).
    #[serde(default)]
    pub session_date: Option<String>,
    /// Unix seconds when the transcript was produced.
    #[serde(default)]
    pub created: i64,
}

/// Path of the metadata sidecar for `stem` inside `transcripts_dir`.
pub fn path(transcripts_dir: &Path, stem: &str) -> PathBuf {
    transcripts_dir.join(format!("{stem}.ssmeta.json"))
}

/// Load metadata for a session, if present and parseable.
pub fn load(transcripts_dir: &Path, stem: &str) -> Option<SessionMeta> {
    let p = path(transcripts_dir, stem);
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write metadata for a session (best-effort; errors are ignored by callers).
pub fn save(transcripts_dir: &Path, stem: &str, meta: &SessionMeta) -> std::io::Result<()> {
    let p = path(transcripts_dir, stem);
    let text = serde_json::to_string_pretty(meta).unwrap_or_default();
    std::fs::write(p, text)
}

/// Current unix time in seconds.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
