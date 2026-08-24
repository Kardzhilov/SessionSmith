//! Cancellable lifecycle management for host-owned background jobs.

use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime},
};

use anyhow::Result;
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

use super::{
    procs::ChildRegistry,
    report::{JobEvent, Reporter},
};

pub type JobId = u64;

/// Give cooperative work a short opportunity to reap cancelled child
/// processes before its host reports the job as terminal. A stuck network or
/// blocking operation is still detached after this grace period.
const CANCELLATION_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobKind {
    Import,
    Run,
    Transcribe,
    Notes,
    Doctor,
    RebuildLog,
    Reindex,
    CandidateResolve,
    SpeakerMap,
    Model,
    Export,
    Record,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct JobSnapshot {
    pub id: JobId,
    pub kind: JobKind,
    pub title: String,
    pub state: JobState,
    pub started_at: Option<SystemTime>,
    pub finished_at: Option<SystemTime>,
    pub summary: Option<String>,
    pub active_children: usize,
}

struct JobRecord {
    kind: JobKind,
    title: String,
    state: JobState,
    cancellation: CancellationToken,
    children: ChildRegistry,
    reporter: Arc<dyn Reporter>,
    started_at: Option<SystemTime>,
    finished_at: Option<SystemTime>,
    summary: Option<String>,
}

struct Inner {
    handle: Handle,
    next_id: AtomicU64,
    jobs: Mutex<BTreeMap<JobId, JobRecord>>,
}

/// Owns job identities, lifecycle snapshots, and cooperative cancellation.
#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

/// The per-job dependencies passed to work submitted through [`JobManager`].
#[derive(Clone)]
pub struct JobContext {
    reporter: Arc<dyn Reporter>,
    cancellation: CancellationToken,
    children: ChildRegistry,
}

impl JobContext {
    pub fn reporter(&self) -> Arc<dyn Reporter> {
        self.reporter.clone()
    }

    pub fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn children(&self) -> ChildRegistry {
        self.children.clone()
    }
}

impl JobManager {
    pub fn new(handle: Handle) -> Self {
        Self {
            inner: Arc::new(Inner {
                handle,
                next_id: AtomicU64::new(1),
                jobs: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Construct a manager on the Tokio runtime currently executing a host
    /// command. Hosts can use this without depending directly on Tokio.
    pub fn try_current() -> std::result::Result<Self, tokio::runtime::TryCurrentError> {
        Handle::try_current().map(Self::new)
    }

    /// Start work on the manager's runtime with this job's context.
    pub fn submit<Work, JobFuture>(
        &self,
        kind: JobKind,
        title: impl Into<String>,
        reporter: Arc<dyn Reporter>,
        work: Work,
    ) -> JobId
    where
        Work: FnOnce(JobContext) -> JobFuture + Send + 'static,
        JobFuture: Future<Output = Result<String>> + Send + 'static,
    {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let cancellation = CancellationToken::new();
        let children = ChildRegistry::default();
        let reporter: Arc<dyn Reporter> = Arc::new(ManagedReporter {
            delegate: reporter,
            cancellation: cancellation.clone(),
        });
        let title = title.into();

        self.with_jobs_mut(|jobs| {
            jobs.insert(
                id,
                JobRecord {
                    kind,
                    title,
                    state: JobState::Queued,
                    cancellation: cancellation.clone(),
                    children: children.clone(),
                    reporter: reporter.clone(),
                    started_at: None,
                    finished_at: None,
                    summary: None,
                },
            );
        });

        let inner = self.inner.clone();
        let context = JobContext {
            reporter: reporter.clone(),
            cancellation: cancellation.clone(),
            children,
        };
        self.inner.handle.spawn(async move {
            if cancellation.is_cancelled() {
                finish(&inner, id, JobState::Cancelled, "cancelled".into());
                reporter.event(JobEvent::JobDone(Err("cancelled".into())));
                return;
            }

            set_running(&inner, id);
            let work = work(context);
            tokio::pin!(work);
            let outcome = tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    if tokio::time::timeout(CANCELLATION_CLEANUP_TIMEOUT, &mut work)
                        .await
                        .is_err()
                    {
                        reporter.event(JobEvent::Warn(
                            "Cancellation cleanup timed out; background work may still be stopping."
                                .into(),
                        ));
                    }
                    Err(anyhow::anyhow!("cancelled"))
                }
                result = &mut work => result,
            };
            let (state, event) = match outcome {
                Ok(summary) => (JobState::Succeeded, Ok(summary)),
                Err(_error) if cancellation.is_cancelled() => {
                    (JobState::Cancelled, Err("cancelled".into()))
                }
                Err(error) => (JobState::Failed, Err(format!("{error:#}"))),
            };
            let summary = match &event {
                Ok(value) | Err(value) => value.clone(),
            };

            finish(&inner, id, state, summary);
            reporter.event(JobEvent::JobDone(event));
        });

        id
    }

    /// Request cooperative cancellation. Returns false for an unknown or
    /// already-finished job.
    pub fn cancel(&self, id: JobId) -> bool {
        let job = self.with_jobs_mut(|jobs| {
            let record = jobs.get_mut(&id)?;
            if matches!(
                record.state,
                JobState::Succeeded | JobState::Failed | JobState::Cancelled | JobState::Cancelling
            ) {
                return None;
            }
            record.state = JobState::Cancelling;
            record.cancellation.cancel();
            Some((record.reporter.clone(), record.children.clone()))
        });

        if let Some((reporter, children)) = job {
            let child_count = children.kill_all();
            let message = if child_count == 0 {
                "Cancellation requested".into()
            } else {
                format!("Cancellation requested; terminating {child_count} child process(es)")
            };
            reporter.event(JobEvent::Info(message));
            true
        } else {
            false
        }
    }

    /// Request cancellation for every job that has not reached a terminal state.
    pub fn cancel_all(&self) -> usize {
        let ids = self.with_jobs(|jobs| {
            jobs.iter()
                .filter_map(|(&id, record)| {
                    (!matches!(
                        record.state,
                        JobState::Succeeded | JobState::Failed | JobState::Cancelled
                    ))
                    .then_some(id)
                })
                .collect::<Vec<_>>()
        });
        ids.into_iter().filter(|&id| self.cancel(id)).count()
    }

    pub fn list(&self) -> Vec<JobSnapshot> {
        self.with_jobs(|jobs| {
            jobs.iter()
                .map(|(&id, record)| snapshot(id, record))
                .collect()
        })
    }

    fn with_jobs<T>(&self, read: impl FnOnce(&BTreeMap<JobId, JobRecord>) -> T) -> T {
        let jobs = self.inner.jobs.lock().expect("job manager mutex poisoned");
        read(&jobs)
    }

    fn with_jobs_mut<T>(&self, write: impl FnOnce(&mut BTreeMap<JobId, JobRecord>) -> T) -> T {
        let mut jobs = self.inner.jobs.lock().expect("job manager mutex poisoned");
        write(&mut jobs)
    }
}

struct ManagedReporter {
    delegate: Arc<dyn Reporter>,
    cancellation: CancellationToken,
}

impl Reporter for ManagedReporter {
    fn event(&self, event: JobEvent) {
        self.delegate.event(event);
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled() || self.delegate.is_cancelled()
    }
}

fn set_running(inner: &Inner, id: JobId) {
    let mut jobs = inner.jobs.lock().expect("job manager mutex poisoned");
    if let Some(record) = jobs.get_mut(&id) {
        if record.state == JobState::Queued {
            record.state = JobState::Running;
            record.started_at = Some(SystemTime::now());
        }
    }
}

fn finish(inner: &Inner, id: JobId, state: JobState, summary: String) {
    let mut jobs = inner.jobs.lock().expect("job manager mutex poisoned");
    if let Some(record) = jobs.get_mut(&id) {
        record.state = state;
        record.finished_at = Some(SystemTime::now());
        record.summary = Some(summary);
    }
}

fn snapshot(id: JobId, record: &JobRecord) -> JobSnapshot {
    JobSnapshot {
        id,
        kind: record.kind.clone(),
        title: record.title.clone(),
        state: record.state.clone(),
        started_at: record.started_at,
        finished_at: record.finished_at,
        summary: record.summary.clone(),
        active_children: record.children.active_children().len(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::{mpsc, oneshot};
    use tokio_util::sync::CancellationToken;

    use super::{JobKind, JobManager, JobState};
    use crate::jobs::report::{ChannelReporter, JobEvent};

    #[tokio::test]
    async fn manager_records_a_successful_job() {
        let manager = JobManager::new(tokio::runtime::Handle::current());
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let reporter = Arc::new(ChannelReporter::new(sender, CancellationToken::new()));
        let (release, gate) = oneshot::channel();
        let id = manager.submit(
            JobKind::Notes,
            "Generate notes",
            reporter,
            move |_| async move {
                gate.await.expect("test gate should be released");
                Ok("notes ready".into())
            },
        );

        release.send(()).expect("job should still be waiting");
        assert_eq!(
            next_completion(&mut receiver).await,
            Ok("notes ready".into())
        );
        let job = manager.list().pop().expect("job snapshot should exist");
        assert_eq!(job.id, id);
        assert_eq!(job.state, JobState::Succeeded);
        assert_eq!(job.summary.as_deref(), Some("notes ready"));
    }

    #[tokio::test]
    async fn manager_cancels_cooperative_work() {
        let manager = JobManager::new(tokio::runtime::Handle::current());
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let reporter = Arc::new(ChannelReporter::new(sender, CancellationToken::new()));
        let id = manager.submit(
            JobKind::Transcribe,
            "Transcribe session",
            reporter,
            move |context| async move {
                context.cancellation().cancelled().await;
                Err(anyhow::anyhow!("cancelled"))
            },
        );

        assert!(manager.cancel(id));
        assert!(!manager.cancel(id));
        assert_eq!(
            next_completion(&mut receiver).await,
            Err("cancelled".into())
        );
        let job = manager.list().pop().expect("job snapshot should exist");
        assert_eq!(job.state, JobState::Cancelled);
        assert_eq!(job.summary.as_deref(), Some("cancelled"));
    }

    #[tokio::test]
    async fn manager_cancels_all_live_jobs() {
        let manager = JobManager::new(tokio::runtime::Handle::current());
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let reporter = Arc::new(ChannelReporter::new(sender, CancellationToken::new()));
        manager.submit(
            JobKind::Run,
            "Run one",
            reporter.clone(),
            |context| async move {
                context.cancellation().cancelled().await;
                Err(anyhow::anyhow!("cancelled"))
            },
        );
        manager.submit(JobKind::Run, "Run two", reporter, |context| async move {
            context.cancellation().cancelled().await;
            Err(anyhow::anyhow!("cancelled"))
        });

        assert_eq!(manager.cancel_all(), 2);
        assert_eq!(
            next_completion(&mut receiver).await,
            Err("cancelled".into())
        );
        assert_eq!(
            next_completion(&mut receiver).await,
            Err("cancelled".into())
        );
        assert!(manager
            .list()
            .iter()
            .all(|job| job.state == JobState::Cancelled));
    }

    #[tokio::test]
    async fn manager_waits_for_cooperative_cancellation_cleanup() {
        let manager = JobManager::new(tokio::runtime::Handle::current());
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let reporter = Arc::new(ChannelReporter::new(sender, CancellationToken::new()));
        let (work_started, work_started_rx) = oneshot::channel();
        let (cleanup_started, cleanup_started_rx) = oneshot::channel();
        let (release_cleanup, release_cleanup_rx) = oneshot::channel();
        let id = manager.submit(
            JobKind::Record,
            "Record session",
            reporter,
            move |context| async move {
                work_started
                    .send(())
                    .expect("test should observe work start");
                context.cancellation().cancelled().await;
                cleanup_started
                    .send(())
                    .expect("test should observe cleanup start");
                release_cleanup_rx
                    .await
                    .expect("test should release cleanup");
                Ok("recording finalized".into())
            },
        );

        work_started_rx
            .await
            .expect("job should begin work before cancellation");
        assert!(manager.cancel(id));
        cleanup_started_rx
            .await
            .expect("job should begin cleanup after cancellation");
        let job = manager.list().pop().expect("job snapshot should exist");
        assert_eq!(job.state, JobState::Cancelling);

        release_cleanup
            .send(())
            .expect("job should still be waiting for cleanup");
        assert_eq!(
            next_completion(&mut receiver).await,
            Err("cancelled".into())
        );
        let job = manager.list().pop().expect("job snapshot should exist");
        assert_eq!(job.state, JobState::Cancelled);
    }

    async fn next_completion(
        receiver: &mut mpsc::UnboundedReceiver<JobEvent>,
    ) -> Result<String, String> {
        while let Some(event) = receiver.recv().await {
            if let JobEvent::JobDone(result) = event {
                return result;
            }
        }
        panic!("reporter channel closed before job completion");
    }
}
