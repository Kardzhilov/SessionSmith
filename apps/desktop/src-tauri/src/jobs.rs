use serde::{Deserialize, Serialize};
use sessionsmith::{
    audio,
    commands::{
        export::{ExportFormat as CoreExportFormat, ExportRequest as CoreExportRequest},
        record::RecordingRequest,
    },
    config::{self, CampaignConfig, GlobalConfig},
    hardware,
    jobs::{
        manager::{JobId, JobKind, JobManager, JobSnapshot as ManagedJobSnapshot, JobState},
        report::{JobEvent, Reporter},
    },
    pipeline,
    prompts::Artifact,
    session::SessionInput,
    tui::{spawn_model_with_reporter, spawn_with_reporter, JobRequest, ModelJob, PipelineJobKind},
    ui,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};

pub const JOB_UPDATED_EVENT: &str = "job://updated";

const LOG_TAIL_LIMIT: usize = 80;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub label: String,
    pub position: u64,
    pub total: u64,
    pub rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub id: JobId,
    pub kind: String,
    pub title: String,
    pub state: String,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub summary: Option<String>,
    pub active_children: usize,
    pub phase: Option<String>,
    pub progress: Option<JobProgress>,
    pub log_tail: Vec<String>,
    pub can_cancel: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSubmission {
    pub id: JobId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAudioRequest {
    pub campaign_id: String,
    pub source_paths: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Html,
    Obsidian,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub campaign_id: String,
    #[serde(default)]
    pub stems: Vec<String>,
    #[serde(default)]
    pub all: bool,
    pub format: ExportFormat,
    #[serde(default)]
    pub player_safe: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRequest {
    pub campaign_id: String,
    pub source_paths: Vec<String>,
    #[serde(default)]
    pub all_inbox: bool,
    pub artifact_ids: Vec<String>,
    pub resume: bool,
    pub force: bool,
    pub candidate: bool,
    pub asr_model: Option<String>,
    pub language: Option<String>,
    pub session_date: Option<String>,
    #[serde(default)]
    pub diarize: bool,
    #[serde(default)]
    pub vad: bool,
    pub backend_kind: Option<String>,
    pub llm_model: Option<String>,
    #[serde(default)]
    pub combine: bool,
    pub session_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesRequest {
    pub campaign_id: String,
    pub stem: String,
    pub artifact_ids: Vec<String>,
    pub resume: bool,
    pub force: bool,
    pub candidate: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeRequest {
    pub campaign_id: String,
    pub source_paths: Vec<String>,
    #[serde(default)]
    pub all_inbox: bool,
    pub force: bool,
    pub asr_model: Option<String>,
    pub language: Option<String>,
    pub session_date: Option<String>,
    #[serde(default)]
    pub diarize: bool,
    #[serde(default)]
    pub vad: bool,
    #[serde(default)]
    pub combine: bool,
    pub session_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelAction {
    DownloadWhisper,
    DeleteWhisper,
    PrepareAsr,
    DeleteAsr,
    PullOllama,
    DeleteOllama,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequest {
    pub action: ModelAction,
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerMapRequest {
    pub campaign_id: String,
    pub stem: String,
    pub mappings: Vec<SpeakerMapEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerMapEntry {
    pub label: String,
    pub name: String,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum CandidateAction {
    KeepCandidate,
    KeepBoth,
    DiscardCandidate,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateResolveRequest {
    pub campaign_id: String,
    pub stem: String,
    pub artifact_id: String,
    pub action: CandidateAction,
}

#[derive(Clone, Default)]
pub struct DesktopJobs {
    inner: Arc<DesktopJobsInner>,
}

#[derive(Default)]
struct DesktopJobsInner {
    manager: OnceLock<JobManager>,
    details: Mutex<BTreeMap<JobId, JobDetails>>,
    artifact_mutation_active: Mutex<bool>,
}

pub(crate) struct ArtifactMutationLease {
    inner: Arc<DesktopJobsInner>,
}

impl Drop for ArtifactMutationLease {
    fn drop(&mut self) {
        *self
            .inner
            .artifact_mutation_active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
    }
}

#[derive(Clone, Default)]
struct JobDetails {
    phase: Option<String>,
    progress: Option<JobProgress>,
    log_tail: VecDeque<String>,
}

struct ProcessingContext {
    global: GlobalConfig,
    campaign: CampaignConfig,
    preset: sessionsmith::presets::Preset,
    asr_model: String,
    inbox_paths: Vec<PathBuf>,
    existing_stems: BTreeSet<String>,
    transcripts_dir: PathBuf,
}

impl DesktopJobs {
    pub fn submit_doctor(&self, app: AppHandle) -> Result<JobSubmission, String> {
        let manager = self.manager()?;
        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = manager.submit(JobKind::Doctor, "System check", reporter.clone(), move |context| async move {
            let reporter = context.reporter();
            ui::with_reporter(reporter, async move {
                ui::header("System check");
                ui::phase("Readiness");

                let report = crate::health::report()
                    .await
                    .map_err(std::io::Error::other)?;
                let mut failures = 0usize;
                let mut warnings = 0usize;

                for check in report.checks {
                    let message = format!("{}: {}", check.label, check.detail);
                    match check.state.as_str() {
                        "ok" => ui::ok(&message),
                        "warn" => {
                            warnings += 1;
                            ui::warn(&message);
                        }
                        _ => {
                            failures += 1;
                            ui::error(&message);
                        }
                    }
                }

                Ok(if failures == 0 && warnings == 0 {
                    "all checks are ready".into()
                } else if failures == 0 {
                    format!("{warnings} optional check(s) unavailable")
                } else {
                    format!("{failures} check(s) need attention")
                })
            })
            .await
        });

        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_record(
        &self,
        app: AppHandle,
        request: RecordRequest,
    ) -> Result<JobSubmission, String> {
        let request = recording_request(request)?;
        let manager = self.manager()?;
        if manager.list().iter().any(|job| {
            job.kind == JobKind::Record
                && !matches!(
                    job.state,
                    JobState::Succeeded | JobState::Failed | JobState::Cancelled
                )
        }) {
            return Err("A recording is already active.".into());
        }

        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = sessionsmith::jobs::record::spawn(&manager, reporter.clone(), request);
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_import(
        &self,
        app: AppHandle,
        request: ImportAudioRequest,
    ) -> Result<JobSubmission, String> {
        let request = import_request(request)?;
        let manager = self.manager()?;
        if manager.list().iter().any(|job| {
            job.kind == JobKind::Import
                && !matches!(
                    job.state,
                    JobState::Succeeded | JobState::Failed | JobState::Cancelled
                )
        }) {
            return Err("An audio import is already active.".into());
        }
        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = sessionsmith::jobs::import_audio::spawn(&manager, reporter.clone(), request);
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_process(
        &self,
        app: AppHandle,
        request: ProcessRequest,
    ) -> Result<JobSubmission, String> {
        let request = process_request(request)?;
        let manager = self.manager()?;
        if self.artifact_mutation_active() {
            return Err("Wait for the active document update before starting a pipeline job.".into());
        }
        if pipeline_job_active(&manager) {
            return Err("A pipeline job is already active.".into());
        }

        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = spawn_with_reporter(&manager, reporter.clone(), request);
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_log_rebuild(
        &self,
        app: AppHandle,
        campaign_id: String,
    ) -> Result<JobSubmission, String> {
        let request = log_rebuild_request(&campaign_id)?;
        let manager = self.manager()?;
        if self.artifact_mutation_active() {
            return Err("Wait for the active local update before rebuilding the campaign log.".into());
        }
        if pipeline_job_active(&manager) {
            return Err("A pipeline job is already active.".into());
        }

        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = spawn_with_reporter(&manager, reporter.clone(), request);
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_reindex(
        &self,
        app: AppHandle,
        campaign_id: String,
    ) -> Result<JobSubmission, String> {
        let context = processing_context(campaign_id.trim())?;
        let manager = self.manager()?;
        if manager.list().iter().any(|job| {
            job.kind == JobKind::Reindex
                && !matches!(job.state, JobState::Succeeded | JobState::Failed | JobState::Cancelled)
        }) {
            return Err("A search index rebuild is already active.".into());
        }

        let title = format!("Rebuild search index · {}", context.campaign.campaign.name);
        let campaign = context.campaign;
        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = manager.submit(JobKind::Reindex, title, reporter.clone(), move |context| async move {
            let reporter = context.reporter();
            ui::with_reporter(reporter, async move {
                ui::phase("Index session notes");
                let indexed = tauri::async_runtime::spawn_blocking(move || {
                    sessionsmith::campaign_ops::reindex_campaign(&campaign)
                        .map_err(std::io::Error::other)
                })
                .await
                .map_err(std::io::Error::other)??;
                let summary = format!("indexed {indexed} session(s)");
                ui::ok(&summary);
                Ok(summary)
            })
            .await
        });
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_export(
        &self,
        app: AppHandle,
        request: ExportRequest,
    ) -> Result<JobSubmission, String> {
        let request = export_request(request)?;
        let manager = self.manager()?;
        if self.artifact_mutation_active() || pipeline_job_active(&manager) {
            return Err("Wait for the active document or pipeline update before exporting.".into());
        }
        if manager.list().iter().any(|job| {
            job.kind == JobKind::Export
                && !matches!(job.state, JobState::Succeeded | JobState::Failed | JobState::Cancelled)
        }) {
            return Err("An export is already active.".into());
        }

        let title = request.title;
        let export = request.export;
        let format = export_format_label(export.format);
        let player_safe = export.player_safe;
        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = manager.submit(JobKind::Export, title, reporter.clone(), move |context| async move {
            let reporter = context.reporter();
            ui::with_reporter(reporter, async move {
                ui::phase("Build managed export");
                let summary = tauri::async_runtime::spawn_blocking(move || {
                    sessionsmith::commands::export::export(export)
                        .map(|result| {
                            let audience = if player_safe { " player-safe" } else { "" };
                            format!("exported {} {format}{audience} session(s)", result.session_count)
                        })
                        .map_err(|_| std::io::Error::other("The selected session notes could not be exported."))
                })
                .await
                .map_err(std::io::Error::other)??;
                ui::ok(&summary);
                Ok(summary)
            })
            .await
        });
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_transcribe(
        &self,
        app: AppHandle,
        request: TranscribeRequest,
    ) -> Result<JobSubmission, String> {
        let request = transcribe_request(request)?;
        let manager = self.manager()?;
        if self.artifact_mutation_active() {
            return Err("Wait for the active local update before starting a pipeline job.".into());
        }
        if pipeline_job_active(&manager) {
            return Err("A pipeline job is already active.".into());
        }

        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = spawn_with_reporter(&manager, reporter.clone(), request);
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_model(
        &self,
        app: AppHandle,
        request: ModelRequest,
    ) -> Result<JobSubmission, String> {
        let (global, job, title) = model_request(request)?;
        let manager = self.manager()?;
        if model_job_active(&manager) {
            return Err("A local model action is already active.".into());
        }

        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = spawn_model_with_reporter(&manager, reporter.clone(), global, job, title);
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_speaker_map(
        &self,
        app: AppHandle,
        request: SpeakerMapRequest,
    ) -> Result<JobSubmission, String> {
        let request = speaker_map_request(request)?;
        let manager = self.manager()?;
        if manager.list().iter().any(|job| {
            job.kind == JobKind::SpeakerMap
                && !matches!(job.state, JobState::Succeeded | JobState::Failed | JobState::Cancelled)
        }) {
            return Err("A speaker mapping update is already active.".into());
        }

        let title = format!("Map speakers · {}", request.stem);
        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = manager.submit(JobKind::SpeakerMap, title, reporter.clone(), move |context| async move {
            let reporter = context.reporter();
            let cancellation = context.cancellation();
            ui::with_reporter(reporter, async move {
                if cancellation.is_cancelled() {
                    return Err(std::io::Error::other("cancelled").into());
                }
                ui::phase("Apply speaker mapping");
                let summary = tauri::async_runtime::spawn_blocking(move || -> Result<String, std::io::Error> {
                    if cancellation.is_cancelled() {
                        return Err(std::io::Error::other("cancelled"));
                    }
                    sessionsmith::speakers::apply_to_session(
                        &request.transcripts_dir,
                        &request.stem,
                        &request.mappings,
                    )
                    .map_err(std::io::Error::other)?;
                    Ok(format!(
                        "mapped {} speaker label(s)",
                        request.mappings.len()
                    ))
                })
                .await
                .map_err(std::io::Error::other)??;
                ui::ok(&summary);
                Ok(summary)
            })
            .await
        });
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_candidate_resolve(
        &self,
        app: AppHandle,
        request: CandidateResolveRequest,
    ) -> Result<JobSubmission, String> {
        let request = candidate_resolve_request(request)?;
        let manager = self.manager()?;
        if pipeline_job_active(&manager) {
            return Err("Wait for the active pipeline job before resolving a candidate.".into());
        }
        if manager.list().iter().any(|job| {
            job.kind == JobKind::CandidateResolve
                && !matches!(job.state, JobState::Succeeded | JobState::Failed | JobState::Cancelled)
        }) {
            return Err("A candidate resolution is already active.".into());
        }
        let mutation = self.begin_artifact_mutation()?;

        let title = format!("{} candidate · {}", candidate_action_label(request.action), request.stem);
        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = manager.submit(JobKind::CandidateResolve, title, reporter.clone(), move |context| async move {
            let _mutation = mutation;
            let reporter = context.reporter();
            ui::with_reporter(reporter, async move {
                ui::phase("Resolve candidate artifact");
                let (summary, index_warning) = tauri::async_runtime::spawn_blocking(move || -> Result<_, std::io::Error> {
                    let summary = match request.action {
                        CandidateAction::KeepCandidate => {
                            sessionsmith::candidates::promote(&request.candidate_path, &request.current_path)
                                .map_err(std::io::Error::other)?;
                            format!(
                                "kept candidate {}",
                                request.artifact.label()
                            )
                        }
                        CandidateAction::KeepBoth => {
                            let alternate = sessionsmith::candidates::keep_both(
                                &request.candidate_path,
                                &request.current_path,
                            )
                            .map_err(std::io::Error::other)?;
                            let name = alternate.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
                                std::io::Error::other("alternate artifact has no UTF-8 file name")
                            })?;
                            format!("kept candidate as {name}")
                        }
                        CandidateAction::DiscardCandidate => {
                            sessionsmith::candidates::discard(&request.candidate_path)
                                .map_err(std::io::Error::other)?;
                            format!("discarded candidate {}", request.artifact.label())
                        }
                    };
                    let index_warning = request.index_enabled.then(|| {
                        sessionsmith::index::record_session(
                            &request.campaign,
                            &request.stem,
                            &request.notes_dir,
                        )
                        .err()
                        .map(|error| format!("Search index needs rebuilding: {error}"))
                    }).flatten();
                    Ok((summary, index_warning))
                })
                .await
                .map_err(std::io::Error::other)??;
                if let Some(warning) = index_warning {
                    ui::warn(&warning);
                }
                ui::ok(&summary);
                Ok(summary)
            })
            .await
        });
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn submit_notes(
        &self,
        app: AppHandle,
        request: NotesRequest,
    ) -> Result<JobSubmission, String> {
        let request = notes_request(request)?;
        let manager = self.manager()?;
        if self.artifact_mutation_active() {
            return Err("Wait for the active document update before generating notes.".into());
        }
        if pipeline_job_active(&manager) {
            return Err("A pipeline job is already active.".into());
        }

        let reporter = Arc::new(DesktopReporter::new(self.clone(), app.clone()));
        let id = spawn_with_reporter(&manager, reporter.clone(), request);
        reporter.bind(id);
        self.emit_snapshot(&app, id);
        Ok(JobSubmission { id })
    }

    pub fn list(&self) -> Vec<JobSnapshot> {
        let Some(manager) = self.manager_if_initialized() else {
            return Vec::new();
        };

        manager
            .list()
            .into_iter()
            .map(|snapshot| self.snapshot_from_managed(snapshot))
            .collect()
    }

    pub fn cancel(&self, app: &AppHandle, id: JobId) -> Result<(), String> {
        let manager = self
            .manager_if_initialized()
            .ok_or_else(|| format!("Job {id} was not found."))?;
        let job = manager
            .list()
            .into_iter()
            .find(|job| job.id == id)
            .ok_or_else(|| format!("Job {id} was not found."))?;
        if !job.can_cancel || !job_supports_cancellation(&job.kind) {
            return Err(format!("Job {id} cannot be cancelled."));
        }
        if !manager.cancel(id) {
            return Err(format!("Job {id} is unknown or already finished."));
        }
        self.emit_snapshot(app, id);
        Ok(())
    }

    pub(crate) fn begin_artifact_mutation(&self) -> Result<ArtifactMutationLease, String> {
        let mut active = self
            .inner
            .artifact_mutation_active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *active {
            return Err("Another document update is still being applied.".into());
        }
        *active = true;
        if self
            .manager_if_initialized()
            .is_some_and(|manager| pipeline_job_active(&manager))
        {
            *active = false;
            return Err("Wait for the active pipeline job before editing a document.".into());
        }
        Ok(ArtifactMutationLease {
            inner: self.inner.clone(),
        })
    }

    fn artifact_mutation_active(&self) -> bool {
        *self
            .inner
            .artifact_mutation_active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn manager(&self) -> Result<JobManager, String> {
        if let Some(manager) = self.manager_if_initialized() {
            return Ok(manager);
        }

        let manager = JobManager::new(tauri::async_runtime::handle().inner().clone());
        let _ = self.inner.manager.set(manager);
        self.manager_if_initialized()
            .ok_or_else(|| "Desktop job manager could not be initialized.".into())
    }

    fn manager_if_initialized(&self) -> Option<JobManager> {
        self.inner.manager.get().cloned()
    }

    fn record_event(&self, app: &AppHandle, id: JobId, event: JobEvent) {
        let mut details = self
            .inner
            .details
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        apply_event(details.entry(id).or_default(), event);
        drop(details);
        self.emit_snapshot(app, id);
    }

    fn emit_snapshot(&self, app: &AppHandle, id: JobId) {
        let Some(manager) = self.manager_if_initialized() else {
            return;
        };
        let Some(snapshot) = manager.list().into_iter().find(|job| job.id == id) else {
            return;
        };
        let _ = app.emit(JOB_UPDATED_EVENT, self.snapshot_from_managed(snapshot));
    }

    fn snapshot_from_managed(&self, snapshot: ManagedJobSnapshot) -> JobSnapshot {
        let details = self
            .inner
            .details
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&snapshot.id)
            .cloned()
            .unwrap_or_default();

        JobSnapshot {
            id: snapshot.id,
            kind: job_kind(&snapshot.kind).into(),
            title: snapshot.title,
            state: job_state(&snapshot.state).into(),
            started_at: epoch_seconds(snapshot.started_at),
            finished_at: epoch_seconds(snapshot.finished_at),
            summary: snapshot.summary,
            active_children: snapshot.active_children,
            phase: details.phase,
            progress: details.progress,
            log_tail: details.log_tail.into_iter().collect(),
            can_cancel: matches!(snapshot.state, JobState::Queued | JobState::Running)
                && snapshot.can_cancel
                && job_supports_cancellation(&snapshot.kind),
        }
    }
}

struct DesktopReporter {
    jobs: DesktopJobs,
    app: AppHandle,
    route: Mutex<ReporterRoute>,
}

#[derive(Default)]
struct ReporterRoute {
    id: Option<JobId>,
    pending: Vec<JobEvent>,
}

impl DesktopReporter {
    fn new(jobs: DesktopJobs, app: AppHandle) -> Self {
        Self {
            jobs,
            app,
            route: Mutex::new(ReporterRoute::default()),
        }
    }

    fn bind(&self, id: JobId) {
        let pending = {
            let mut route = self
                .route
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            route.id = Some(id);
            std::mem::take(&mut route.pending)
        };
        for event in pending {
            self.jobs.record_event(&self.app, id, event);
        }
    }
}

impl Reporter for DesktopReporter {
    fn event(&self, event: JobEvent) {
        enum Delivery {
            Now(JobId, JobEvent),
            Pending,
        }

        let delivery = {
            let mut route = self
                .route
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match route.id {
                Some(id) => Delivery::Now(id, event),
                None => {
                    route.pending.push(event);
                    Delivery::Pending
                }
            }
        };

        if let Delivery::Now(id, event) = delivery {
            self.jobs.record_event(&self.app, id, event);
        }
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}

fn apply_event(details: &mut JobDetails, event: JobEvent) {
    match event {
        JobEvent::Header(message) => push_log(details, message),
        JobEvent::Step { n, total, msg } => push_log(details, format!("[{n}/{total}] {msg}")),
        JobEvent::Ok(message) => push_log(details, message),
        JobEvent::Warn(message) => push_log(details, format!("Warning: {message}")),
        JobEvent::Error(message) => push_log(details, format!("Error: {message}")),
        JobEvent::Info(message) => push_log(details, message),
        JobEvent::Progress {
            label,
            pos,
            total,
            rate,
        } => {
            details.progress = Some(JobProgress {
                label,
                position: pos,
                total,
                rate,
            });
        }
        JobEvent::Phase(phase) => {
            details.phase = Some(phase.clone());
            details.progress = None;
            push_log(details, phase);
        }
        JobEvent::ArtifactWritten {
            session, artifact, ..
        } => push_log(details, format!("{session}: wrote {artifact}")),
        JobEvent::JobDone(result) => match result {
            Ok(summary) => push_log(details, summary),
            Err(error) if error == "cancelled" => push_log(details, "Cancelled".into()),
            Err(error) => push_log(details, format!("Error: {error}")),
        },
    }
}

fn push_log(details: &mut JobDetails, message: String) {
    if details.log_tail.len() == LOG_TAIL_LIMIT {
        details.log_tail.pop_front();
    }
    details.log_tail.push_back(message);
}

fn job_kind(kind: &JobKind) -> &'static str {
    match kind {
        JobKind::Import => "import",
        JobKind::Run => "run",
        JobKind::Transcribe => "transcribe",
        JobKind::Notes => "notes",
        JobKind::Doctor => "doctor",
        JobKind::RebuildLog => "rebuildLog",
        JobKind::Reindex => "reindex",
        JobKind::CandidateResolve => "candidateResolve",
        JobKind::SpeakerMap => "speakerMap",
        JobKind::Model => "model",
        JobKind::Export => "export",
        JobKind::Record => "record",
    }
}

fn job_supports_cancellation(kind: &JobKind) -> bool {
    !matches!(
        kind,
        JobKind::Reindex | JobKind::CandidateResolve | JobKind::SpeakerMap | JobKind::Export
    )
}

fn job_state(state: &JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Cancelling => "cancelling",
        JobState::Succeeded => "succeeded",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
    }
}

fn epoch_seconds(time: Option<SystemTime>) -> Option<u64> {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
}

fn recording_request(request: RecordRequest) -> Result<RecordingRequest, String> {
    let request = RecordingRequest {
        name: request.name.trim().into(),
        device: None,
        format: None,
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

fn import_request(request: ImportAudioRequest) -> Result<sessionsmith::jobs::import_audio::AudioImportRequest, String> {
    let campaign_id = request.campaign_id.trim();
    if campaign_id.is_empty() {
        return Err("A campaign is required for audio import.".into());
    }
    if request.source_paths.is_empty() {
        return Err("Select at least one audio file to import.".into());
    }
    if request.source_paths.len() > 100 {
        return Err("Select no more than 100 audio files at a time.".into());
    }
    if request.source_paths.iter().any(|path| path.trim().is_empty()) {
        return Err("Selected audio paths must not be empty.".into());
    }

    Ok(sessionsmith::jobs::import_audio::AudioImportRequest {
        inbox: campaign_inbox_dir(campaign_id)?,
        sources: request.source_paths.into_iter().map(PathBuf::from).collect(),
    })
}

struct ValidatedExportRequest {
    title: String,
    export: CoreExportRequest,
}

fn export_request(request: ExportRequest) -> Result<ValidatedExportRequest, String> {
    let campaign_id = request.campaign_id.trim();
    if campaign_id.is_empty() {
        return Err("A campaign is required for export.".into());
    }
    if request.all && !request.stems.is_empty() {
        return Err("Choose all sessions or selected sessions, not both.".into());
    }
    if !request.all && request.stems.is_empty() {
        return Err("Select at least one session to export.".into());
    }
    if request.stems.len() > 100 {
        return Err("Select no more than 100 sessions to export at once.".into());
    }

    let library = crate::commands::campaign_library(campaign_id.into())?;
    let root = workspace_root();
    let campaign_path = campaign_config_path(&root, campaign_id)?;
    let campaign = CampaignConfig::load(&campaign_path).map_err(|error| error.to_string())?;
    let notes_dir = resolve_workspace_path(&root, &config::output_dir())
        .join(campaign.slug())
        .join("notes");
    let notes_dir = fs::canonicalize(&notes_dir)
        .map_err(|_| "This campaign has no generated session notes to export.".to_string())?;
    if !notes_dir.is_dir() {
        return Err("This campaign has no generated session notes to export.".into());
    }
    let available_sessions: BTreeMap<_, _> = library
        .sessions
        .into_iter()
        .filter_map(|session| {
            let stem = session.stem;
            approved_export_session_dir(&notes_dir, &stem).map(|notes| (stem, notes))
        })
        .collect();

    let session_dirs = if request.all {
        available_sessions.into_values().collect()
    } else {
        let mut selected = BTreeSet::new();
        let mut session_dirs = Vec::with_capacity(request.stems.len());
        for raw_stem in request.stems {
            let stem = raw_stem.trim();
            if !is_valid_export_stem(stem) {
                return Err("Selected sessions must use valid session names.".into());
            }
            if !selected.insert(stem.to_string()) {
                return Err("A session can be exported only once per request.".into());
            }
            let notes = available_sessions.get(stem).ok_or_else(|| {
                "Selected sessions must still have generated notes in this campaign.".to_string()
            })?;
            session_dirs.push(notes.clone());
        }
        session_dirs.sort();
        session_dirs
    };
    if session_dirs.is_empty() {
        return Err("This campaign has no generated session notes to export.".into());
    }

    let format = match request.format {
        ExportFormat::Html => CoreExportFormat::Html,
        ExportFormat::Obsidian => CoreExportFormat::Obsidian,
    };
    let title = format!("Export {} · {}", export_format_label(format), campaign.campaign.name);
    let output_dir = managed_export_dir(&root, &campaign.slug())?;

    Ok(ValidatedExportRequest {
        title,
        export: CoreExportRequest {
            campaign_name: campaign.campaign.name,
            session_dirs,
            output_dir,
            format,
            player_safe: request.player_safe,
        },
    })
}

fn is_valid_export_stem(stem: &str) -> bool {
    is_simple_session_stem(stem)
        && stem.len() <= 100
        && !stem.chars().any(char::is_control)
}

fn approved_export_session_dir(notes_dir: &Path, stem: &str) -> Option<PathBuf> {
    if !is_valid_export_stem(stem) {
        return None;
    }
    let session = fs::canonicalize(notes_dir.join(stem)).ok()?;
    (session.is_dir() && session.starts_with(notes_dir)).then_some(session)
}

fn export_format_label(format: CoreExportFormat) -> &'static str {
    match format {
        CoreExportFormat::Html => "HTML",
        CoreExportFormat::Obsidian => "Obsidian",
    }
}

fn managed_export_dir(workspace_root: &Path, campaign_slug: &str) -> Result<PathBuf, String> {
    if campaign_slug.is_empty()
        || Path::new(campaign_slug)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(campaign_slug)
    {
        return Err("The selected campaign has no valid managed export folder.".into());
    }
    let workspace_root = fs::canonicalize(workspace_root)
        .map_err(|_| "The workspace export folder could not be prepared.".to_string())?;
    let exports_root = workspace_root.join("exports");
    fs::create_dir_all(&exports_root)
        .map_err(|_| "The workspace export folder could not be prepared.".to_string())?;
    let exports_root = fs::canonicalize(&exports_root)
        .map_err(|_| "The workspace export folder could not be prepared.".to_string())?;
    if !exports_root.is_dir() || !exports_root.starts_with(&workspace_root) {
        return Err("The workspace export folder is outside the approved workspace.".into());
    }

    let destination = exports_root.join(campaign_slug);
    fs::create_dir_all(&destination)
        .map_err(|_| "The managed campaign export folder could not be prepared.".to_string())?;
    let destination = fs::canonicalize(&destination)
        .map_err(|_| "The managed campaign export folder could not be prepared.".to_string())?;
    if !destination.is_dir() || !destination.starts_with(&exports_root) {
        return Err("The managed campaign export folder is outside the approved workspace.".into());
    }
    Ok(destination)
}

fn process_request(request: ProcessRequest) -> Result<JobRequest, String> {
    let artifacts = artifacts_from_ids(&request.artifact_ids)?;
    let (mut context, sessions) = selected_inbox_sessions(
        &request.campaign_id,
        &request.source_paths,
        request.all_inbox,
        request.combine,
        request.session_name,
    )?;
    let transcription = apply_transcription_overrides(
        &mut context,
        request.asr_model,
        request.language,
        request.session_date,
        request.diarize,
        request.vad,
    )?;
    let model_override = apply_notes_overrides(
        &mut context.global,
        request.backend_kind,
        request.llm_model,
    )?;

    Ok(JobRequest {
        kind: PipelineJobKind::Run,
        g: context.global,
        campaign: context.campaign,
        preset: context.preset,
        asr_model: transcription.asr_model,
        language: transcription.language,
        session_date: transcription.session_date,
        sessions,
        transcripts: Vec::new(),
        artifacts,
        force: request.force || request.candidate,
        force_transcribe: request.force,
        resume: request.resume,
        update_log: !request.candidate,
        candidate: request.candidate,
        model_override,
    })
}

fn transcribe_request(request: TranscribeRequest) -> Result<JobRequest, String> {
    let (mut context, sessions) = selected_inbox_sessions(
        &request.campaign_id,
        &request.source_paths,
        request.all_inbox,
        request.combine,
        request.session_name,
    )?;
    let transcription = apply_transcription_overrides(
        &mut context,
        request.asr_model,
        request.language,
        request.session_date,
        request.diarize,
        request.vad,
    )?;

    Ok(JobRequest {
        kind: PipelineJobKind::Transcribe,
        g: context.global,
        campaign: context.campaign,
        preset: context.preset,
        asr_model: transcription.asr_model,
        language: transcription.language,
        session_date: transcription.session_date,
        sessions,
        transcripts: Vec::new(),
        artifacts: Vec::new(),
        force: request.force,
        force_transcribe: request.force,
        resume: true,
        update_log: false,
        candidate: false,
        model_override: None,
    })
}

struct ResolvedTranscription {
    asr_model: String,
    language: String,
    session_date: Option<String>,
}

fn apply_transcription_overrides(
    context: &mut ProcessingContext,
    asr_model: Option<String>,
    language: Option<String>,
    session_date: Option<String>,
    diarize: bool,
    vad: bool,
) -> Result<ResolvedTranscription, String> {
    let asr_model = match asr_model {
        Some(model) => validate_asr_model(&model)?,
        None => context.asr_model.clone(),
    };
    if diarize {
        context.global.asr.diarize = true;
    }
    if vad {
        context.global.asr.vad = true;
    }

    Ok(ResolvedTranscription {
        asr_model,
        language: validate_language(language)?,
        session_date: validate_session_date(session_date)?,
    })
}

fn apply_notes_overrides(
    global: &mut GlobalConfig,
    backend_kind: Option<String>,
    llm_model: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(kind) = backend_kind {
        global.backend.kind = validate_backend_kind(&kind)?;
    }
    let model_override = validate_llm_model(llm_model)?;
    if let Some(model) = &model_override {
        global.backend.model = Some(model.clone());
    }
    Ok(model_override)
}

fn validate_asr_model(value: &str) -> Result<String, String> {
    let model_id = value.trim();
    let known_whisper = sessionsmith::models::WHISPER_MODELS
        .iter()
        .any(|model| model.id == model_id);
    let known_asr = sessionsmith::asr::ASR_CATALOG
        .iter()
        .any(|model| model.id == model_id);
    if known_whisper || known_asr {
        Ok(model_id.to_string())
    } else {
        Err("Select an ASR model from the local catalog.".into())
    }
}

fn validate_language(value: Option<String>) -> Result<String, String> {
    let language = value.unwrap_or_else(|| "auto".into());
    let language = language.trim().to_ascii_lowercase();
    if language == "auto" {
        return Ok(language);
    }

    let mut parts = language.split('-');
    let Some(primary) = parts.next() else {
        return Err("Use 'auto' or a standard language tag such as 'en' or 'pt-br'.".into());
    };
    let valid_primary = (2..=3).contains(&primary.len())
        && primary.bytes().all(|character| character.is_ascii_lowercase());
    let valid_subtags = parts.all(|part| {
        (2..=8).contains(&part.len())
            && part.bytes().all(|character| character.is_ascii_lowercase())
    });
    if valid_primary && valid_subtags {
        Ok(language)
    } else {
        Err("Use 'auto' or a standard language tag such as 'en' or 'pt-br'.".into())
    }
}

fn validate_session_date(value: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    sessionsmith::transcribe::session_date_for(Path::new("desktop-session.wav"), Some(value))
        .map(Some)
        .map_err(|error| error.to_string())
}

fn validate_backend_kind(value: &str) -> Result<String, String> {
    let backend = value.trim().to_ascii_lowercase();
    match backend.as_str() {
        "ollama" | "openai" | "anthropic" => Ok(backend),
        _ => Err("Select Ollama, OpenAI, or Anthropic as the notes backend.".into()),
    }
}

fn validate_llm_model(value: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let model = value.trim();
    if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
        return Err("Enter a valid notes model name.".into());
    }
    Ok(Some(model.into()))
}

fn model_request(request: ModelRequest) -> Result<(GlobalConfig, ModelJob, String), String> {
    let id = request.model_id.trim();
    if id.is_empty() {
        return Err("Select a local model to manage.".into());
    }

    let (job, title) = match request.action {
        ModelAction::DownloadWhisper | ModelAction::DeleteWhisper => {
            if !sessionsmith::models::WHISPER_MODELS
                .iter()
                .any(|model| model.id == id)
            {
                return Err("Select a Whisper model from the local catalog.".into());
            }
            match request.action {
                ModelAction::DownloadWhisper => (
                    ModelJob::PullWhisper(id.to_string()),
                    format!("Download Whisper · {id}"),
                ),
                ModelAction::DeleteWhisper => (
                    ModelJob::DeleteWhisper(id.to_string()),
                    format!("Delete Whisper · {id}"),
                ),
                _ => unreachable!("outer match restricts Whisper actions"),
            }
        }
        ModelAction::PrepareAsr | ModelAction::DeleteAsr => {
            let spec = sessionsmith::asr::find(id)
                .ok_or_else(|| "Select an ASR model from the local catalog.".to_string())?;
            if spec.engine == sessionsmith::asr::AsrEngine::WhisperCpp {
                return Err("Manage Whisper models through the Whisper catalog.".into());
            }
            match request.action {
                ModelAction::PrepareAsr => (
                    ModelJob::PrepareAsr(id.to_string()),
                    format!("Prepare ASR · {}", spec.display),
                ),
                ModelAction::DeleteAsr => (
                    ModelJob::DeleteAsr(id.to_string()),
                    format!("Delete ASR · {}", spec.display),
                ),
                _ => unreachable!("outer match restricts ASR actions"),
            }
        }
        ModelAction::PullOllama | ModelAction::DeleteOllama => {
            let known = sessionsmith::models::OLLAMA_CATALOG
                .iter()
                .flat_map(|model| model.options)
                .any(|option| option.pull == id);
            if !known {
                return Err("Select an Ollama model from the local catalog.".into());
            }
            match request.action {
                ModelAction::PullOllama => (
                    ModelJob::PullOllama(id.to_string()),
                    format!("Download Ollama · {id}"),
                ),
                ModelAction::DeleteOllama => (
                    ModelJob::DeleteOllama(id.to_string()),
                    format!("Delete Ollama · {id}"),
                ),
                _ => unreachable!("outer match restricts Ollama actions"),
            }
        }
    };

    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    Ok((global, job, title))
}

struct ValidatedSpeakerMapRequest {
    stem: String,
    transcripts_dir: PathBuf,
    mappings: BTreeMap<String, String>,
}

fn speaker_map_request(request: SpeakerMapRequest) -> Result<ValidatedSpeakerMapRequest, String> {
    let campaign_id = request.campaign_id.trim();
    if campaign_id.is_empty() {
        return Err("A campaign is required for speaker mapping.".into());
    }
    let stem = request.stem.trim();
    if !is_simple_session_stem(stem) {
        return Err("Select a valid session transcript to map speakers.".into());
    }
    crate::workspace::session_workspace(campaign_id.to_string(), stem.to_string())?;
    let context = processing_context(campaign_id)?;
    let raw_txt = context.transcripts_dir.join(format!("{stem}.diarized.txt"));
    let txt = context.transcripts_dir.join(format!("{stem}.txt"));
    let source = if raw_txt.is_file() { raw_txt } else { txt };
    let text = fs::read_to_string(&source)
        .map_err(|error| format!("Could not read the selected transcript: {error}"))?;
    let labels = sessionsmith::speakers::labels(&text)
        .into_iter()
        .collect::<BTreeSet<_>>();
    if labels.is_empty() {
        return Err("This session has no diarized speaker labels to map.".into());
    }

    let mappings = validate_speaker_mappings(request.mappings, &labels)?;

    Ok(ValidatedSpeakerMapRequest {
        stem: stem.into(),
        transcripts_dir: context.transcripts_dir,
        mappings,
    })
}

fn validate_speaker_mappings(
    entries: Vec<SpeakerMapEntry>,
    labels: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, String> {
    if entries.is_empty() || entries.len() > 40 {
        return Err("Provide between one and forty speaker mappings.".into());
    }
    let mut mappings = BTreeMap::new();
    for entry in entries {
        if entry.name.chars().any(char::is_control) {
            return Err("Speaker names must be printable and at most 100 characters.".into());
        }
        let (label, name) = sessionsmith::speakers::parse_mapping(&format!(
            "{}={}",
            entry.label.trim(),
            entry.name.trim()
        ))
        .map_err(|error| error.to_string())?;
        if name.chars().count() > 100 {
            return Err("Speaker names must be printable and at most 100 characters.".into());
        }
        if !labels.contains(&label) {
            return Err("Speaker mappings must use labels from the selected session.".into());
        }
        if mappings.insert(label, name).is_some() {
            return Err("Each speaker label can be mapped only once.".into());
        }
    }

    Ok(mappings)
}

struct ValidatedCandidateResolveRequest {
    action: CandidateAction,
    artifact: Artifact,
    campaign: CampaignConfig,
    stem: String,
    notes_dir: PathBuf,
    current_path: PathBuf,
    candidate_path: PathBuf,
    index_enabled: bool,
}

fn candidate_resolve_request(
    request: CandidateResolveRequest,
) -> Result<ValidatedCandidateResolveRequest, String> {
    let campaign_id = request.campaign_id.trim();
    if campaign_id.is_empty() {
        return Err("A campaign is required to resolve a candidate.".into());
    }
    let stem = request.stem.trim();
    if !is_simple_session_stem(stem) {
        return Err("Select a valid session artifact to resolve its candidate.".into());
    }
    let artifact = Artifact::from_id(request.artifact_id.trim())
        .ok_or_else(|| "Select a known generated artifact candidate.".to_string())?;
    crate::workspace::session_workspace(campaign_id.to_string(), stem.to_string())?;
    let context = processing_context(campaign_id)?;
    let notes_dir = context.campaign.notes_dir().join(stem);
    let current_path = notes_dir.join(pipeline::artifact_file(artifact, false));
    let candidate_path = notes_dir.join(pipeline::artifact_file(artifact, true));
    if !candidate_path.is_file() {
        return Err("The selected artifact candidate no longer exists.".into());
    }
    if matches!(request.action, CandidateAction::KeepBoth) && !current_path.is_file() {
        return Err("Keeping both requires an existing current artifact.".into());
    }

    Ok(ValidatedCandidateResolveRequest {
        action: request.action,
        artifact,
        campaign: context.campaign,
        stem: stem.into(),
        notes_dir,
        current_path,
        candidate_path,
        index_enabled: context.global.runtime.index,
    })
}

fn candidate_action_label(action: CandidateAction) -> &'static str {
    match action {
        CandidateAction::KeepCandidate => "Keep",
        CandidateAction::KeepBoth => "Keep both",
        CandidateAction::DiscardCandidate => "Discard",
    }
}

fn selected_inbox_sessions(
    campaign_id: &str,
    source_paths: &[String],
    all_inbox: bool,
    combine: bool,
    session_name: Option<String>,
) -> Result<(ProcessingContext, Vec<SessionInput>), String> {
    let campaign_id = campaign_id.trim();
    if campaign_id.is_empty() {
        return Err("A campaign is required for processing.".into());
    }
    if all_inbox && !source_paths.is_empty() {
        return Err("Choose either all current Inbox audio or specific Inbox audio files.".into());
    }
    if !all_inbox && source_paths.is_empty() {
        return Err("Select at least one Inbox audio file to process.".into());
    }
    if !combine && session_name.as_deref().is_some_and(|name| !name.trim().is_empty()) {
        return Err("A combined session name requires combining the selected files.".into());
    }

    let context = processing_context(campaign_id)?;
    let selected_files = if all_inbox {
        context.inbox_paths.clone()
    } else {
        let mut available_paths: BTreeMap<_, _> = context
            .inbox_paths
            .iter()
            .cloned()
            .map(|path| (path.display().to_string(), path))
            .collect();
        let mut selected_paths = BTreeSet::new();
        let mut selected_files = Vec::with_capacity(source_paths.len());

        for source_path in source_paths {
            let source_path = source_path.trim();
            if source_path.is_empty() {
                return Err("Selected Inbox audio paths must not be empty.".into());
            }
            if !selected_paths.insert(source_path.to_string()) {
                return Err("An Inbox audio file was selected more than once.".into());
            }
            let path = available_paths.remove(source_path).ok_or_else(|| {
                "Selected audio must still be an unprocessed file in this campaign's Inbox.".to_string()
            })?;
            selected_files.push(path);
        }
        selected_files
    };

    if selected_files.is_empty() {
        return Err("There is no current Inbox audio to process.".into());
    }
    if selected_files.len() > 20 {
        return Err("Select no more than 20 audio files at a time.".into());
    }
    if combine && selected_files.len() < 2 {
        return Err("Select at least two Inbox audio files to combine.".into());
    }
    let mut session_names = BTreeSet::new();
    for path in &selected_files {
        let name = session_name_from_path(path)?;
        if !combine && !session_names.insert(name) {
            return Err("Selected Inbox audio files must have distinct session names.".into());
        }
    }

    if combine {
        let name = session_name
            .as_deref()
            .map(str::trim)
            .filter(|name| is_valid_new_session_stem(name))
            .ok_or_else(|| "Enter a simple name for the combined session.".to_string())?;
        if context.existing_stems.contains(name) {
            return Err("A session with that name already exists in this campaign.".into());
        }
        return Ok((
            context,
            vec![SessionInput {
                files: selected_files,
                name: name.into(),
            }],
        ));
    }

    let sessions = selected_files
        .into_iter()
        .map(|path| {
            let name = session_name_from_path(&path)?;
            Ok(SessionInput {
                files: vec![path],
                name,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((context, sessions))
}

fn session_name_from_path(path: &Path) -> Result<String, String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Selected Inbox audio must have a valid UTF-8 filename.".to_string())
}

fn is_valid_new_session_stem(stem: &str) -> bool {
    is_simple_session_stem(stem)
        && stem.len() <= 100
        && !stem.chars().any(char::is_control)
}

fn log_rebuild_request(campaign_id: &str) -> Result<JobRequest, String> {
    if campaign_id.trim().is_empty() {
        return Err("A campaign is required to rebuild its log.".into());
    }
    let context = processing_context(campaign_id.trim())?;
    Ok(JobRequest {
        kind: PipelineJobKind::RebuildLog,
        g: context.global,
        campaign: context.campaign,
        preset: context.preset,
        asr_model: context.asr_model,
        language: "auto".into(),
        session_date: None,
        sessions: Vec::new(),
        transcripts: Vec::new(),
        artifacts: Vec::new(),
        force: false,
        force_transcribe: false,
        resume: true,
        update_log: false,
        candidate: false,
        model_override: None,
    })
}

fn notes_request(request: NotesRequest) -> Result<JobRequest, String> {
    let campaign_id = request.campaign_id.trim();
    if campaign_id.is_empty() {
        return Err("A campaign is required for note generation.".into());
    }
    let stem = request.stem.trim();
    if !is_simple_session_stem(stem) {
        return Err("Select a valid session transcript to generate notes.".into());
    }
    let artifacts = artifacts_from_ids(&request.artifact_ids)?;
    let context = processing_context(campaign_id)?;
    let transcript = context.transcripts_dir.join(format!("{stem}.txt"));
    if !transcript.is_file() {
        return Err("The selected session transcript no longer exists.".into());
    }

    Ok(JobRequest {
        kind: PipelineJobKind::Notes,
        g: context.global,
        campaign: context.campaign,
        preset: context.preset,
        asr_model: context.asr_model,
        language: "auto".into(),
        session_date: None,
        sessions: Vec::new(),
        transcripts: vec![transcript],
        artifacts,
        force: request.force || request.candidate,
        force_transcribe: false,
        resume: request.resume,
        update_log: !request.candidate,
        candidate: request.candidate,
        model_override: None,
    })
}

fn pipeline_job_active(manager: &JobManager) -> bool {
    manager.list().iter().any(|job| {
        matches!(
            job.kind,
            JobKind::Run | JobKind::Transcribe | JobKind::Notes | JobKind::RebuildLog
        ) && !matches!(
            job.state,
            JobState::Succeeded | JobState::Failed | JobState::Cancelled
        )
    })
}

fn model_job_active(manager: &JobManager) -> bool {
    manager.list().iter().any(|job| {
        job.kind == JobKind::Model
            && !matches!(job.state, JobState::Succeeded | JobState::Failed | JobState::Cancelled)
    })
}

fn artifacts_from_ids(artifact_ids: &[String]) -> Result<Vec<Artifact>, String> {
    if artifact_ids.is_empty() {
        return Err("Select at least one output to generate.".into());
    }

    let mut ids = BTreeSet::new();
    let mut artifacts = Vec::new();
    for artifact_id in artifact_ids {
        let artifact_id = artifact_id.trim();
        if artifact_id.is_empty() {
            return Err("Output identifiers must not be empty.".into());
        }
        if ids.insert(artifact_id.to_string()) {
            let artifact = Artifact::from_id(artifact_id)
                .ok_or_else(|| format!("Unknown output '{artifact_id}'."))?;
            artifacts.push(artifact);
        }
    }
    Ok(artifacts)
}

fn processing_context(campaign_id: &str) -> Result<ProcessingContext, String> {
    crate::commands::campaign_library(campaign_id.into())?;
    let root = workspace_root();
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    let campaign_path = campaign_config_path(&root, campaign_id)?;
    let campaign = CampaignConfig::load(&campaign_path).map_err(|error| error.to_string())?;
    let global = config::effective(&global, &campaign);
    let preset = sessionsmith::presets::load(&campaign.system.preset)
        .map_err(|error| error.to_string())?;
    let asr_model = global.asr.model.clone().unwrap_or_else(|| {
        hardware::recommend(&hardware::detect())
            .whisper_model
            .to_string()
    });
    let audio_dir = resolve_workspace_path(&root, &config::audio_dir());
    let output_root = resolve_workspace_path(&root, &config::output_dir()).join(campaign.slug());
    let transcripts_dir = output_root.join("transcripts");
    let notes_dir = output_root.join("notes");
    let existing_stems = session_stems(&transcripts_dir, &notes_dir);
    let inbox_paths = audio::scan(&audio_dir, &transcripts_dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|audio| !existing_stems.contains(&audio.stem()))
        .map(|audio| audio.path)
        .collect();

    Ok(ProcessingContext {
        global,
        campaign,
        preset,
        asr_model,
        inbox_paths,
        existing_stems,
        transcripts_dir,
    })
}

fn is_simple_session_stem(stem: &str) -> bool {
    !stem.is_empty()
        && !stem.starts_with('.')
        && Path::new(stem)
            .file_name()
            .and_then(|name| name.to_str())
            == Some(stem)
}

fn campaign_config_path(root: &Path, campaign_id: &str) -> Result<PathBuf, String> {
    let id_path = Path::new(campaign_id);
    if id_path.file_name().and_then(|name| name.to_str()) != Some(campaign_id)
        || campaign_id.starts_with('.')
    {
        return Err("Campaign identifiers must be simple campaign file names.".into());
    }

    let campaign_path = root.join("campaigns").join(format!("{campaign_id}.toml"));
    if campaign_path.is_file() {
        return Ok(campaign_path);
    }
    let root_campaign = root.join("campaign.toml");
    if campaign_id == "campaign" && root_campaign.is_file() {
        return Ok(root_campaign);
    }
    Err(format!("Campaign '{campaign_id}' was not found."))
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
            if is_current_session_transcript(&path) {
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    stems.insert(stem.to_string());
                }
            }
        }
    }
    if let Ok(entries) = fs::read_dir(notes) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
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

fn is_current_session_transcript(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("txt")
        && !path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(sessionsmith::speakers::is_raw_diarized_stem)
}

fn campaign_inbox_dir(campaign_id: &str) -> Result<PathBuf, String> {
    crate::commands::campaign_library(campaign_id.into())?;
    let _global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    let audio_dir = config::audio_dir();
    if audio_dir.is_absolute() {
        Ok(audio_dir)
    } else {
        Ok(workspace_root().join(audio_dir))
    }
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

#[cfg(test)]
mod tests {
    use super::{
        apply_event, artifacts_from_ids, import_request, log_rebuild_request, notes_request,
        approved_export_session_dir, managed_export_dir, process_request, push_log,
        recording_request, ExportFormat, ExportRequest, ModelAction, ModelRequest,
        ImportAudioRequest, JobDetails, NotesRequest, ProcessRequest, RecordRequest,
        CandidateAction, CandidateResolveRequest, SpeakerMapEntry, SpeakerMapRequest, TranscribeRequest, validate_asr_model,
        validate_backend_kind, validate_language, validate_session_date, LOG_TAIL_LIMIT,
    };
    use sessionsmith::jobs::report::JobEvent;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temporary_directory(name: &str) -> PathBuf {
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
    fn desktop_jobs_initialize_from_the_tauri_runtime() {
        assert!(super::DesktopJobs::default().manager().is_ok());
    }

    #[test]
    fn structured_events_update_the_job_detail_view() {
        let mut details = JobDetails::default();
        apply_event(&mut details, JobEvent::Phase("Readiness".into()));
        apply_event(
            &mut details,
            JobEvent::Progress {
                label: "Downloading".into(),
                pos: 2,
                total: 4,
                rate: Some(1.5),
            },
        );

        assert_eq!(details.phase.as_deref(), Some("Readiness"));
        assert_eq!(details.progress.as_ref().map(|progress| progress.position), Some(2));
        assert_eq!(details.log_tail.front().map(String::as_str), Some("Readiness"));
    }

    #[test]
    fn job_log_tail_discards_the_oldest_entries() {
        let mut details = JobDetails::default();
        for index in 0..=LOG_TAIL_LIMIT {
            push_log(&mut details, index.to_string());
        }

        assert_eq!(details.log_tail.len(), LOG_TAIL_LIMIT);
        assert_eq!(details.log_tail.front().map(String::as_str), Some("1"));
        assert_eq!(details.log_tail.back().map(String::as_str), Some("80"));
    }

    #[test]
    fn record_request_is_trimmed_and_cannot_escape_the_audio_directory() {
        let request = recording_request(RecordRequest {
            name: " session-12 ".into(),
        })
        .expect("valid recording name should be accepted");
        assert_eq!(request.name, "session-12");

        assert!(recording_request(RecordRequest {
            name: "../outside".into(),
        })
        .is_err());
    }

    #[test]
    fn import_request_requires_a_campaign_and_selected_paths() {
        assert!(import_request(ImportAudioRequest {
            campaign_id: String::new(),
            source_paths: vec!["/tmp/session.wav".into()],
        })
        .is_err());
        assert!(import_request(ImportAudioRequest {
            campaign_id: "test".into(),
            source_paths: Vec::new(),
        })
        .is_err());
    }

    #[test]
    fn export_request_requires_an_explicit_nonempty_scope() {
        let request = ExportRequest {
            campaign_id: "test".into(),
            stems: Vec::new(),
            all: false,
            format: ExportFormat::Html,
            player_safe: false,
        };
        assert!(super::export_request(request).is_err());
    }

    #[test]
    fn managed_export_dir_stays_under_the_workspace() {
        let workspace = temporary_directory("managed-export");
        let output = managed_export_dir(&workspace, "test-campaign").unwrap();
        assert!(output.starts_with(workspace.join("exports")));
        assert!(output.is_dir());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn managed_export_dir_rejects_a_campaign_symlink_escape() {
        use std::os::unix::fs::symlink;

        let workspace = temporary_directory("managed-export-workspace");
        let outside = temporary_directory("managed-export-outside");
        let exports = workspace.join("exports");
        std::fs::create_dir_all(&exports).unwrap();
        symlink(&outside, exports.join("test-campaign")).unwrap();

        assert!(managed_export_dir(&workspace, "test-campaign").is_err());
        fs::remove_dir_all(workspace).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn export_session_dir_rejects_a_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = temporary_directory("export-session-root");
        let notes = root.join("notes");
        let outside = temporary_directory("export-session-outside");
        fs::create_dir_all(&notes).unwrap();
        symlink(&outside, notes.join("session-one")).unwrap();

        let notes = fs::canonicalize(notes).unwrap();
        assert!(approved_export_session_dir(&notes, "session-one").is_none());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn process_request_requires_a_campaign_sources_and_outputs() {
        assert!(process_request(ProcessRequest {
            campaign_id: String::new(),
            source_paths: vec!["/tmp/session.wav".into()],
            all_inbox: false,
            artifact_ids: vec!["summary".into()],
            resume: true,
            force: false,
            candidate: false,
            asr_model: None,
            language: None,
            session_date: None,
            diarize: false,
            vad: false,
            backend_kind: None,
            llm_model: None,
            combine: false,
            session_name: None,
        })
        .is_err());
        assert!(process_request(ProcessRequest {
            campaign_id: "test".into(),
            source_paths: Vec::new(),
            all_inbox: false,
            artifact_ids: vec!["summary".into()],
            resume: true,
            force: false,
            candidate: false,
            asr_model: None,
            language: None,
            session_date: None,
            diarize: false,
            vad: false,
            backend_kind: None,
            llm_model: None,
            combine: false,
            session_name: None,
        })
        .is_err());
        assert!(process_request(ProcessRequest {
            campaign_id: "test".into(),
            source_paths: vec!["/tmp/session.wav".into()],
            all_inbox: false,
            artifact_ids: Vec::new(),
            resume: true,
            force: false,
            candidate: false,
            asr_model: None,
            language: None,
            session_date: None,
            diarize: false,
            vad: false,
            backend_kind: None,
            llm_model: None,
            combine: false,
            session_name: None,
        })
        .is_err());
    }

    #[test]
    fn process_request_rejects_mixed_inbox_scopes() {
        let result = process_request(ProcessRequest {
            campaign_id: "test".into(),
            source_paths: vec!["/tmp/session.wav".into()],
            all_inbox: true,
            artifact_ids: vec!["summary".into()],
            resume: true,
            force: false,
            candidate: false,
            asr_model: None,
            language: None,
            session_date: None,
            diarize: false,
            vad: false,
            backend_kind: None,
            llm_model: None,
            combine: false,
            session_name: None,
        });

        assert!(matches!(result, Err(error) if error.contains("either all current Inbox audio")));
    }

    #[test]
    fn process_outputs_are_validated_and_deduplicated() {
        let artifacts = artifacts_from_ids(&["summary".into(), "summary".into(), "recap".into()])
            .expect("known outputs should be accepted");
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts_from_ids(&["unknown".into()]).is_err());
    }

    #[test]
    fn log_rebuild_requires_a_campaign() {
        assert!(log_rebuild_request(" ").is_err());
    }

    #[test]
    fn notes_request_requires_a_campaign_stem_and_outputs() {
        let request = |campaign_id: &str, stem: &str, artifact_ids: Vec<String>| NotesRequest {
            campaign_id: campaign_id.into(),
            stem: stem.into(),
            artifact_ids,
            resume: true,
            force: false,
            candidate: false,
        };

        assert!(notes_request(request("", "session-1", vec!["summary".into()])).is_err());
        assert!(notes_request(request("test", "../session-1", vec!["summary".into()])).is_err());
        assert!(notes_request(request("test", "session-1", Vec::new())).is_err());
    }

    #[test]
    fn transcribe_request_requires_a_campaign_and_inbox_audio() {
        assert!(super::transcribe_request(TranscribeRequest {
            campaign_id: String::new(),
            source_paths: vec!["/tmp/session.wav".into()],
            all_inbox: false,
            force: false,
            asr_model: None,
            language: None,
            session_date: None,
            diarize: false,
            vad: false,
            combine: false,
            session_name: None,
        })
        .is_err());
        assert!(super::transcribe_request(TranscribeRequest {
            campaign_id: "test".into(),
            source_paths: Vec::new(),
            all_inbox: false,
            force: false,
            asr_model: None,
            language: None,
            session_date: None,
            diarize: false,
            vad: false,
            combine: false,
            session_name: None,
        })
        .is_err());
    }

    #[test]
    fn model_request_rejects_unknown_and_wrong_catalog_models() {
        assert!(super::model_request(ModelRequest {
            action: ModelAction::DownloadWhisper,
            model_id: "../../not-a-model".into(),
        })
        .is_err());
        assert!(super::model_request(ModelRequest {
            action: ModelAction::PrepareAsr,
            model_id: "large-v3-turbo".into(),
        })
        .is_err());
        assert!(super::model_request(ModelRequest {
            action: ModelAction::PullOllama,
            model_id: "../../not-a-model".into(),
        })
        .is_err());
        assert!(super::model_request(ModelRequest {
            action: ModelAction::PullOllama,
            model_id: "qwen3.5:9b".into(),
        })
        .is_ok());
    }

    #[test]
    fn processing_overrides_accept_only_bounded_values() {
        assert_eq!(validate_asr_model("large-v3-turbo").as_deref(), Ok("large-v3-turbo"));
        assert!(validate_asr_model("../../arbitrary").is_err());
        assert_eq!(validate_language(Some("PT-BR".into())).as_deref(), Ok("pt-br"));
        assert!(validate_language(Some("english".into())).is_err());
        assert_eq!(
            validate_session_date(Some("2026-08-23".into())),
            Ok(Some("2026-08-23".into()))
        );
        assert!(validate_session_date(Some("2026-13-40".into())).is_err());
        assert_eq!(validate_backend_kind("OpenAI").as_deref(), Ok("openai"));
        assert!(validate_backend_kind("shell").is_err());
    }

    #[test]
    fn combined_session_stems_cannot_escape_or_hide_files() {
        assert!(super::is_valid_new_session_stem("session-12_combined"));
        assert!(!super::is_valid_new_session_stem("../outside"));
        assert!(!super::is_valid_new_session_stem(".hidden"));
        assert!(!super::is_valid_new_session_stem("session\n12"));
    }

    #[test]
    fn speaker_mapping_request_requires_campaign_stem_and_entries() {
        assert!(super::speaker_map_request(SpeakerMapRequest {
            campaign_id: String::new(),
            stem: "session-1".into(),
            mappings: vec![SpeakerMapEntry {
                label: "SPEAKER_00".into(),
                name: "Avery".into(),
            }],
        })
        .is_err());
        assert!(super::speaker_map_request(SpeakerMapRequest {
            campaign_id: "test".into(),
            stem: "../session-1".into(),
            mappings: Vec::new(),
        })
        .is_err());
    }

    #[test]
    fn speaker_mapping_entries_are_bound_to_detected_labels_and_safe_names() {
        let labels = std::collections::BTreeSet::from(["SPEAKER_00".to_string()]);
        let entry = |label: &str, name: &str| SpeakerMapEntry {
            label: label.into(),
            name: name.into(),
        };

        assert_eq!(
            super::validate_speaker_mappings(vec![entry("SPEAKER_00", "Avery")], &labels)
                .unwrap()
                .get("SPEAKER_00")
                .map(String::as_str),
            Some("Avery")
        );
        assert!(super::validate_speaker_mappings(vec![entry("SPEAKER_01", "Avery")], &labels).is_err());
        assert!(super::validate_speaker_mappings(
            vec![entry("SPEAKER_00", "Avery"), entry("SPEAKER_00", "Morgan")],
            &labels,
        )
        .is_err());
        assert!(super::validate_speaker_mappings(vec![entry("SPEAKER_00", "Avery\n")], &labels).is_err());
    }

    #[test]
    fn candidate_resolution_rejects_unbounded_identifiers_before_file_access() {
        let request = |campaign_id: &str, stem: &str, artifact_id: &str| CandidateResolveRequest {
            campaign_id: campaign_id.into(),
            stem: stem.into(),
            artifact_id: artifact_id.into(),
            action: CandidateAction::KeepCandidate,
        };

        assert!(super::candidate_resolve_request(request("", "session-1", "summary")).is_err());
        assert!(super::candidate_resolve_request(request("test", "../session-1", "summary")).is_err());
        assert!(super::candidate_resolve_request(request("test", "session-1", "../../outside")).is_err());
        assert_eq!(super::candidate_action_label(CandidateAction::KeepBoth), "Keep both");
    }
}