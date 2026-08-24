use sessionsmith::{
    audio,
    config::{self, CampaignConfig},
    meta,
    util,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioPlayerSnapshot {
    pub status: String,
    pub label: Option<String>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct DesktopAudioPlayer {
    inner: Arc<Mutex<NativePlayerState>>,
}

struct NativePlayerState {
    source: Option<ResolvedAudioSource>,
    child: Option<Child>,
    status: PlaybackStatus,
    position_ms: u64,
    started_at: Option<Instant>,
    duration_ms: Option<u64>,
    error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaybackStatus {
    Unloaded,
    Paused,
    Playing,
    Stopped,
    Ended,
}

impl Default for NativePlayerState {
    fn default() -> Self {
        Self {
            source: None,
            child: None,
            status: PlaybackStatus::Unloaded,
            position_ms: 0,
            started_at: None,
            duration_ms: None,
            error: None,
        }
    }
}

impl DesktopAudioPlayer {
    /// Resolve a selected session source, then leave it paused for an explicit
    /// transport action. The source path never leaves this module.
    pub(crate) fn load(
        &self,
        campaign_id: &str,
        stem: &str,
    ) -> Result<AudioPlayerSnapshot, String> {
        let source = resolve_source(campaign_id, stem).map_err(|error| self.record_error(error))?;
        if playback_backend().is_none() {
            return Err(self.record_error(
                "No supported desktop audio player was found. Install ffmpeg or mpv.".into(),
            ));
        }
        let duration_ms = audio::probe_duration(&source.path)
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .map(|duration| seconds_to_millis(duration));

        let mut state = self.lock();
        state.clear_child();
        state.source = Some(source);
        state.status = PlaybackStatus::Paused;
        state.position_ms = 0;
        state.started_at = None;
        state.duration_ms = duration_ms;
        state.error = None;
        Ok(state.snapshot())
    }

    pub(crate) fn play(&self) -> Result<AudioPlayerSnapshot, String> {
        let mut state = self.lock();
        state.refresh_completion();
        let Some(source) = state.source.clone() else {
            return Err(state.fail("Load a session before starting playback."));
        };
        if state.status == PlaybackStatus::Playing {
            return Ok(state.snapshot());
        }
        let position_ms = if state.status == PlaybackStatus::Ended {
            0
        } else {
            state.position_ms
        };
        let child = spawn_player(&source.path, position_ms).map_err(|error| state.fail(&error))?;
        state.clear_child();
        state.child = Some(child);
        state.status = PlaybackStatus::Playing;
        state.position_ms = position_ms;
        state.started_at = Some(Instant::now());
        state.error = None;
        Ok(state.snapshot())
    }

    pub(crate) fn pause(&self) -> Result<AudioPlayerSnapshot, String> {
        let mut state = self.lock();
        state.refresh_completion();
        if state.source.is_none() {
            return Err(state.fail("Load a session before pausing playback."));
        }
        if state.status == PlaybackStatus::Playing {
            state.position_ms = state.current_position_ms();
            state.clear_child();
            state.started_at = None;
            state.status = PlaybackStatus::Paused;
        }
        state.error = None;
        Ok(state.snapshot())
    }

    pub(crate) fn seek(&self, position_ms: u64) -> Result<AudioPlayerSnapshot, String> {
        let mut state = self.lock();
        state.refresh_completion();
        let Some(source) = state.source.clone() else {
            return Err(state.fail("Load a session before seeking playback."));
        };
        let target_ms = clamp_position(position_ms, state.duration_ms);
        let resume = state.status == PlaybackStatus::Playing;
        state.clear_child();
        state.position_ms = target_ms;
        state.started_at = None;
        if resume {
            let child = spawn_player(&source.path, target_ms).map_err(|error| state.fail(&error))?;
            state.child = Some(child);
            state.status = PlaybackStatus::Playing;
            state.started_at = Some(Instant::now());
        } else {
            state.status = PlaybackStatus::Paused;
        }
        state.error = None;
        Ok(state.snapshot())
    }

    pub(crate) fn stop(&self) -> AudioPlayerSnapshot {
        let mut state = self.lock();
        state.refresh_completion();
        if state.source.is_some() {
            state.clear_child();
            state.position_ms = 0;
            state.started_at = None;
            state.status = PlaybackStatus::Stopped;
            state.error = None;
        }
        state.snapshot()
    }

    pub(crate) fn snapshot(&self) -> AudioPlayerSnapshot {
        let mut state = self.lock();
        state.refresh_completion();
        state.snapshot()
    }

    fn record_error(&self, message: String) -> String {
        self.lock().error = Some(message.clone());
        message
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, NativePlayerState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl NativePlayerState {
    fn clear_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn fail(&mut self, message: &str) -> String {
        self.error = Some(message.into());
        message.into()
    }

    fn refresh_completion(&mut self) {
        if self.status != PlaybackStatus::Playing {
            return;
        }
        let completed = match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(Some(_)) | Err(_)),
            None => true,
        };
        if completed {
            self.position_ms = self.duration_ms.unwrap_or_else(|| self.current_position_ms());
            self.child = None;
            self.started_at = None;
            self.status = PlaybackStatus::Ended;
        }
    }

    fn current_position_ms(&self) -> u64 {
        let position_ms = self.position_ms.saturating_add(
            self.started_at
                .filter(|_| self.status == PlaybackStatus::Playing)
                .map(|started_at| seconds_to_millis(started_at.elapsed().as_secs_f64()))
                .unwrap_or_default(),
        );
        clamp_position(position_ms, self.duration_ms)
    }

    fn snapshot(&self) -> AudioPlayerSnapshot {
        AudioPlayerSnapshot {
            status: match self.status {
                PlaybackStatus::Unloaded => "unloaded",
                PlaybackStatus::Paused => "paused",
                PlaybackStatus::Playing => "playing",
                PlaybackStatus::Stopped => "stopped",
                PlaybackStatus::Ended => "ended",
            }
            .into(),
            label: self.source.as_ref().map(|source| source.label.clone()),
            position_ms: self.current_position_ms(),
            duration_ms: self.duration_ms,
            error: self.error.clone(),
        }
    }
}

impl Drop for NativePlayerState {
    fn drop(&mut self) {
        self.clear_child();
    }
}

fn playback_backend() -> Option<PlayerBackend> {
    util::find_in_path("ffplay")
        .map(PlayerBackend::Ffplay)
        .or_else(|| util::find_in_path("mpv").map(PlayerBackend::Mpv))
}

enum PlayerBackend {
    Ffplay(PathBuf),
    Mpv(PathBuf),
}

fn spawn_player(path: &Path, position_ms: u64) -> Result<Child, String> {
    let offset = format!("{:.3}", position_ms as f64 / 1000.0);
    let mut command = match playback_backend() {
        Some(PlayerBackend::Ffplay(program)) => {
            let mut command = Command::new(program);
            command
                .args(["-nodisp", "-autoexit", "-nostdin", "-loglevel", "error", "-ss"])
                .arg(offset)
                .arg(path);
            command
        }
        Some(PlayerBackend::Mpv(program)) => {
            let mut command = Command::new(program);
            command
                .args(["--no-video", "--really-quiet"])
                .arg(format!("--start={offset}"))
                .arg(path);
            command
        }
        None => {
            return Err("No supported desktop audio player was found. Install ffmpeg or mpv.".into());
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "The desktop audio player could not be started.".into())
}

fn seconds_to_millis(seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds * 1_000.0).min(u64::MAX as f64) as u64
}

fn clamp_position(position_ms: u64, duration_ms: Option<u64>) -> u64 {
    duration_ms.map_or(position_ms, |duration_ms| position_ms.min(duration_ms))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAudioSource {
    pub path: PathBuf,
    pub label: String,
}

/// Resolve a session's playback source without accepting a path from the webview.
/// Metadata is preferred, but every source is canonicalized and contained in a
/// configured SessionSmith root before it can be opened by the native player.
pub(crate) fn resolve_source(
    campaign_id: &str,
    stem: &str,
) -> Result<ResolvedAudioSource, String> {
    if !is_simple_identifier(campaign_id) || !is_simple_identifier(stem) {
        return Err("Select a valid campaign session to play audio.".into());
    }
    crate::workspace::session_workspace(campaign_id.to_string(), stem.to_string())?;

    let root = crate::workspace::workspace_root();
    let campaign_path = crate::workspace::campaign_config_path(&root, campaign_id)?;
    let campaign = CampaignConfig::load(&campaign_path).map_err(|error| error.to_string())?;
    let audio_root = resolve_workspace_path(&root, &config::audio_dir());
    let output_root = resolve_workspace_path(&root, &config::output_dir()).join(campaign.slug());
    let transcripts_dir = output_root.join("transcripts");
    let approved_roots: [&Path; 2] = [audio_root.as_path(), output_root.as_path()];

    let metadata_source = meta::load(&transcripts_dir, stem).and_then(|metadata| metadata.source_audio);
    let legacy_source = metadata_source
        .is_none()
        .then(|| audio::find_by_stem(&audio_root, stem))
        .flatten();
    let path = select_audio_source(
        metadata_source.as_deref(),
        legacy_source.as_deref(),
        &root,
        &audio_root,
        &output_root,
        &approved_roots,
    )?;

    Ok(ResolvedAudioSource {
        label: display_label(&path),
        path,
    })
}

fn select_audio_source(
    metadata_source: Option<&Path>,
    legacy_source: Option<&Path>,
    workspace_root: &Path,
    audio_root: &Path,
    output_root: &Path,
    approved_roots: &[&Path],
) -> Result<PathBuf, String> {
    if let Some(source) = metadata_source {
        return metadata_candidates(workspace_root, audio_root, output_root, source)
            .into_iter()
            .find_map(|candidate| approved_audio_path(&candidate, approved_roots))
            .ok_or_else(|| "Session metadata does not reference an approved audio file.".into());
    }
    legacy_source
        .and_then(|source| approved_audio_path(source, approved_roots))
        .ok_or_else(|| "No approved audio source is available for this session.".into())
}

fn metadata_candidates(
    workspace_root: &Path,
    audio_root: &Path,
    output_root: &Path,
    source: &Path,
) -> Vec<PathBuf> {
    if source.is_absolute() {
        return vec![source.to_path_buf()];
    }
    let candidates = [
        workspace_root.join(source),
        audio_root.join(source),
        output_root.join(source),
    ];
    let mut unique = Vec::new();
    for candidate in candidates {
        if !unique.contains(&candidate) {
            unique.push(candidate);
        }
    }
    unique
}

fn approved_audio_path(candidate: &Path, approved_roots: &[&Path]) -> Option<PathBuf> {
    if !candidate.is_file() || !audio::is_supported_audio_path(candidate) {
        return None;
    }
    let candidate = fs::canonicalize(candidate).ok()?;
    approved_roots.iter().any(|root| {
        fs::canonicalize(root)
            .ok()
            .is_some_and(|root| candidate.starts_with(root))
    })
    .then_some(candidate)
}

fn resolve_workspace_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn display_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy())
        .map(|name| {
            name.chars()
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
                .take(180)
                .collect::<String>()
        })
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| "Session audio".into())
}

fn is_simple_identifier(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && !value.chars().any(char::is_control)
        && Path::new(value).file_name().and_then(|name| name.to_str()) == Some(value)
}

#[cfg(test)]
mod tests {
    use super::{
        approved_audio_path, display_label, is_simple_identifier, metadata_candidates,
        select_audio_source,
    };
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temporary_directory(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "sessionsmith-desktop-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn approved_audio_requires_a_supported_file_inside_an_approved_root() {
        let root = temporary_directory("audio-source");
        let audio = root.join("audio");
        let outside = root.join("outside");
        fs::create_dir_all(&audio).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let allowed = audio.join("session.wav");
        let rejected = outside.join("session.wav");
        let text = audio.join("notes.txt");
        fs::write(&allowed, "audio").unwrap();
        fs::write(&rejected, "audio").unwrap();
        fs::write(&text, "not audio").unwrap();

        assert!(approved_audio_path(&allowed, &[&audio]).is_some());
        assert!(approved_audio_path(&rejected, &[&audio]).is_none());
        assert!(approved_audio_path(&text, &[&audio]).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_metadata_outside_the_approved_roots_cannot_fall_back_to_legacy_audio() {
        let root = temporary_directory("metadata-outside");
        let audio = root.join("audio");
        let output = root.join("output/campaign");
        let outside = root.join("outside.wav");
        let legacy = audio.join("session.wav");
        fs::create_dir_all(&audio).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(&outside, "outside").unwrap();
        fs::write(&legacy, "legacy").unwrap();

        let result = select_audio_source(
            Some(&outside),
            Some(&legacy),
            &root,
            &audio,
            &output,
            &[&audio, &output],
        );

        assert_eq!(result.unwrap_err(), "Session metadata does not reference an approved audio file.");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_and_output_root_sources_are_accepted() {
        let root = temporary_directory("approved-sources");
        let audio = root.join("audio");
        let output = root.join("output/campaign");
        let merged = output.join("merged/session.flac");
        let legacy = audio.join("session.wav");
        fs::create_dir_all(merged.parent().unwrap()).unwrap();
        fs::create_dir_all(&audio).unwrap();
        fs::write(&merged, "merged").unwrap();
        fs::write(&legacy, "legacy").unwrap();

        let legacy_result = select_audio_source(
            None,
            Some(&legacy),
            &root,
            &audio,
            &output,
            &[&audio, &output],
        )
        .unwrap();
        let relative_merged = merged.strip_prefix(&root).unwrap();
        let metadata_result = select_audio_source(
            Some(relative_merged),
            None,
            &root,
            &audio,
            &output,
            &[&audio, &output],
        )
        .unwrap();

        assert_eq!(legacy_result, fs::canonicalize(legacy).unwrap());
        assert_eq!(metadata_result, fs::canonicalize(merged).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_unsupported_metadata_source_is_rejected() {
        let root = temporary_directory("unsupported-source");
        let audio = root.join("audio");
        let output = root.join("output/campaign");
        let unsupported = audio.join("session.txt");
        let legacy = audio.join("session.wav");
        fs::create_dir_all(&audio).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(&unsupported, "not audio").unwrap();
        fs::write(&legacy, "legacy").unwrap();

        assert!(select_audio_source(
            Some(&unsupported),
            Some(&legacy),
            &root,
            &audio,
            &output,
            &[&audio, &output],
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected_even_when_its_link_is_inside_the_audio_root() {
        use std::os::unix::fs::symlink;

        let root = temporary_directory("symlink-escape");
        let audio = root.join("audio");
        let output = root.join("output/campaign");
        let outside = root.join("outside.wav");
        let escaped = audio.join("escaped.wav");
        fs::create_dir_all(&audio).unwrap();
        fs::create_dir_all(&output).unwrap();
        fs::write(&outside, "outside").unwrap();
        symlink(&outside, &escaped).unwrap();

        assert!(select_audio_source(
            None,
            Some(&escaped),
            &root,
            &audio,
            &output,
            &[&audio, &output],
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn metadata_candidates_cover_relative_workspace_audio_and_output_paths() {
        let root = Path::new("/workspace");
        let audio = Path::new("/workspace/audio");
        let output = Path::new("/workspace/output/campaign");
        let candidates = metadata_candidates(root, audio, output, Path::new("audio/session.flac"));

        assert_eq!(candidates[0], Path::new("/workspace/audio/session.flac"));
        assert!(candidates.contains(&Path::new("/workspace/output/campaign/audio/session.flac").into()));
    }

    #[test]
    fn source_identity_and_display_label_are_bounded() {
        assert!(is_simple_identifier("session-14"));
        assert!(!is_simple_identifier("../session"));
        assert!(!is_simple_identifier("session\u{0000}"));
        assert_eq!(display_label(Path::new("audio/  session\nname.flac")), "session name.flac");
    }
}