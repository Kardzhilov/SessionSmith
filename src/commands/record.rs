//! Live audio capture into `audio/` via ffmpeg. Uses the platform's native
//! capture backend (PulseAudio/ALSA on Linux, avfoundation on macOS, dshow on
//! Windows). No extra Rust dependencies — consistent with the existing ffmpeg
//! usage elsewhere in the pipeline.

use anyhow::{bail, Context, Result};
use inquire::Text;
use std::process::Command;

use crate::cli::RecordArgs;
use crate::{deps, ui};

pub async fn run(args: RecordArgs) -> Result<()> {
    ui::header("SessionSmith · record");
    deps::ensure_dirs()?;

    let name = match args.name {
        Some(n) => n,
        None => Text::new("Recording name:").with_default("session").prompt()?,
    };
    let out = crate::config::audio_dir().join(format!("{name}.wav"));
    if out.exists() {
        bail!("{} already exists — choose another name", out.display());
    }

    let (default_fmt, default_device) = default_capture();
    let fmt = args.format.unwrap_or_else(|| default_fmt.to_string());
    let device = args.device.unwrap_or_else(|| default_device.to_string());

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
    let status = Command::new("ffmpeg")
        .args(["-y", "-f", &fmt, "-i", &device, "-ar", "16000", "-ac", "1"])
        .arg(&out)
        .status()
        .with_context(|| "ffmpeg not found — install ffmpeg")?;

    // ffmpeg exits non-zero when interrupted but still finalises the file.
    if !out.exists() {
        bail!("recording failed (ffmpeg exit {status}); check the device/format flags");
    }
    ui::ok(&format!("saved {}", out.display()));
    ui::info("Run `sessionsmith` to transcribe and generate notes from it.");
    Ok(())
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
