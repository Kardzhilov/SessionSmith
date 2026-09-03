use crate::jobs::{DesktopJobs, WatchCampaignContext, WatchJobState};
use serde::{Deserialize, Serialize};
use sessionsmith::{audio, watch::StableFileDetector};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::AppHandle;
use tauri_specta::Event as _;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

const DEFAULT_INTERVAL_SECS: u64 = 5;
const MIN_INTERVAL_SECS: u64 = 2;
const MAX_INTERVAL_SECS: u64 = 30;
const QUEUE_CAPACITY: usize = 16;

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InboxWatchStartRequest {
    pub campaign_id: String,
    #[specta(type = Option<specta_typescript::Number>)]
    pub interval_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "inbox-watch://status")]
pub struct InboxWatchStatus {
    pub running: bool,
    pub campaign_id: Option<String>,
    pub campaign_name: Option<String>,
    #[specta(type = Option<specta_typescript::Number>)]
    pub interval_secs: Option<u64>,
    #[specta(type = specta_typescript::Number)]
    pub queued: usize,
    pub processing_path: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub overflow_count: usize,
    pub last_error: Option<String>,
}

#[derive(Clone, Default)]
pub struct InboxWatchService {
    inner: Arc<Mutex<WatchState>>,
}

#[derive(Default)]
struct WatchState {
    next_generation: u64,
    active: Option<ActiveWatch>,
    rename_in_progress: bool,
}

struct ActiveWatch {
    generation: u64,
    status: InboxWatchStatus,
    queue: VecDeque<PathBuf>,
    submitted: Option<SubmittedWatchJob>,
    cancel: CancellationToken,
    done: Option<oneshot::Receiver<()>>,
}

#[derive(Debug, Clone)]
struct SubmittedWatchJob {
    id: u64,
    path: PathBuf,
}

pub(crate) struct CampaignRenameWatchGuard {
    inner: Arc<Mutex<WatchState>>,
}

impl Drop for CampaignRenameWatchGuard {
    fn drop(&mut self) {
        lock_state(&self.inner).rename_in_progress = false;
    }
}

impl WatchState {
    fn begin(
        &mut self,
        campaign_id: String,
        context: WatchCampaignContext,
        interval_secs: u64,
        cancel: CancellationToken,
        done: oneshot::Receiver<()>,
    ) -> Result<u64, String> {
        if self.rename_in_progress {
            return Err(
                "Wait for the campaign rename to finish before starting Inbox watch.".into(),
            );
        }
        if let Some(active) = &self.active {
            return Err(format!(
                "Inbox watch is already active for '{}'. Stop it before starting another watcher.",
                active
                    .status
                    .campaign_name
                    .as_deref()
                    .unwrap_or("a campaign")
            ));
        }
        self.next_generation += 1;
        let generation = self.next_generation;
        self.active = Some(ActiveWatch {
            generation,
            status: InboxWatchStatus {
                running: true,
                campaign_id: Some(campaign_id),
                campaign_name: Some(context.campaign_name.clone()),
                interval_secs: Some(interval_secs),
                ..InboxWatchStatus::default()
            },
            queue: VecDeque::new(),
            submitted: None,
            cancel,
            done: Some(done),
        });
        Ok(generation)
    }

    fn stop(&mut self) -> Option<ActiveWatch> {
        let active = self.active.take()?;
        active.cancel.cancel();
        Some(active)
    }

    fn status(&self) -> InboxWatchStatus {
        self.active
            .as_ref()
            .map(|active| active.status.clone())
            .unwrap_or_default()
    }
}

