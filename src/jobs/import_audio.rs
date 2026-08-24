//! Managed copy-only audio imports for host applications.

use anyhow::{bail, Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    audio,
    jobs::{
        manager::{JobId, JobKind, JobManager},
        report::Reporter,
    },
};

const COPY_BUFFER_SIZE: usize = 256 * 1024;

/// A host-validated, copy-only request to add source audio to an Inbox.
#[derive(Debug, Clone)]
pub struct AudioImportRequest {
    pub inbox: PathBuf,
    pub sources: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ImportOutcome {
    imported: usize,
    skipped: usize,
}

/// Submit a cancellable copy job. Source paths are read only by the Rust host;
/// destination paths are always derived from the configured Inbox.
pub fn spawn(
    manager: &JobManager,
    reporter: Arc<dyn Reporter>,
    request: AudioImportRequest,
) -> JobId {
    let title = format!("Import {} audio file(s)", request.sources.len());
    manager.submit(
        JobKind::Import,
        title,
        reporter,
        move |context| async move {
            let reporter = context.reporter();
            crate::ui::with_reporter(reporter, async move {
                let outcome =
                    crate::ui::spawn_blocking_with_reporter(move || import_audio(&request))
                        .await??;
                Ok(if outcome.skipped == 0 {
                    format!("imported {} audio file(s)", outcome.imported)
                } else {
                    format!(
                        "imported {} audio file(s); skipped {}",
                        outcome.imported, outcome.skipped
                    )
                })
            })
            .await
        },
    )
}

fn import_audio(request: &AudioImportRequest) -> Result<ImportOutcome> {
    if request.sources.is_empty() {
        bail!("no audio files selected");
    }
    ensure_not_cancelled()?;
    fs::create_dir_all(&request.inbox)
        .with_context(|| format!("creating audio inbox {}", request.inbox.display()))?;

    crate::ui::header("Import audio");
    let mut outcome = ImportOutcome {
        imported: 0,
        skipped: 0,
    };
    let total = request.sources.len();
    for (index, source) in request.sources.iter().enumerate() {
        ensure_not_cancelled()?;
        crate::ui::step(index + 1, total, &source.display().to_string());
        match import_one(source, &request.inbox) {
            Ok(true) => outcome.imported += 1,
            Ok(false) => outcome.skipped += 1,
            Err(error) if crate::ui::cancellation_requested() => return Err(error),
            Err(error) => {
                outcome.skipped += 1;
                crate::ui::warn(&format!("could not import {}: {error:#}", source.display()));
            }
        }
    }

    if outcome.imported == 0 {
        bail!("no audio files were imported");
    }
    Ok(outcome)
}

/// Returns true when the source was copied, false when it was safely skipped.
fn import_one(source: &Path, inbox: &Path) -> Result<bool> {
    if !source.is_file() {
        crate::ui::warn(&format!(
            "skipping missing or non-file source: {}",
            source.display()
        ));
        return Ok(false);
    }
    if !audio::is_supported_audio_path(source) {
        crate::ui::warn(&format!(
            "skipping unsupported audio type: {}",
            source.display()
        ));
        return Ok(false);
    }

    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow::anyhow!("source filename is not valid UTF-8"))?;
    let destination = inbox.join(name);
    if destination.exists() {
        crate::ui::warn(&format!("skipping existing Inbox file: {name}"));
        return Ok(false);
    }

    let part = inbox.join(format!(".{name}.import.part"));
    if part.exists() {
        crate::ui::warn(&format!("skipping incomplete previous import: {name}"));
        return Ok(false);
    }

