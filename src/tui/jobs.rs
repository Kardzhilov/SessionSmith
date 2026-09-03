//! TUI adapters for shared background-job orchestration.

use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::config::GlobalConfig;
use crate::jobs::manager::{JobId, JobManager};
use crate::jobs::report::{ChannelReporter, Reporter};
use crate::ui::UiEvent;

pub use crate::jobs::orchestrate::{
    spawn_model_with_reporter, spawn_with_reporter, JobKind, JobRequest, ModelJob,
};

/// Spawn a pipeline job with progress delivered through the TUI event channel.
pub fn spawn(manager: &JobManager, tx: UnboundedSender<UiEvent>, req: JobRequest) -> JobId {
    let reporter: Arc<dyn Reporter> = Arc::new(ChannelReporter::new(tx, CancellationToken::new()));
    spawn_with_reporter(manager, reporter, req)
}

/// Spawn a model-management job with progress delivered through the TUI event
/// channel. Returns its ID only when the operation is safe to cancel.
pub fn spawn_model(
    manager: &JobManager,
    tx: UnboundedSender<UiEvent>,
    g: GlobalConfig,
    job: ModelJob,
    title: impl Into<String>,
) -> Option<JobId> {
    let cancellation_safe = job.supports_cancellation();
    let reporter = Arc::new(ChannelReporter::new(tx, CancellationToken::new()));
    let job_id = spawn_model_with_reporter(manager, reporter, g, job, title);
    cancellation_safe.then_some(job_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::manager::JobState;
    use crate::jobs::report::JobEvent;

    #[tokio::test]
    async fn cancellable_model_job_uses_shared_manager_lifecycle() {
        let manager = JobManager::new(tokio::runtime::Handle::current());
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let id = spawn_model(
            &manager,
            sender,
            GlobalConfig::default(),
            ModelJob::DeleteAsr("missing-model".into()),
            "Delete missing ASR model",
        )
        .expect("local model deletion should be cancellable");

        let completion = loop {
            match receiver.recv().await {
                Some(JobEvent::JobDone(result)) => break result,
                Some(_) => {}
                None => panic!("job reporter closed before completion"),
            }
        };
        assert!(completion
            .expect_err("unknown model should fail")
            .contains("unknown ASR model 'missing-model'"));
        let job = manager
            .list()
            .into_iter()
            .find(|job| job.id == id)
            .expect("manager should retain model job");
        assert_eq!(job.state, JobState::Failed);
    }
}