impl InboxWatchService {
    pub fn start(
        &self,
        app: AppHandle,
        jobs: DesktopJobs,
        request: InboxWatchStartRequest,
    ) -> Result<InboxWatchStatus, String> {
        let campaign_id = request.campaign_id.trim();
        if campaign_id.is_empty() {
            return Err("Select a campaign before starting Inbox watch.".into());
        }
        let interval_secs = validate_interval(request.interval_secs)?;
        let context = jobs.watch_campaign_context(campaign_id)?;
        let cancel = CancellationToken::new();
        let (done_tx, done_rx) = oneshot::channel();
        let generation = {
            let mut state = lock_state(&self.inner);
            state.begin(
                campaign_id.to_string(),
                context.clone(),
                interval_secs,
                cancel.clone(),
                done_rx,
            )?
        };

        let service = self.clone();
        let task_app = app.clone();
        let task_campaign_id = campaign_id.to_string();
        tauri::async_runtime::spawn(async move {
            service
                .run_loop(
                    task_app,
                    jobs,
                    generation,
                    task_campaign_id,
                    context,
                    interval_secs,
                    cancel,
                )
                .await;
            let _ = done_tx.send(());
        });

        let status = self.status();
        emit_status(&app, &status);
        Ok(status)
    }

    pub async fn stop(&self, app: &AppHandle) -> InboxWatchStatus {
        let active = lock_state(&self.inner).stop();
        if let Some(mut active) = active {
            if let Some(done) = active.done.take() {
                let _ = done.await;
            }
        }
        let status = self.status();
        emit_status(app, &status);
        status
    }

    pub fn stop_on_exit(&self) {
        let _ = lock_state(&self.inner).stop();
    }

    pub fn status(&self) -> InboxWatchStatus {
        lock_state(&self.inner).status()
    }

