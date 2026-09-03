//! UI-neutral orchestration for pipeline and model-management jobs.

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::{CampaignConfig, GlobalConfig};
use crate::jobs::manager::{JobId, JobManager};
use crate::jobs::procs::ChildRegistry;
use crate::jobs::report::{JobEvent, Reporter};
use crate::pipeline::{self, PipelineOpts, Session};
use crate::presets::Preset;
use crate::prompts::Artifact;
use crate::session::SessionInput;
use crate::transcribe::{self, TranscribeOpts};

/// What a background pipeline job should do.
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

/// A fully-owned unit of work handed to [`spawn_with_reporter`].
pub struct JobRequest {
    pub kind: JobKind,
    pub g: GlobalConfig,
    pub campaign: CampaignConfig,
    pub preset: Preset,
    pub asr_model: String,
    /// Language hint passed to the selected ASR engine (`auto` by default).
    pub language: String,
    /// Optional ISO session date used instead of filename/date inference.
    pub session_date: Option<String>,
    /// Audio sessions to transcribe (Run / Transcribe).
    pub sessions: Vec<SessionInput>,
    /// Transcript `.txt` paths to turn into notes (Notes).
    pub transcripts: Vec<PathBuf>,
    pub artifacts: Vec<Artifact>,
    pub force: bool,
    /// Force re-transcription (separate from `force`, which forces notes). A
    /// re-run leaves this `false` so transcription is reused when the ASR model
    /// is unchanged.
    pub force_transcribe: bool,
    pub resume: bool,
    pub update_log: bool,
    /// Write regenerated artifacts as `.candidate` files for comparison.
    pub candidate: bool,
    pub model_override: Option<String>,
}

/// Spawn a pipeline job through the shared lifecycle manager using a host-owned
/// reporter.
pub fn spawn_with_reporter(
    manager: &JobManager,
    reporter: Arc<dyn Reporter>,
    req: JobRequest,
) -> JobId {
    let kind = manager_kind(&req.kind);
    let title = manager_title(&req.kind);

    manager.submit(kind, title, reporter, move |context| async move {
        let reporter = context.reporter();
        let children = context.children();
        crate::ui::with_reporter(reporter.clone(), async move {
            run(req, Some(children), reporter.as_ref()).await
        })
        .await
    })
}

/// A model install/update/delete operation (does not need a campaign).
pub enum ModelJob {
    PullWhisper(String),
    DeleteWhisper(String),
    PullOllama(String),
    DeleteOllama(String),
    /// Download the uv environment + weights for a bridge ASR model (id).
    PrepareAsr(String),
    /// Delete local files/markers for an advanced ASR model (id).
    DeleteAsr(String),
}

impl ModelJob {
    /// Whether cancelling this operation reliably stops its local work. Ollama
    /// does not document server-side abort semantics for pull or delete calls,
    /// so those operations deliberately remain non-cancellable.
    pub fn supports_cancellation(&self) -> bool {
        !matches!(self, Self::PullOllama(_) | Self::DeleteOllama(_))
    }
}

/// Spawn a model-management job through the shared lifecycle manager using a
/// host-owned reporter. Callers must expose only [`ModelJob`] variants whose
/// cancellation contract they can support.
pub fn spawn_model_with_reporter(
    manager: &JobManager,
    reporter: Arc<dyn Reporter>,
    g: GlobalConfig,
    job: ModelJob,
    title: impl Into<String>,
) -> JobId {
    let can_cancel = job.supports_cancellation();
    manager.submit_with_cancellation(
        crate::jobs::manager::JobKind::Model,
        title,
        can_cancel,
        reporter,
        move |context| async move {
            let reporter = context.reporter();
            let children = context.children();
            crate::ui::with_reporter(reporter.clone(), async move {
                run_model(&g, job, Some(children)).await
            })
            .await
        },
    )
}

