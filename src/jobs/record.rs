//! Managed live-recording jobs.

use std::{path::PathBuf, process::Command, sync::Arc};

use anyhow::{bail, Context, Result};

use crate::{
    deps,
    jobs::{
        manager::{JobId, JobKind, JobManager},
        procs::{configure_command, ChildRegistry},
        report::Reporter,
    },
    ui,
};

/// A non-interactive recording request suitable for a managed host job.
#[derive(Debug, Clone)]
pub struct RecordingRequest {
    /// Output stem, without a file extension.
    pub name: String,
    /// ffmpeg input device, or the platform default when omitted.
    pub device: Option<String>,
    /// ffmpeg input format, or the platform default when omitted.
    pub format: Option<String>,
}

impl RecordingRequest {
    /// Validate a prompt-free request before a host submits a recording job.
    pub fn validate(&self) -> Result<()> {
        recording_output_path(&self.name).map(|_| ())
    }
}

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
                    record_to_file_with_children(&request, Some(&children))
                })
                .await??;
                Ok(format!("saved {}", output.display()))
            })
            .await
        },
    )
}

/// Capture audio into a durable WAV file without terminal prompts. The caller
/// may supply a managed child registry so cancellation terminates ffmpeg and
/// waits for it to exit before the job completes.
pub fn record_to_file_with_children(
    request: &RecordingRequest,
    children: Option<&ChildRegistry>,
) -> Result<PathBuf> {
    ui::header("SessionSmith · record");
    deps::ensure_dirs()?;

    request.validate()?;
    let out = recording_output_path(&request.name)?;
    if out.exists() {
        bail!("{} already exists — choose another name", out.display());
    }

    let (default_fmt, default_device) = default_capture();
    let fmt = request
        .format
        .clone()
        .unwrap_or_else(|| default_fmt.to_string());
    let device = request
        .device
        .clone()
        .unwrap_or_else(|| default_device.to_string());

    ui::panel(
        "Recording",
        &[
            format!("Output : {}", out.display()),
            format!("Format : {fmt}"),
            format!("Device : {device}"),
            "Press q then Enter (or Ctrl-C) to stop.".to_string(),
        ],
    );

    // ffmpeg captures until the user quits; stdin is inherited so `q` works.
    let mut command = Command::new(capture_ffmpeg(&fmt));
    command
        .args(["-y", "-f", &fmt, "-i", &device, "-ar", "16000", "-ac", "1"])
        .arg(&out);
    let status = run_command_status(&mut command, children, "ffmpeg recording")
        .with_context(|| "ffmpeg not found — install ffmpeg")?;

    // ffmpeg exits non-zero when interrupted but still finalises the file.
    if !out.exists() {
        bail!("recording failed (ffmpeg exit {status}); check the device/format flags");
    }
    ui::ok(&format!("saved {}", out.display()));
    ui::info("Run `sessionsmith` to transcribe and generate notes from it.");
    Ok(out)
}

fn capture_ffmpeg(format: &str) -> PathBuf {
    if cfg!(target_os = "linux") {
        if let Some(path) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&path) {
                let candidate = directory.join("ffmpeg");
                if candidate.is_file() && ffmpeg_supports_capture_format(&candidate, format) {
                    return candidate;
                }
            }
        }
    }
    crate::util::find_in_path("ffmpeg").unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn ffmpeg_supports_capture_format(ffmpeg: &std::path::Path, format: &str) -> bool {
    Command::new(ffmpeg)
        .args(["-hide_banner", "-devices"])
        .output()
        .ok()
        .is_some_and(|output| ffmpeg_device_listing_supports(&output.stdout, format))
}

fn ffmpeg_device_listing_supports(listing: &[u8], format: &str) -> bool {
    String::from_utf8_lossy(listing).lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next().is_some_and(|flags| flags.contains('D'))
            && fields.next() == Some(format)
    })
}

fn recording_output_path(name: &str) -> Result<PathBuf> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        bail!("recording name must not be empty");
    }
    if PathBuf::from(trimmed)
        .file_name()
        .and_then(|value| value.to_str())
        != Some(trimmed)
    {
        bail!("recording name must not contain a path");
    }
    Ok(crate::config::audio_dir().join(format!("{trimmed}.wav")))
}

fn run_command_status(
    command: &mut Command,
    children: Option<&ChildRegistry>,
    label: &str,
) -> std::io::Result<std::process::ExitStatus> {
    let Some(children) = children else {
        return command.status();
    };

    configure_command(command);
    let mut child = command.spawn()?;
    let registration = children.register(&child, label);
    let status = child.wait();
    drop(registration);
    status
}

/// Default ffmpeg capture format and device for the current OS.
fn default_capture() -> (&'static str, &'static str) {
    if cfg!(target_os = "macos") {
        ("avfoundation", ":0") // default audio input device
    } else if cfg!(target_os = "windows") {
        ("dshow", "audio=Microphone")
    } else {
        ("pulse", "default")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_format_requires_an_ffmpeg_input_device() {
        let listing = " D  lavfi           Libavfilter virtual input\n DE alsa            ALSA audio output\n DE pulse           Pulse audio output\n  E sdl2            SDL2 output device\n";

        assert!(ffmpeg_device_listing_supports(listing.as_bytes(), "pulse"));
        assert!(ffmpeg_device_listing_supports(listing.as_bytes(), "alsa"));
        assert!(!ffmpeg_device_listing_supports(
            listing.as_bytes(),
            "sdl2"
        ));
    }

    #[test]
    fn recording_name_stays_within_the_audio_directory() {
        assert!(recording_output_path("session-12").is_ok());
        assert!(recording_output_path("").is_err());
        assert!(recording_output_path("../outside").is_err());
        assert!(recording_output_path("nested/session").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn registry_aware_recording_command_can_be_cancelled() {
        use std::time::Duration;

        let registry = ChildRegistry::default();
        let worker_registry = registry.clone();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 2"]);
            run_command_status(&mut command, Some(&worker_registry), "test recording")
                .expect("test process should run")
        });

        let started = (0..100).any(|_| {
            if !registry.active_children().is_empty() {
                true
            } else {
                std::thread::sleep(Duration::from_millis(10));
                false
            }
        });
        if !started {
            let _ = worker.join();
            panic!("test recording should be registered");
        }

        assert_eq!(registry.kill_all(), 1);
        let status = worker.join().expect("test worker should complete");
        assert!(!status.success());
        assert!(registry.active_children().is_empty());
    }
}
