//! In-process transcription via `whisper-rs` (whisper.cpp linked directly).
//!
//! Only compiled when the `local-whisper` feature is enabled. GPU backends
//! (CUDA / Vulkan / Metal) are selected at build time via the matching Cargo
//! features and used automatically by whisper.cpp when available.

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Once;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::jobs::procs::{configure_command, ChildRegistry};

/// Route whisper.cpp / GGML logs through the `tracing` framework so they obey
/// the app's log filter (hidden at the default `warn` level) instead of
/// spamming stdout. Runs once.
fn init_logging() {
    static LOG_INIT: Once = Once::new();
    LOG_INIT.call_once(|| {
        whisper_rs::install_logging_hooks();
    });
}

/// One transcribed segment with start/end timestamps in centiseconds.
pub struct LocalSegment {
    pub start_cs: i64,
    pub end_cs: i64,
    pub text: String,
}

/// Human-readable label for the compute backend this build links.
pub fn gpu_label() -> &'static str {
    if cfg!(feature = "cuda") {
        "cuda (whisper-rs)"
    } else if cfg!(feature = "vulkan") {
        "vulkan (whisper-rs)"
    } else if cfg!(feature = "metal") {
        "metal (whisper-rs)"
    } else {
        "cpu (whisper-rs)"
    }
}

/// Transcribe an audio file with a local ggml model, returning timestamped
/// segments. `language` may be `"auto"`. `use_gpu` allows forcing CPU even on
/// a GPU-enabled build.
pub fn transcribe_file(
    model_path: &Path,
    audio: &Path,
    language: &str,
    threads: i32,
    use_gpu: bool,
    initial_prompt: Option<&str>,
) -> Result<Vec<LocalSegment>> {
    transcribe_file_with_children(
        model_path,
        audio,
        language,
        threads,
        use_gpu,
        initial_prompt,
        None,
    )
}

/// Transcribe while associating the ffmpeg decoder with a host-owned job and
/// polling that job for cancellation during local Whisper inference.
pub fn transcribe_file_with_children(
    model_path: &Path,
    audio: &Path,
    language: &str,
    threads: i32,
    use_gpu: bool,
    initial_prompt: Option<&str>,
    children: Option<&ChildRegistry>,
) -> Result<Vec<LocalSegment>> {
    init_logging();
    ensure_not_cancelled()?;
    let samples = decode_audio(audio, children)?;
    ensure_not_cancelled()?;

    let mut cparams = WhisperContextParameters::default();
    cparams.use_gpu(use_gpu);
    let ctx = WhisperContext::new_with_params(
        model_path
            .to_str()
            .ok_or_else(|| anyhow!("model path is not valid UTF-8"))?,
        cparams,
    )
    .map_err(|e| anyhow!("loading whisper model {}: {e}", model_path.display()))?;

    let mut state = ctx
        .create_state()
        .map_err(|e| anyhow!("creating whisper state: {e}"))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(threads.max(1));
    if language != "auto" {
        params.set_language(Some(language));
    }
    if let Some(prompt) = initial_prompt.filter(|prompt| !prompt.is_empty()) {
        params.set_initial_prompt(prompt);
    }
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_abort_callback_safe(transcription_cancelled);

    state.full(params, &samples).map_err(|error| {
        if transcription_cancelled() {
            anyhow!("cancelled")
        } else {
            anyhow!("whisper transcription failed: {error}")
        }
    })?;
    ensure_not_cancelled()?;

    let n = state
        .full_n_segments()
        .map_err(|e| anyhow!("reading segment count: {e}"))?;
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let text = state.full_get_segment_text(i).unwrap_or_default();
        let start_cs = state.full_get_segment_t0(i).unwrap_or(0);
        let end_cs = state.full_get_segment_t1(i).unwrap_or(0);
        out.push(LocalSegment {
            start_cs,
            end_cs,
            text,
        });
    }
    Ok(out)
}