async fn run_model(
    g: &GlobalConfig,
    job: ModelJob,
    children: Option<ChildRegistry>,
) -> anyhow::Result<String> {
    use crate::models;
    let base = g
        .backend
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434".into());
    match job {
        ModelJob::PullWhisper(id) => {
            let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
            crate::ui::header(&format!("Checking whisper · {id}"));
            models::download_whisper(&id, &cache).await?;
            Ok(format!("whisper '{id}' ready"))
        }
        ModelJob::DeleteWhisper(id) => {
            let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
            models::delete_whisper(&id, &cache)?;
            Ok(format!("deleted whisper '{id}'"))
        }
        ModelJob::PullOllama(name) => {
            crate::ui::header(&format!("Checking Ollama model · {name}"));
            models::ollama_pull_stream(&name, &base).await?;
            Ok(format!("model '{name}' ready"))
        }
        ModelJob::DeleteOllama(name) => {
            models::ollama_delete(&name, &base).await?;
            Ok(format!("deleted '{name}'"))
        }
        ModelJob::PrepareAsr(id) => {
            let spec =
                crate::asr::find(&id).ok_or_else(|| anyhow::anyhow!("unknown ASR model '{id}'"))?;
            crate::ui::header(&format!(
                "Checking {} · {}",
                spec.engine.label(),
                spec.display
            ));
            if spec.engine == crate::asr::AsrEngine::TranscribeCpp {
                let cache = models::gguf_asr_cache_dir()?;
                models::download_gguf_asr(&id, &cache).await?;
                let device = g.asr.device.clone();
                let children = children.clone();
                crate::ui::spawn_blocking_with_reporter(move || {
                    crate::transcribe_cpp::ensure_runtime_for_with_children(
                        device.as_deref(),
                        children.as_ref(),
                    )
                })
                .await??;
                crate::asr::mark_prepared(&id);
                return Ok(format!("{} ready", spec.display));
            }
            let device = g.asr.device.clone().unwrap_or_else(|| "auto".to_string());
            let engine = spec.engine;
            let model_ref = spec.model_ref.to_string();
            let children = children.clone();
            crate::ui::spawn_blocking_with_reporter(move || {
                crate::pybridge::run_asr_prepare_with_children(
                    engine,
                    &model_ref,
                    &device,
                    children.as_ref(),
                )
            })
            .await??;
            crate::asr::mark_prepared(&id);
            Ok(format!("{} ready", spec.display))
        }
        ModelJob::DeleteAsr(id) => {
            let spec =
                crate::asr::find(&id).ok_or_else(|| anyhow::anyhow!("unknown ASR model '{id}'"))?;
            match spec.engine {
                crate::asr::AsrEngine::TranscribeCpp => {
                    let cache = models::gguf_asr_cache_dir()?;
                    models::delete_gguf_asr(&id, &cache)?;
                }
                engine if engine.is_bridge() => {
                    crate::pybridge::delete_asr_cache(engine, spec.model_ref)?;
                }
                crate::asr::AsrEngine::WhisperCpp => {}
                _ => {}
            }
            crate::asr::clear_prepared(&id);
            Ok(format!("deleted {}", spec.display))
        }
    }
}

async fn run(
    req: JobRequest,
    children: Option<ChildRegistry>,
    reporter: &dyn Reporter,
) -> anyhow::Result<String> {
    match req.kind {
        JobKind::Run => run_pipeline(req, true, children.as_ref()).await,
        JobKind::Transcribe => run_pipeline(req, false, children.as_ref()).await,
        JobKind::Notes => run_notes_only(req).await,
        JobKind::Doctor => run_doctor(req, reporter).await,
        JobKind::RebuildLog => run_rebuild_log(req).await,
    }
}

async fn run_pipeline(
    req: JobRequest,
    with_notes: bool,
    children: Option<&ChildRegistry>,
) -> anyhow::Result<String> {
    if req.sessions.is_empty() {
        anyhow::bail!("no audio selected");
    }
    let tx_opts = TranscribeOpts {
        model: req.asr_model.clone(),
        language: req.language.clone(),
        force: req.force_transcribe,
        replacements: req.campaign.transcription.replacements.clone(),
        source_files: Vec::new(),
        initial_prompt: transcribe::vocabulary_prompt(&req.campaign, &req.preset),
        session_date: req.session_date.clone(),
        diarize: req.g.asr.diarize,
        vad: req.g.asr.vad,
    };
    let merge_dir = crate::config::audio_dir().join("merged");
    let total = req.sessions.len();
    let mut produced = 0usize;
    let mut skipped = 0usize;

    for (i, sess) in req.sessions.iter().enumerate() {
        crate::ui::step(i + 1, total, &sess.name);
        crate::ui::phase(&format!("Transcribe · {}", sess.name));

        let missing: Vec<_> = sess.files.iter().filter(|file| !file.exists()).collect();
        if !missing.is_empty() {
            for file in missing {
                crate::ui::warn(&format!("audio not found: {}", file.display()));
            }
            crate::ui::warn(&format!("skipping session '{}'", sess.name));
            skipped += 1;
            continue;
        }

        let audio_path = if sess.files.len() == 1 {
            sess.files[0].clone()
        } else {
            crate::session::prepare_audio_with_children(sess, &merge_dir, children).await?
        };
        let mut session_opts = tx_opts.clone();
        session_opts.source_files = sess.files.clone();
        let out = transcribe::transcribe_with_children(
            &audio_path,
            &req.campaign.transcripts_dir(),
            &req.g,
            &session_opts,
            children,
        )
        .await?;
        if !req.campaign.transcription.speakers.is_empty() {
            let stem = out
                .txt
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or(&sess.name);
            crate::speakers::apply_to_session(
                &req.campaign.transcripts_dir(),
                stem,
                &req.campaign.transcription.speakers,
            )?;
        }

        if with_notes {
            let session_obj = Session::new(&out.txt, &req.campaign.notes_dir())?;
            let opts = PipelineOpts {
                artifacts: req.artifacts.clone(),
                resume: req.resume,
                force: req.force,
                update_log: req.update_log,
                model_override: req.model_override.clone(),
                candidate: req.candidate,
            };
            pipeline::run_notes(&session_obj, &req.g, &req.campaign, &req.preset, &opts).await?;
            crate::ui::ok(&format!("artifacts in {}", session_obj.notes_dir.display()));
        }
        produced += 1;
    }

    let skipped_suffix = if skipped > 0 {
        format!("; skipped {skipped} with missing audio")
    } else {
        String::new()
    };
    Ok(if with_notes {
        format!("processed {produced} session(s){skipped_suffix}")
    } else {
        format!("transcribed {produced} session(s){skipped_suffix}")
    })
}

