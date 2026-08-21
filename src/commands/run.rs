use anyhow::{bail, Result};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::SystemTime;

use crate::audio;
use crate::cli::RunArgs;
use crate::config::GlobalConfig;
use crate::pipeline::{self, PipelineOpts, Session};
use crate::presets;
use crate::session::SessionInput;
use crate::transcribe::{self, TranscribeOpts};
use crate::{commands, deps, hardware, ui};

static WATCH_ACTIVE: AtomicBool = AtomicBool::new(false);
static WATCH_INTERRUPTS: AtomicU8 = AtomicU8::new(0);

/// Returns true when Ctrl-C should request a graceful watch shutdown. A second
/// Ctrl-C falls through to the normal immediate process exit handler.
pub fn request_watch_stop() -> bool {
    WATCH_ACTIVE.load(Ordering::SeqCst)
        && WATCH_INTERRUPTS.fetch_add(1, Ordering::SeqCst) == 0
}

pub async fn run(args: RunArgs) -> Result<()> {
    if args.watch {
        return watch(args).await;
    }
    run_once(args).await
}

async fn run_once(args: RunArgs) -> Result<()> {
    ui::header("SessionSmith");
    deps::ensure_dirs()?;

    let camp_path = commands::resolve_campaign(None)?;
    let campaign  = commands::load_campaign_or_die(&camp_path)?;
    let preset    = presets::load(&campaign.system.preset)?;
    let mut speaker_map = campaign.transcription.speakers.clone();
    for value in &args.speakers {
        let (label, name) = crate::speakers::parse_mapping(value)?;
        speaker_map.insert(label, name);
    }

    let mut g = crate::config::effective(&GlobalConfig::load_or_default()?, &campaign);
    if let Some(b) = args.backend   { g.backend.kind  = b; }
    if let Some(m) = &args.model    { g.backend.model  = Some(m.clone()); }

    let asr_model = args.asr_model.clone()
        .or_else(|| g.asr.model.clone())
        .unwrap_or_else(|| hardware::recommend(&hardware::detect()).whisper_model.to_string());

    ui::panel("Session", &[
        format!("Campaign : {}", campaign.campaign.name),
        format!("System   : {}", preset.name),
        format!("Backend  : {} → model {}",
                g.backend.kind,
                g.backend.model.clone().unwrap_or_else(|| "(not set)".into())),
        format!("ASR      : {}", asr_model),
    ]);

    // Build the session list from CLI args / picker.
    let sessions: Vec<SessionInput> = if args.combine {
        if args.files.is_empty() {
            anyhow::bail!("--combine needs at least one audio file");
        }
        vec![SessionInput {
            files: args.files.clone(),
            name: args.name.clone().expect("clap requires --name with --combine"),
        }]
    } else if !args.files.is_empty() {
        args.files.iter().map(|f| SessionInput {
            files: vec![f.clone()],
            name: f.file_stem().map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "session".to_string()),
        }).collect()
    } else if args.all {
        let mut scanned = audio::scan(&crate::config::audio_dir(), &campaign.transcripts_dir())?;
        audio::enrich_durations(&mut scanned);
        if scanned.is_empty() {
            anyhow::bail!("no audio files in `audio/`");
        }
        scanned.into_iter().map(|f| SessionInput {
            name: f.stem(),
            files: vec![f.path],
        }).collect()
    } else {
        commands::transcribe::pick_and_build_sessions(&g, &campaign.transcripts_dir()).await?
    };

    let tx_opts = TranscribeOpts {
        model: asr_model,
        language: args.language.clone(),
        force: args.force,
        replacements: campaign.transcription.replacements.clone(),
        source_files: Vec::new(),
        initial_prompt: transcribe::vocabulary_prompt(&campaign, &preset),
        session_date: args.date.clone(),
        diarize: args.diarize || g.asr.diarize,
        vad: args.vad || g.asr.vad,
    };

    let artifacts = commands::notes::resolve_artifacts(&args.artifacts, &campaign.outputs.default)?;
    let merge_dir = crate::config::audio_dir().join("merged");
    let mut skipped = 0usize;

    for (i, sess) in sessions.iter().enumerate() {
        ui::step(i + 1, sessions.len(), &sess.name);

        // Validate files exist.
        let missing: Vec<_> = sess.files.iter().filter(|file| !file.exists()).collect();
        if !missing.is_empty() {
            for file in missing {
                ui::warn(&format!("audio file not found: {}", file.display()));
            }
            ui::warn(&format!("skipping session '{}'", sess.name));
            skipped += 1;
            continue;
        }

        // Concat if needed, then transcribe.
        let audio_path = commands::transcribe::prepare_audio(sess, &merge_dir).await?;
        let mut session_opts = tx_opts.clone();
        session_opts.source_files = sess.files.clone();
        let out = transcribe::transcribe(&audio_path, &campaign.transcripts_dir(), &g, &session_opts).await?;
        if !speaker_map.is_empty() {
            let stem = out.txt.file_stem().and_then(|value| value.to_str()).unwrap_or(&sess.name);
            crate::speakers::apply_to_session(&campaign.transcripts_dir(), stem, &speaker_map)?;
        }

        let session_obj = Session::new(&out.txt, &campaign.notes_dir())?;

        let opts = PipelineOpts {
            artifacts: artifacts.clone(),
            resume: args.resume,
            force: args.force || args.candidate,
            update_log: !args.no_log && !args.candidate,
            model_override: args.model.clone(),
            candidate: args.candidate,
        };
        pipeline::run_notes(&session_obj, &g, &campaign, &preset, &opts).await?;
        ui::ok(&format!("artifacts in {}", session_obj.notes_dir.display()));
    }
    if skipped > 0 {
        ui::warn(&format!("skipped {skipped} session(s) with missing audio"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WatchFingerprint {
    size_bytes: u64,
    mtime: SystemTime,
}

fn stable_new_files(
    files: &[audio::AudioFile],
    previous: &mut BTreeMap<PathBuf, WatchFingerprint>,
    handled: &HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let mut current = BTreeMap::new();
    let mut ready = Vec::new();
    for file in files {
        let fingerprint = WatchFingerprint { size_bytes: file.size_bytes, mtime: file.mtime };
        if !file.already_transcribed
            && !handled.contains(&file.path)
            && previous.get(&file.path) == Some(&fingerprint)
        {
            ready.push(file.path.clone());
        }
        current.insert(file.path.clone(), fingerprint);
    }
    *previous = current;
    ready
}

async fn watch(mut args: RunArgs) -> Result<()> {
    if !args.files.is_empty() {
        bail!("--watch scans the configured audio directory; do not pass audio files");
    }
    let camp_path = commands::resolve_campaign(None)?;
    std::env::set_var("SESSIONSMITH_CAMPAIGN", &camp_path);
    let campaign = commands::load_campaign_or_die(&camp_path)?;
    let interval = std::time::Duration::from_secs(args.watch_interval);
    let _watch_guard = WatchGuard::new();
    let mut previous = BTreeMap::new();
    let mut handled = HashSet::new();
    args.watch = false;
    args.all = false;

    ui::info(&format!("watching {} every {}s", crate::config::audio_dir().display(), args.watch_interval));
    loop {
        if WATCH_INTERRUPTS.load(Ordering::SeqCst) > 0 {
            ui::info("watch stopped after the current recording");
            return Ok(());
        }
        let files = audio::scan(&crate::config::audio_dir(), &campaign.transcripts_dir())?;
        for path in stable_new_files(&files, &mut previous, &handled) {
            ui::info(&format!("stable recording detected: {}", path.display()));
            let mut single_run = args.clone();
            single_run.files = vec![path.clone()];
            if let Err(err) = run_once(single_run).await {
                ui::warn(&format!("watch failed for {}: {err:#}", path.display()));
            }
            handled.insert(path);
            if WATCH_INTERRUPTS.load(Ordering::SeqCst) > 0 {
                ui::info("watch stopped after the current recording");
                return Ok(());
            }
        }
        tokio::time::sleep(interval).await;
    }
}

struct WatchGuard;

impl WatchGuard {
    fn new() -> Self {
        WATCH_INTERRUPTS.store(0, Ordering::SeqCst);
        WATCH_ACTIVE.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        WATCH_ACTIVE.store(false, Ordering::SeqCst);
        WATCH_INTERRUPTS.store(0, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_requires_two_identical_scans_before_processing() {
        let path = PathBuf::from("session.wav");
        let file = audio::AudioFile {
            path: path.clone(),
            mtime: SystemTime::UNIX_EPOCH,
            duration_secs: None,
            size_bytes: 10,
            already_transcribed: false,
        };
        let mut previous = BTreeMap::new();
        let handled = HashSet::new();
        assert!(stable_new_files(&[file.clone()], &mut previous, &handled).is_empty());
        assert_eq!(stable_new_files(&[file], &mut previous, &handled), vec![path]);
    }
}
