use anyhow::Result;
use std::path::Path;

use crate::audio;
use crate::cli::RunArgs;
use crate::config::GlobalConfig;
use crate::pipeline::{self, PipelineOpts, Session};
use crate::presets;
use crate::session::SessionInput;
use crate::transcribe::{self, TranscribeOpts};
use crate::{commands, deps, hardware, ui};

pub async fn run(args: RunArgs) -> Result<()> {
    ui::header("SessionSmith");
    deps::ensure_dirs()?;

    let camp_path = commands::resolve_campaign(None)?;
    let campaign  = commands::load_campaign_or_die(&camp_path)?;
    let preset    = presets::load(&campaign.system.preset)?;

    let mut g = GlobalConfig::load_or_default()?;
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
    let sessions: Vec<SessionInput> = if !args.files.is_empty() {
        args.files.iter().map(|f| SessionInput {
            files: vec![f.clone()],
            name: f.file_stem().map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "session".to_string()),
        }).collect()
    } else if args.all {
        let mut scanned = audio::scan(Path::new("audio"), &campaign.transcripts_dir())?;
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
        language: "auto".into(),
        force: args.force,
    };

    let artifacts = commands::notes::resolve_artifacts(&args.artifacts, &campaign.outputs.default)?;
    let tmp_dir   = std::env::temp_dir().join("sessionsmith_concat");

    for (i, sess) in sessions.iter().enumerate() {
        ui::step(i + 1, sessions.len(), &sess.name);

        // Validate files exist.
        for f in &sess.files {
            if !f.exists() {
                ui::warn(&format!("audio file not found: {}", f.display()));
                continue;
            }
        }

        // Concat if needed, then transcribe.
        let audio_path = commands::transcribe::prepare_audio(sess, &tmp_dir).await?;
        let out = transcribe::transcribe(&audio_path, &campaign.transcripts_dir(), &g, &tx_opts).await?;

        // Clean up temp merged file.
        if sess.files.len() > 1 { std::fs::remove_file(&audio_path).ok(); }

        let session_obj = Session::new(&out.txt, &campaign.notes_dir())?;

        let opts = PipelineOpts {
            artifacts: artifacts.clone(),
            resume: args.resume,
            force: args.force,
            update_log: !args.no_log,
            model_override: args.model.clone(),
        };
        pipeline::run_notes(&session_obj, &g, &campaign, &preset, &opts).await?;
        ui::ok(&format!("artifacts in {}", session_obj.notes_dir.display()));
    }
    Ok(())
}