async fn run_notes_only(req: JobRequest) -> anyhow::Result<String> {
    if req.transcripts.is_empty() {
        anyhow::bail!("no transcript selected");
    }
    let total = req.transcripts.len();
    for (i, transcript) in req.transcripts.iter().enumerate() {
        crate::ui::step(
            i + 1,
            total,
            &transcript
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default(),
        );
        if !transcript.exists() {
            crate::ui::warn(&format!("transcript not found: {}", transcript.display()));
            continue;
        }
        let session_obj = Session::new(transcript, &req.campaign.notes_dir())?;
        let opts = PipelineOpts {
            artifacts: req.artifacts.clone(),
            resume: req.resume,
            force: req.force,
            update_log: req.update_log,
            model_override: req.model_override.clone(),
            candidate: req.candidate,
        };
        pipeline::run_notes(&session_obj, &req.g, &req.campaign, &req.preset, &opts).await?;
        crate::ui::ok(&format!("artifacts in {}", session_obj.notes_dir.display()));
    }
    Ok(format!("generated notes for {total} transcript(s)"))
}

async fn run_doctor(req: JobRequest, reporter: &dyn Reporter) -> anyhow::Result<String> {
    use crate::{deps, hardware, models};
    let g = &req.g;
    let hw = hardware::detect();
    let rec = hardware::recommend(&hw);

    reporter.event(JobEvent::Header("System check".into()));
    reporter.event(JobEvent::Info(format!(
        "OS: {} · CPU cores: {} · RAM: {} GB",
        hw.os, hw.cpu_cores, hw.ram_gb
    )));
    match &hw.gpu {
        Some(gpu) => reporter.event(JobEvent::Info(format!(
            "GPU: {} {} ({} GB VRAM)",
            gpu.vendor, gpu.name, gpu.vram_gb
        ))),
        None => reporter.event(JobEvent::Info("GPU: none detected".into())),
    }
    reporter.event(JobEvent::Info(format!(
        "Recommended whisper: {} · LLM: {}",
        rec.whisper_model, rec.llm_model
    )));

    let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
    let asr_model = g
        .asr
        .model
        .clone()
        .unwrap_or_else(|| rec.whisper_model.to_string());
    let mut checks = vec![
        deps::check_ffmpeg(),
        deps::check_ffprobe(),
        deps::check_whisper_cli(g.asr.binary.as_deref()),
        deps::check_whisper_model(&asr_model, &cache),
    ];
    checks.push(deps::check_backend(g).await);

    let mut fails = 0;
    for check in &checks {
        if check.ok {
            reporter.event(JobEvent::Ok(format!("{}: {}", check.name, check.detail)));
        } else {
            fails += 1;
            reporter.event(JobEvent::Warn(format!("{}: {}", check.name, check.detail)));
        }
    }

    Ok(if fails == 0 {
        "all systems operational".into()
    } else {
        format!("{fails} dependency issue(s) — see above")
    })
}

async fn run_rebuild_log(req: JobRequest) -> anyhow::Result<String> {
    crate::campaign_log::rebuild_for(&req.campaign, &req.g, &req.preset).await?;
    Ok("campaign log rebuilt".into())
}

