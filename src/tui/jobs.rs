//! Background job runner. Long operations (transcription, the LLM pipeline,
//! the campaign-log rebuild, doctor) run in a `tokio` task with the UI event
//! sink installed, so their progress streams into the TUI live pane instead of
//! blocking the event loop or corrupting the alternate screen.

use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

use crate::config::{CampaignConfig, GlobalConfig};
use crate::pipeline::{self, PipelineOpts, Session};
use crate::presets::Preset;
use crate::prompts::Artifact;
use crate::session::SessionInput;
use crate::transcribe::{self, TranscribeOpts};
use crate::ui::UiEvent;

/// What a background job should do.
pub enum JobKind {
    /// Full pipeline: transcribe each session then generate notes.
    Run,
    /// Transcription only.
    Transcribe,
    /// Notes only, over already-existing transcripts.
    Notes,
    /// Re-check dependencies / hardware / backend.
    Doctor,
    /// Rebuild the rolling campaign log from scratch.
    RebuildLog,
}

/// A fully-owned unit of work handed to [`spawn`].
pub struct JobRequest {
    pub kind: JobKind,
    pub g: GlobalConfig,
    pub campaign: CampaignConfig,
    pub preset: Preset,
    pub asr_model: String,
    /// Audio sessions to transcribe (Run / Transcribe).
    pub sessions: Vec<SessionInput>,
    /// Transcript `.txt` paths to turn into notes (Notes).
    pub transcripts: Vec<PathBuf>,
    pub artifacts: Vec<Artifact>,
    pub force: bool,
    pub resume: bool,
    pub update_log: bool,
    pub model_override: Option<String>,
}

/// Spawn `req` on the current tokio runtime, streaming progress to `tx` and
/// finishing with a [`UiEvent::JobDone`].
pub fn spawn(handle: &tokio::runtime::Handle, tx: UnboundedSender<UiEvent>, req: JobRequest) {
    let tx2 = tx.clone();
    handle.spawn(async move {
        crate::ui::set_event_sink(Some(tx.clone()));
        let res = run(req).await;
        crate::ui::set_event_sink(None);
        let _ = tx2.send(UiEvent::JobDone(
            res.map(|summary| summary).map_err(|e| format!("{e:#}")),
        ));
    });
}

/// A model install/update/delete operation (does not need a campaign).
pub enum ModelJob {
    PullWhisper(String),
    DeleteWhisper(String),
    PullOllama(String),
    DeleteOllama(String),
}

/// Spawn a model-management job.
pub fn spawn_model(
    handle: &tokio::runtime::Handle,
    tx: UnboundedSender<UiEvent>,
    g: GlobalConfig,
    job: ModelJob,
) {
    let tx2 = tx.clone();
    handle.spawn(async move {
        crate::ui::set_event_sink(Some(tx.clone()));
        let res = run_model(&g, job).await;
        crate::ui::set_event_sink(None);
        let _ = tx2.send(UiEvent::JobDone(res.map_err(|e| format!("{e:#}"))));
    });
}

async fn run_model(g: &GlobalConfig, job: ModelJob) -> anyhow::Result<String> {
    use crate::models;
    let base = g
        .backend
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434".into());
    match job {
        ModelJob::PullWhisper(id) => {
            let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
            crate::ui::header(&format!("Downloading whisper · {id}"));
            models::download_whisper(&id, &cache).await?;
            Ok(format!("whisper '{id}' ready"))
        }
        ModelJob::DeleteWhisper(id) => {
            let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
            models::delete_whisper(&id, &cache)?;
            Ok(format!("deleted whisper '{id}'"))
        }
        ModelJob::PullOllama(name) => {
            crate::ui::header(&format!("Pulling Ollama model · {name}"));
            models::ollama_pull_stream(&name, &base).await?;
            Ok(format!("model '{name}' ready"))
        }
        ModelJob::DeleteOllama(name) => {
            models::ollama_delete(&name, &base).await?;
            Ok(format!("deleted '{name}'"))
        }
    }
}

async fn run(req: JobRequest) -> anyhow::Result<String> {
    match req.kind {
        JobKind::Run => run_pipeline(req, true).await,
        JobKind::Transcribe => run_pipeline(req, false).await,
        JobKind::Notes => run_notes_only(req).await,
        JobKind::Doctor => run_doctor(req).await,
        JobKind::RebuildLog => run_rebuild_log(req).await,
    }
}

