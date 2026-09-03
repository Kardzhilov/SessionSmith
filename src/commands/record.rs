//! CLI wrapper for managed live audio capture.

use anyhow::Result;
use inquire::Text;

use crate::cli::RecordArgs;
use crate::jobs::{
    procs::ChildRegistry,
    record::{record_to_file_with_children, RecordingRequest},
};

pub async fn run(args: RecordArgs) -> Result<()> {
    run_with_children(args, None).await
}

/// Run a recording while associating ffmpeg with a host-owned job. Existing
/// CLI callers use [`run`] without a registry.
pub async fn run_with_children(args: RecordArgs, children: Option<&ChildRegistry>) -> Result<()> {
    let name = match args.name {
        Some(n) => n,
        None => Text::new("Recording name:")
            .with_default("session")
            .prompt()?,
    };
    record_to_file_with_children(
        &RecordingRequest {
            name,
            device: args.device,
            format: args.format,
        },
        children,
    )?;
    Ok(())
}