fn manager_kind(kind: &JobKind) -> crate::jobs::manager::JobKind {
    match kind {
        JobKind::Run => crate::jobs::manager::JobKind::Run,
        JobKind::Transcribe => crate::jobs::manager::JobKind::Transcribe,
        JobKind::Notes => crate::jobs::manager::JobKind::Notes,
        JobKind::Doctor => crate::jobs::manager::JobKind::Doctor,
        JobKind::RebuildLog => crate::jobs::manager::JobKind::RebuildLog,
    }
}

fn manager_title(kind: &JobKind) -> &'static str {
    match kind {
        JobKind::Run => "Run pipeline",
        JobKind::Transcribe => "Transcribe",
        JobKind::Notes => "Generate notes",
        JobKind::Doctor => "System check",
        JobKind::RebuildLog => "Rebuild campaign log",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires an explicitly isolated XDG cache"]
    #[cfg(unix)]
    async fn prepares_deletes_and_reprepares_disposable_moss_cache() {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            std::env::var("SESSIONSMITH_ACCEPTANCE_MODEL_DELETE").as_deref(),
            Ok("1"),
            "set SESSIONSMITH_ACCEPTANCE_MODEL_DELETE=1 only with an isolated XDG_CACHE_HOME"
        );
        let cache_root = dirs::cache_dir().expect("isolated cache root should resolve");
        assert!(
            cache_root.starts_with(std::env::temp_dir()),
            "acceptance model deletion must use a temporary cache root"
        );
        let model_dir = cache_root
            .join("huggingface/hub")
            .join("models--OpenMOSS-Team--MOSS-Transcribe-Diarize");
        let fake_bin = cache_root.join("acceptance-bin");
        std::fs::create_dir_all(&fake_bin).expect("fake executable directory should be created");
        let fake_uv = fake_bin.join("uv");
        std::fs::write(
            &fake_uv,
            "#!/bin/sh\nset -eu\nmodel_dir=\"$XDG_CACHE_HOME/huggingface/hub/models--OpenMOSS-Team--MOSS-Transcribe-Diarize\"\nmkdir -p \"$model_dir/snapshots/disposable\"\nprintf '{}' > \"$model_dir/snapshots/disposable/config.json\"\nprintf '@@P 1 1 ready\\n'\n",
        )
        .expect("fake uv should be written");
        let mut permissions = std::fs::metadata(&fake_uv)
            .expect("fake uv metadata should exist")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&fake_uv, permissions)
            .expect("fake uv should be executable");
        let original_path = std::env::var_os("PATH").unwrap_or_default();
        let mut path_entries = vec![fake_bin];
        path_entries.extend(std::env::split_paths(&original_path));
        std::env::set_var(
            "PATH",
            std::env::join_paths(path_entries).expect("fake uv path should be valid"),
        );

        let model_id = "moss-transcribe-diarize-0.9b";
        crate::asr::clear_prepared(model_id);
        let prepared = run_model(
            &GlobalConfig::default(),
            ModelJob::PrepareAsr(model_id.into()),
            None,
        )
        .await
        .expect("disposable MOSS preparation should succeed");
        assert_eq!(prepared, "MOSS Transcribe-Diarize 0.9B ready");
        assert!(model_dir.is_dir());
        assert!(crate::asr::is_prepared("moss-transcribe-diarize-0.9b"));

        let summary = run_model(
            &GlobalConfig::default(),
            ModelJob::DeleteAsr(model_id.into()),
            None,
        )
        .await
        .expect("disposable MOSS deletion should succeed");

        assert_eq!(summary, "deleted MOSS Transcribe-Diarize 0.9B");
        assert!(!model_dir.exists());
        assert!(!crate::asr::is_prepared(model_id));

        run_model(
            &GlobalConfig::default(),
            ModelJob::PrepareAsr(model_id.into()),
            None,
        )
        .await
        .expect("disposable MOSS re-preparation should succeed");
        assert!(model_dir.is_dir());
        assert!(crate::asr::is_prepared(model_id));
        std::env::set_var("PATH", original_path);
    }

    #[test]
    fn ollama_network_operations_remain_non_cancellable() {
        assert!(ModelJob::PullWhisper("base".into()).supports_cancellation());
        assert!(ModelJob::PrepareAsr("parakeet-tdt-0.6b-v3".into()).supports_cancellation());
        assert!(!ModelJob::PullOllama("qwen3.5:4b".into()).supports_cancellation());
        assert!(!ModelJob::DeleteOllama("qwen3.5:4b".into()).supports_cancellation());
    }
}