async fn run_pipeline(req: JobRequest, with_notes: bool) -> anyhow::Result<String> {
    if req.sessions.is_empty() {
        anyhow::bail!("no audio selected");
    }
    let tx_opts = TranscribeOpts {
        model: req.asr_model.clone(),
        language: "auto".into(),
        force: req.force,
        diarize: req.g.asr.diarize,
        vad: req.g.asr.vad,
    };
    let tmp_dir = std::env::temp_dir().join("sessionsmith_concat");
    let total = req.sessions.len();
    let mut produced = 0usize;

    for (i, sess) in req.sessions.iter().enumerate() {
        crate::ui::step(i + 1, total, &sess.name);

        // Single-file passthrough; multi-file sessions are concatenated.
        let audio_path = if sess.files.len() == 1 {
            sess.files[0].clone()
        } else {
            crate::commands::transcribe::prepare_audio(sess, &tmp_dir).await?
        };
        if !audio_path.exists() {
            crate::ui::warn(&format!("audio not found: {}", audio_path.display()));
            continue;
        }

        let out = transcribe::transcribe(
            &audio_path,
            &req.campaign.transcripts_dir(),
            &req.g,
            &tx_opts,
        )
        .await?;
        if sess.files.len() > 1 {
            std::fs::remove_file(&audio_path).ok();
        }

        if with_notes {
            let session_obj = Session::new(&out.txt, &req.campaign.notes_dir())?;
            let opts = PipelineOpts {
                artifacts: req.artifacts.clone(),
                resume: req.resume,
                force: req.force,
                update_log: req.update_log,
                model_override: req.model_override.clone(),
            };
            pipeline::run_notes(&session_obj, &req.g, &req.campaign, &req.preset, &opts).await?;
            crate::ui::ok(&format!("artifacts in {}", session_obj.notes_dir.display()));
        }
        produced += 1;
    }

    Ok(if with_notes {
        format!("processed {produced} session(s)")
    } else {
        format!("transcribed {produced} session(s)")
    })
}

async fn run_notes_only(req: JobRequest) -> anyhow::Result<String> {
    if req.transcripts.is_empty() {
        anyhow::bail!("no transcript selected");
    }
    let total = req.transcripts.len();
    for (i, t) in req.transcripts.iter().enumerate() {
        crate::ui::step(i + 1, total, &t.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
        if !t.exists() {
            crate::ui::warn(&format!("transcript not found: {}", t.display()));
            continue;
        }
        let session_obj = Session::new(t, &req.campaign.notes_dir())?;
        let opts = PipelineOpts {
            artifacts: req.artifacts.clone(),
            resume: req.resume,
            force: req.force,
            update_log: req.update_log,
            model_override: req.model_override.clone(),
        };
        pipeline::run_notes(&session_obj, &req.g, &req.campaign, &req.preset, &opts).await?;
        crate::ui::ok(&format!("artifacts in {}", session_obj.notes_dir.display()));
    }
    Ok(format!("generated notes for {total} transcript(s)"))
}

async fn run_doctor(req: JobRequest) -> anyhow::Result<String> {
    use crate::{deps, hardware, models};
    let g = &req.g;
    let hw = hardware::detect();
    let rec = hardware::recommend(&hw);

    crate::ui::header("System check");
    crate::ui::info(&format!(
        "OS: {} · CPU cores: {} · RAM: {} GB",
        hw.os, hw.cpu_cores, hw.ram_gb
    ));
    match &hw.gpu {
        Some(gpu) => crate::ui::info(&format!(
            "GPU: {} {} ({} GB VRAM)",
            gpu.vendor, gpu.name, gpu.vram_gb
        )),
        None => crate::ui::info("GPU: none detected"),
    }
    crate::ui::info(&format!(
        "Recommended whisper: {} · LLM: {}",
        rec.whisper_model, rec.llm_model
    ));

    let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
    let asr_model = g.asr.model.clone().unwrap_or_else(|| rec.whisper_model.to_string());
    let mut checks = vec![
        deps::check_ffmpeg(),
        deps::check_ffprobe(),
        deps::check_whisper_cli(g.asr.binary.as_deref()),
        deps::check_whisper_model(&asr_model, &cache),
    ];
    checks.push(deps::check_backend(g).await);

    let mut fails = 0;
    for c in &checks {
        if c.ok {
            crate::ui::ok(&format!("{}: {}", c.name, c.detail));
        } else {
            fails += 1;
            crate::ui::warn(&format!("{}: {}", c.name, c.detail));
        }
    }

    Ok(if fails == 0 {
        "all systems operational".into()
    } else {
        format!("{fails} dependency issue(s) — see above")
    })
}

async fn run_rebuild_log(_req: JobRequest) -> anyhow::Result<String> {
    crate::commands::log_cmd::run(crate::cli::LogArgs {
        action: Some(crate::cli::LogAction::Rebuild),
    })
    .await?;
    Ok("campaign log rebuilt".into())
}
