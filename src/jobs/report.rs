//! Progress reporting and cooperative cancellation for background jobs.

use std::path::PathBuf;

use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

/// Structured events that hosts can render while a job is running.
#[derive(Clone, Debug, PartialEq)]
pub enum JobEvent {
    Header(String),
    Step {
        n: usize,
        total: usize,
        msg: String,
    },
    Ok(String),
    Warn(String),
    Error(String),
    Info(String),
    /// A determinate or indeterminate progress update. A `total` of zero
    /// denotes an unknown total.
    Progress {
        label: String,
        pos: u64,
        total: u64,
        rate: Option<f64>,
    },
    /// A background job completed with a human-readable summary or error.
    JobDone(Result<String, String>),
    /// A high-level pipeline phase started.
    Phase(String),
    /// A generated artifact reached the filesystem and can be re-read.
    ArtifactWritten {
        session: String,
        artifact: String,
        path: PathBuf,
    },
}

/// Host-independent progress reporting and cooperative cancellation.
pub trait Reporter: Send + Sync {
    fn event(&self, event: JobEvent);
    fn is_cancelled(&self) -> bool;
}

/// A reporter that forwards structured events through a Tokio channel.
#[derive(Clone)]
pub struct ChannelReporter {
    sender: UnboundedSender<JobEvent>,
    cancellation: CancellationToken,
}

impl ChannelReporter {
    pub fn new(sender: UnboundedSender<JobEvent>, cancellation: CancellationToken) -> Self {
        Self {
            sender,
            cancellation,
        }
    }
}

impl Reporter for ChannelReporter {
    fn event(&self, event: JobEvent) {
        let _ = self.sender.send(event);
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

/// A reporter for callers that do not need progress events.
#[derive(Default)]
pub struct NullReporter;

impl Reporter for NullReporter {
    fn event(&self, _: JobEvent) {}

    fn is_cancelled(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{ChannelReporter, JobEvent, Reporter};
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn channel_reporter_forwards_events_and_observes_cancellation() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();
        let reporter = ChannelReporter::new(sender, cancellation.clone());

        reporter.event(JobEvent::Phase("Transcribe".into()));
        assert_eq!(
            receiver.recv().await,
            Some(JobEvent::Phase("Transcribe".into()))
        );
        assert!(!reporter.is_cancelled());

        cancellation.cancel();
        assert!(reporter.is_cancelled());
    }
}