    let total = source
        .metadata()
        .with_context(|| format!("reading metadata for {}", source.display()))?
        .len();
    let mut input = File::open(source).with_context(|| format!("opening {}", source.display()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .with_context(|| format!("creating {}", part.display()))?;
    let label = format!("importing {name}");
    let copy_result = copy_with_cancellation(&mut input, &mut output, total, &label);
    drop(output);

    if let Err(error) = copy_result {
        fs::remove_file(&part).ok();
        return Err(error);
    }
    ensure_not_cancelled()?;
    if let Err(error) = publish_without_overwrite(&part, &destination) {
        fs::remove_file(&part).ok();
        return Err(error);
    }
    crate::ui::ok(&format!("imported {name}"));
    Ok(true)
}

fn publish_without_overwrite(part: &Path, destination: &Path) -> Result<()> {
    // Both paths are in the Inbox, so a hard link atomically refuses a target
    // created after the earlier collision check without crossing filesystems.
    fs::hard_link(part, destination)
        .with_context(|| format!("publishing {}", destination.display()))?;
    fs::remove_file(part).with_context(|| format!("removing {}", part.display()))?;
    Ok(())
}

fn copy_with_cancellation(
    input: &mut File,
    output: &mut File,
    total: u64,
    label: &str,
) -> Result<()> {
    let mut buffer = vec![0; COPY_BUFFER_SIZE];
    let mut copied = 0u64;
    loop {
        ensure_not_cancelled()?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        copied += read as u64;
        crate::ui::progress(label, copied, total);
    }
    output.flush()?;
    ensure_not_cancelled()?;
    Ok(())
}

fn ensure_not_cancelled() -> Result<()> {
    if crate::ui::cancellation_requested() {
        bail!("cancelled");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::report::ChannelReporter;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn copies_supported_audio_into_the_inbox() {
        let source_dir = tempfile::tempdir().expect("source directory should exist");
        let inbox_dir = tempfile::tempdir().expect("inbox directory should exist");
        let source = source_dir.path().join("session.wav");
        fs::write(&source, b"audio bytes").expect("source should be written");

        let outcome = import_audio(&AudioImportRequest {
            inbox: inbox_dir.path().to_path_buf(),
            sources: vec![source],
        })
        .expect("import should succeed");

        assert_eq!(outcome.imported, 1);
        assert_eq!(outcome.skipped, 0);
        assert_eq!(
            fs::read(inbox_dir.path().join("session.wav")).expect("destination should exist"),
            b"audio bytes"
        );
    }

    #[test]
    fn keeps_existing_inbox_audio_without_overwriting_it() {
        let source_dir = tempfile::tempdir().expect("source directory should exist");
        let inbox_dir = tempfile::tempdir().expect("inbox directory should exist");
        let source = source_dir.path().join("session.wav");
        fs::write(&source, b"new audio").expect("source should be written");
        let destination = inbox_dir.path().join("session.wav");
        fs::write(&destination, b"existing audio").expect("destination should be written");

        let error = import_audio(&AudioImportRequest {
            inbox: inbox_dir.path().to_path_buf(),
            sources: vec![source],
        })
        .expect_err("collision-only import should report no imported files");

        assert!(error.to_string().contains("no audio files"));
        assert_eq!(
            fs::read(destination).expect("destination should remain"),
            b"existing audio"
        );
    }

    #[test]
    fn imports_valid_files_when_a_batch_contains_invalid_ones() {
        let source_dir = tempfile::tempdir().expect("source directory should exist");
        let inbox_dir = tempfile::tempdir().expect("inbox directory should exist");
        let valid_source = source_dir.path().join("session.wav");
        let unsupported_source = source_dir.path().join("readme.txt");
        fs::write(&valid_source, b"audio bytes").expect("source should be written");
        fs::write(&unsupported_source, b"not audio").expect("source should be written");

        let outcome = import_audio(&AudioImportRequest {
            inbox: inbox_dir.path().to_path_buf(),
            sources: vec![
                valid_source,
                unsupported_source,
                source_dir.path().join("missing.mp3"),
            ],
        })
        .expect("the valid file should still import");

        assert_eq!(outcome.imported, 1);
        assert_eq!(outcome.skipped, 2);
        assert!(inbox_dir.path().join("session.wav").is_file());
    }

    #[test]
    fn publication_refuses_a_destination_created_after_copying() {
        let inbox_dir = tempfile::tempdir().expect("inbox directory should exist");
        let part = inbox_dir.path().join(".session.wav.import.part");
        let destination = inbox_dir.path().join("session.wav");
        fs::write(&part, b"new audio").expect("part should be written");
        fs::write(&destination, b"existing audio").expect("destination should be written");

        assert!(publish_without_overwrite(&part, &destination).is_err());
        assert_eq!(
            fs::read(&destination).expect("destination should remain"),
            b"existing audio"
        );
    }

    #[tokio::test]
    async fn cancelled_import_never_publishes_a_destination_file() {
        let source_dir = tempfile::tempdir().expect("source directory should exist");
        let inbox_dir = tempfile::tempdir().expect("inbox directory should exist");
        let source = source_dir.path().join("session.wav");
        fs::write(&source, b"audio bytes").expect("source should be written");
        let (sender, _receiver) = mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let reporter = Arc::new(ChannelReporter::new(sender, cancellation));

        crate::ui::with_reporter(reporter, async {
            let error = import_audio(&AudioImportRequest {
                inbox: inbox_dir.path().to_path_buf(),
                sources: vec![source],
            })
            .expect_err("cancelled import should fail");
            assert_eq!(error.to_string(), "cancelled");
        })
        .await;

        assert!(!inbox_dir.path().join("session.wav").exists());
    }
}
