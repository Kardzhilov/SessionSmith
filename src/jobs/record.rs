//! Managed live-recording jobs.

use std::sync::Arc;

use crate::{
    commands::record::{self, RecordingRequest},
    jobs::{
        manager::{JobId, JobKind, JobManager},
        report::Reporter,
    },
};

/// Start a cancellable ffmpeg recording. Cancellation terminates the owned
/// process group, and the blocking worker waits for ffmpeg to exit so it can
/// finalize the WAV container before the job reaches a terminal state.
pub fn spawn(
    manager: &JobManager,
    reporter: Arc<dyn Reporter>,
    request: RecordingRequest,
) -> JobId {
    let title = format!("Record {}", request.name);
    manager.submit(
        JobKind::Record,
        title,
        reporter,
        move |context| async move {
            let reporter = context.reporter();
            let children = context.children();
            crate::ui::with_reporter(reporter, async move {
                let output = crate::ui::spawn_blocking_with_reporter(move || {
                    record::record_to_file_with_children(&request, Some(&children))
                })
                .await??;
                Ok(format!("saved {}", output.display()))
            })
            .await
        },
    )
}