/// Decode any audio file to 16 kHz mono `f32` PCM via ffmpeg (which whisper.cpp
/// requires). Reads the raw stream directly rather than a temp file.
fn decode_audio(audio: &Path, children: Option<&ChildRegistry>) -> Result<Vec<f32>> {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-v", "error", "-i"])
        .arg(audio)
        .args(["-ar", "16000", "-ac", "1", "-f", "f32le", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = run_command_output(&mut command, children, "ffmpeg local whisper decode")
        .with_context(|| "ffmpeg not found — install ffmpeg")?;
    if !out.status.success() {
        bail!(
            "ffmpeg could not decode {}: {}",
            audio.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let samples: Vec<f32> = out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    if samples.is_empty() {
        bail!("decoded no audio samples from {}", audio.display());
    }
    Ok(samples)
}

fn transcription_cancelled() -> bool {
    crate::ui::cancellation_requested()
}

fn ensure_not_cancelled() -> Result<()> {
    if transcription_cancelled() {
        bail!("cancelled");
    }
    Ok(())
}

fn run_command_output(
    command: &mut Command,
    children: Option<&ChildRegistry>,
    label: &str,
) -> std::io::Result<std::process::Output> {
    let Some(children) = children else {
        return command.output();
    };

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_command(command);
    let child = command.spawn()?;
    let registration = children.register(&child, label);
    let output = child.wait_with_output();
    drop(registration);
    output
}

/// Render segments to plain transcript text (one line per segment).
pub fn segments_to_text(segments: &[LocalSegment]) -> String {
    let mut s = String::new();
    for seg in segments {
        let t = seg.text.trim();
        if !t.is_empty() {
            s.push_str(t);
            s.push('\n');
        }
    }
    s
}

/// Render segments to SubRip (.srt) subtitle format.
pub fn segments_to_srt(segments: &[LocalSegment]) -> String {
    let mut s = String::new();
    let mut idx = 1;
    for seg in segments {
        let t = seg.text.trim();
        if t.is_empty() {
            continue;
        }
        s.push_str(&idx.to_string());
        s.push('\n');
        s.push_str(&format!(
            "{} --> {}\n",
            srt_time(seg.start_cs),
            srt_time(seg.end_cs)
        ));
        s.push_str(t);
        s.push_str("\n\n");
        idx += 1;
    }
    s
}

/// Format centiseconds as an SRT timestamp `HH:MM:SS,mmm`.
fn srt_time(cs: i64) -> String {
    let ms = cs.max(0) * 10;
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let sec = (ms % 60_000) / 1000;
    let milli = ms % 1000;
    format!("{h:02}:{m:02}:{sec:02},{milli:03}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::jobs::report::ChannelReporter;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn srt_time_formats() {
        assert_eq!(srt_time(0), "00:00:00,000");
        assert_eq!(srt_time(150), "00:00:01,500");
        assert_eq!(srt_time(360_050), "01:00:00,500");
    }

    #[test]
    fn renders_segments() {
        let segs = vec![
            LocalSegment {
                start_cs: 0,
                end_cs: 100,
                text: " Hello ".into(),
            },
            LocalSegment {
                start_cs: 100,
                end_cs: 200,
                text: "".into(),
            },
            LocalSegment {
                start_cs: 200,
                end_cs: 300,
                text: "world".into(),
            },
        ];
        assert_eq!(segments_to_text(&segs), "Hello\nworld\n");
        let srt = segments_to_srt(&segs);
        assert!(srt.contains("1\n00:00:00,000 --> 00:00:01,000\nHello"));
        assert!(srt.contains("2\n00:00:02,000 --> 00:00:03,000\nworld"));
    }

    #[tokio::test]
    async fn abort_callback_observes_structured_job_cancellation() {
        let (sender, _receiver) = mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();
        let reporter = Arc::new(ChannelReporter::new(sender, cancellation.clone()));

        crate::ui::with_reporter(reporter, async {
            assert!(!transcription_cancelled());
            cancellation.cancel();
            assert!(transcription_cancelled());
            assert!(ensure_not_cancelled().is_err());
        })
        .await;
    }

    #[cfg(unix)]
    #[test]
    fn registry_aware_decoder_command_reaps_and_deregisters() {
        let registry = ChildRegistry::default();
        let mut command = Command::new("sh");
        command.args(["-c", "printf samples"]);

        let output = run_command_output(&mut command, Some(&registry), "test decoder")
            .expect("command should run");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"samples");
        assert!(registry.active_children().is_empty());
    }
}