    pub(crate) fn begin_campaign_rename(&self) -> Result<CampaignRenameWatchGuard, String> {
        let mut state = lock_state(&self.inner);
        if state.active.is_some() {
            return Err("Stop Inbox watch before renaming a campaign.".into());
        }
        if state.rename_in_progress {
            return Err("Another campaign rename is already in progress.".into());
        }
        state.rename_in_progress = true;
        Ok(CampaignRenameWatchGuard {
            inner: self.inner.clone(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_loop(
        &self,
        app: AppHandle,
        jobs: DesktopJobs,
        generation: u64,
        campaign_id: String,
        context: WatchCampaignContext,
        interval_secs: u64,
        cancel: CancellationToken,
    ) {
        let mut detector = StableFileDetector::default();
        loop {
            if cancel.is_cancelled()
                || !self
                    .tick(
                        &app,
                        &jobs,
                        generation,
                        &campaign_id,
                        &context,
                        &mut detector,
                    )
                    .await
            {
                return;
            }
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(interval_secs)) => {}
            }
        }
    }

    async fn tick(
        &self,
        app: &AppHandle,
        jobs: &DesktopJobs,
        generation: u64,
        campaign_id: &str,
        context: &WatchCampaignContext,
        detector: &mut StableFileDetector,
    ) -> bool {
        let submitted = {
            let state = lock_state(&self.inner);
            let Some(active) = state
                .active
                .as_ref()
                .filter(|active| active.generation == generation)
            else {
                return false;
            };
            active.submitted.clone()
        };
        let mut changed = false;
        if let Some(submitted) = submitted {
            let job_state = jobs.watch_job_state(submitted.id);
            let handled = {
                let mut state = lock_state(&self.inner);
                let Some(active) = state
                    .active
                    .as_mut()
                    .filter(|active| active.generation == generation)
                else {
                    return false;
                };
                reconcile_submitted_job(active, submitted.id, job_state)
            };
            if let Some(path) = handled {
                detector.mark_handled(path);
            }
            changed = true;
        }

        let audio_dir = context.audio_dir.clone();
        let transcripts_dir = context.transcripts_dir.clone();
        let scan = tauri::async_runtime::spawn_blocking(move || {
            audio::scan(&audio_dir, &transcripts_dir).map_err(|error| error.to_string())
        })
        .await;
        let files = match scan {
            Ok(Ok(files)) => files,
            Ok(Err(error)) => {
                self.set_error(app, generation, error);
                return true;
            }
            Err(error) => {
                self.set_error(app, generation, format!("Inbox scan task failed: {error}"));
                return true;
            }
        };

        let ready = detector.observe(&files);
        {
            let mut state = lock_state(&self.inner);
            let Some(active) = state
                .active
                .as_mut()
                .filter(|active| active.generation == generation)
            else {
                return false;
            };
            for path in ready {
                changed |= enqueue_path(active, path);
            }
            active.status.queued = active.queue.len();
        }

        let pipeline_busy = jobs.pipeline_busy();
        let next = {
            let state = lock_state(&self.inner);
            let Some(active) = state
                .active
                .as_ref()
                .filter(|active| active.generation == generation)
            else {
                return false;
            };
            next_queued_path(active, pipeline_busy || active.submitted.is_some())
        };

        if pipeline_busy {
            if changed {
                self.emit_current(app);
            }
            return true;
        }

        let Some(path) = next else {
            if changed {
                self.emit_current(app);
            }
            return true;
        };
        match jobs.submit_watch_process(app.clone(), campaign_id.to_string(), path.clone()) {
            Ok(submission) => {
                let mut state = lock_state(&self.inner);
                let Some(active) = state
                    .active
                    .as_mut()
                    .filter(|active| active.generation == generation)
                else {
                    return false;
                };
                active.queue.pop_front();
                active.status.queued = active.queue.len();
                active.status.processing_path = Some(path.to_string_lossy().into_owned());
                active.submitted = Some(SubmittedWatchJob {
                    id: submission.id,
                    path,
                });
                active.status.last_error = None;
                drop(state);
                self.emit_current(app);
            }
            Err(_error) if jobs.pipeline_busy() => {
                if changed {
                    self.emit_current(app);
                }
            }
            Err(error) => {
                let mut state = lock_state(&self.inner);
                let Some(active) = state
                    .active
                    .as_mut()
                    .filter(|active| active.generation == generation)
                else {
                    return false;
                };
                record_submission_error(active, &path, error);
                drop(state);
                self.emit_current(app);
            }
        }
        true
    }

    fn set_error(&self, app: &AppHandle, generation: u64, error: String) {
        let mut state = lock_state(&self.inner);
        let Some(active) = state
            .active
            .as_mut()
            .filter(|active| active.generation == generation)
        else {
            return;
        };
        active.status.last_error = Some(error);
        drop(state);
        self.emit_current(app);
    }

    fn emit_current(&self, app: &AppHandle) {
        emit_status(app, &self.status());
    }
}

fn validate_interval(interval_secs: Option<u64>) -> Result<u64, String> {
    let interval_secs = interval_secs.unwrap_or(DEFAULT_INTERVAL_SECS);
    if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&interval_secs) {
        return Err(format!(
            "Inbox watch interval must be between {MIN_INTERVAL_SECS} and {MAX_INTERVAL_SECS} seconds."
        ));
    }
    Ok(interval_secs)
}

fn enqueue_path(active: &mut ActiveWatch, path: PathBuf) -> bool {
    if active.queue.contains(&path)
        || active
            .submitted
            .as_ref()
            .is_some_and(|submitted| submitted.path == path)
    {
        return false;
    }
    if active.queue.len() == QUEUE_CAPACITY {
        active.status.overflow_count += 1;
        active.status.last_error = Some(format!(
            "Inbox watch queue is full; deferred {} until capacity is available.",
            display_name(&path)
        ));
    } else {
        active.queue.push_back(path);
    }
    active.status.queued = active.queue.len();
    true
}

fn reconcile_submitted_job(
    active: &mut ActiveWatch,
    job_id: u64,
    job_state: Option<WatchJobState>,
) -> Option<PathBuf> {
    let submitted = active
        .submitted
        .as_ref()
        .filter(|submitted| submitted.id == job_id)
        .cloned()?;
    match job_state {
        Some(WatchJobState::Active) => None,
        Some(WatchJobState::Succeeded) => {
            active.submitted = None;
            active.status.processing_path = None;
            active.status.last_error = None;
            Some(submitted.path)
        }
        Some(WatchJobState::Failed(error)) => {
            retry_submitted_path(active, &submitted.path);
            active.submitted = None;
            active.status.processing_path = None;
            active.status.last_error = Some(format!(
                "Processing {} failed and will retry: {error}",
                display_name(&submitted.path)
            ));
            None
        }
        Some(WatchJobState::Cancelled) => {
            retry_submitted_path(active, &submitted.path);
            active.submitted = None;
            active.status.processing_path = None;
            active.status.last_error = Some(format!(
                "Processing {} was cancelled and will retry.",
                display_name(&submitted.path)
            ));
            None
        }
        None => {
            retry_submitted_path(active, &submitted.path);
            active.submitted = None;
            active.status.processing_path = None;
            active.status.last_error = Some(format!(
                "Processing job for {} disappeared and will retry.",
                display_name(&submitted.path)
            ));
            None
        }
    }
}

fn retry_submitted_path(active: &mut ActiveWatch, path: &PathBuf) {
    if active.queue.contains(path) {
        return;
    }
    if active.queue.len() == QUEUE_CAPACITY {
        active.queue.pop_back();
        active.status.overflow_count += 1;
    }
    active.queue.push_front(path.clone());
    active.status.queued = active.queue.len();
}

fn record_submission_error(active: &mut ActiveWatch, path: &std::path::Path, error: String) {
    active.status.last_error = Some(format!("Could not submit {}: {error}", display_name(path)));
}

fn next_queued_path(active: &ActiveWatch, pipeline_busy: bool) -> Option<PathBuf> {
    (!pipeline_busy)
        .then(|| active.queue.front().cloned())
        .flatten()
}

fn lock_state(state: &Mutex<WatchState>) -> std::sync::MutexGuard<'_, WatchState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn emit_status(app: &AppHandle, status: &InboxWatchStatus) {
    let _ = status.emit(app);
}

fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(name: &str) -> WatchCampaignContext {
        WatchCampaignContext {
            campaign_name: name.into(),
            audio_dir: PathBuf::from("audio"),
            transcripts_dir: PathBuf::from("transcripts"),
        }
    }

    fn begin(state: &mut WatchState, campaign_id: &str) -> Result<u64, String> {
        let (_done_tx, done_rx) = oneshot::channel();
        state.begin(
            campaign_id.into(),
            context(campaign_id),
            DEFAULT_INTERVAL_SECS,
            CancellationToken::new(),
            done_rx,
        )
    }

    #[test]
    fn interval_is_bounded() {
        assert_eq!(validate_interval(None).unwrap(), DEFAULT_INTERVAL_SECS);
        assert!(validate_interval(Some(MIN_INTERVAL_SECS - 1)).is_err());
        assert!(validate_interval(Some(MAX_INTERVAL_SECS + 1)).is_err());
    }

    #[test]
    fn singleton_rejects_same_or_different_campaign_until_stopped() {
        let mut state = WatchState::default();
        begin(&mut state, "alpha").unwrap();
        assert!(begin(&mut state, "alpha").is_err());
        assert!(begin(&mut state, "beta").is_err());

        let stopped = state.stop().unwrap();
        assert!(stopped.cancel.is_cancelled());
        assert!(begin(&mut state, "beta").is_ok());
    }

    #[test]
    fn queue_keeps_oldest_items_and_counts_dropped_newest() {
        let mut state = WatchState::default();
        let generation = begin(&mut state, "alpha").unwrap();
        let active = state.active.as_mut().unwrap();
        for index in 0..QUEUE_CAPACITY {
            enqueue_path(active, PathBuf::from(format!("{index}.wav")));
        }
        enqueue_path(active, PathBuf::from("overflow.wav"));
        assert_eq!(active.generation, generation);
        assert_eq!(active.queue.len(), QUEUE_CAPACITY);
        assert_eq!(active.queue.front(), Some(&PathBuf::from("0.wav")));
        assert_eq!(active.queue.back(), Some(&PathBuf::from("15.wav")));
        assert_eq!(active.status.overflow_count, 1);
        assert!(active
            .status
            .last_error
            .as_deref()
            .unwrap()
            .contains("overflow.wav"));

        active.queue.pop_front();
        assert!(enqueue_path(active, PathBuf::from("overflow.wav")));
        assert!(active.queue.contains(&PathBuf::from("overflow.wav")));
    }

    #[test]
    fn queue_deduplicates_queued_and_submitted_paths() {
        let mut state = WatchState::default();
        begin(&mut state, "alpha").unwrap();
        let active = state.active.as_mut().unwrap();
        let path = PathBuf::from("session.wav");
        assert!(enqueue_path(active, path.clone()));
        assert!(!enqueue_path(active, path.clone()));
        active.queue.clear();
        active.submitted = Some(SubmittedWatchJob {
            id: 7,
            path: path.clone(),
        });
        assert!(!enqueue_path(active, path));
    }

    #[test]
    fn failed_and_cancelled_jobs_return_the_path_to_the_queue() {
        let mut state = WatchState::default();
        begin(&mut state, "alpha").unwrap();
        let active = state.active.as_mut().unwrap();
        let path = PathBuf::from("retry.wav");
        active.submitted = Some(SubmittedWatchJob {
            id: 7,
            path: path.clone(),
        });
        active.status.processing_path = Some(path.to_string_lossy().into_owned());

        assert!(reconcile_submitted_job(
            active,
            7,
            Some(WatchJobState::Failed("backend timeout".into()))
        )
        .is_none());
        assert_eq!(active.queue.front(), Some(&path));
        assert!(active.submitted.is_none());

        active.queue.clear();
        active.submitted = Some(SubmittedWatchJob {
            id: 8,
            path: path.clone(),
        });
        assert!(reconcile_submitted_job(active, 8, Some(WatchJobState::Cancelled)).is_none());
        assert_eq!(active.queue.front(), Some(&path));
    }

    #[test]
    fn successful_job_returns_the_path_for_permanent_handling() {
        let mut state = WatchState::default();
        begin(&mut state, "alpha").unwrap();
        let active = state.active.as_mut().unwrap();
        let path = PathBuf::from("handled.wav");
        active.submitted = Some(SubmittedWatchJob {
            id: 7,
            path: path.clone(),
        });

        assert_eq!(
            reconcile_submitted_job(active, 7, Some(WatchJobState::Succeeded)),
            Some(path)
        );
        assert!(active.submitted.is_none());
    }

    #[test]
    fn manual_pipeline_collision_retains_the_next_file() {
        let mut state = WatchState::default();
        begin(&mut state, "alpha").unwrap();
        let active = state.active.as_mut().unwrap();
        enqueue_path(active, PathBuf::from("manual-collision.wav"));

        assert!(next_queued_path(active, true).is_none());
        assert_eq!(
            active.queue.front(),
            Some(&PathBuf::from("manual-collision.wav"))
        );
        assert_eq!(
            next_queued_path(active, false),
            active.queue.front().cloned()
        );
    }

    #[test]
    fn submission_failure_retains_the_queue_head() {
        let mut state = WatchState::default();
        begin(&mut state, "alpha").unwrap();
        let active = state.active.as_mut().unwrap();
        let path = PathBuf::from("submit-failure.wav");
        enqueue_path(active, path.clone());

        record_submission_error(active, &path, "manager unavailable".into());

        assert_eq!(active.queue.front(), Some(&path));
        assert_eq!(active.status.queued, 1);
        assert!(active
            .status
            .last_error
            .as_deref()
            .unwrap()
            .contains("manager unavailable"));
    }

    #[test]
    fn stop_cancels_polling_and_discards_queued_work() {
        let mut state = WatchState::default();
        begin(&mut state, "alpha").unwrap();
        state
            .active
            .as_mut()
            .unwrap()
            .queue
            .push_back(PathBuf::from("queued.wav"));

        let stopped = state.stop().unwrap();
        assert!(stopped.cancel.is_cancelled());
        assert!(state.active.is_none());
        assert!(!state.status().running);
    }

    #[test]
    fn campaign_rename_and_watch_start_are_mutually_exclusive() {
        let service = InboxWatchService::default();
        {
            let mut state = lock_state(&service.inner);
            begin(&mut state, "alpha").unwrap();
        }
        assert!(service.begin_campaign_rename().is_err());
        lock_state(&service.inner).stop();

        let guard = service.begin_campaign_rename().unwrap();
        {
            let mut state = lock_state(&service.inner);
            assert!(begin(&mut state, "alpha").is_err());
        }
        drop(guard);
        let mut state = lock_state(&service.inner);
        assert!(begin(&mut state, "alpha").is_ok());
    }
}
