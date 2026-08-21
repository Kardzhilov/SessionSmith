//! In-process transcription via `whisper-rs` (whisper.cpp linked directly).
//!
//! Only compiled when the `local-whisper` feature is enabled. GPU backends
//! (CUDA / Vulkan / Metal) are selected at build time via the matching Cargo
//! features and used automatically by whisper.cpp when available.

use anyhow::{anyhow, bail, Context, Result};
use std::path::Path;
use std::process::Command;
use std::sync::Once;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

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
    init_logging();
    let samples = decode_audio(audio)?;

    let mut cparams = WhisperContextParameters::default();
    cparams.use_gpu(use_gpu);
    let ctx = WhisperContext::new_with_params(
        model_path.to_str().ok_or_else(|| anyhow!("model path is not valid UTF-8"))?,
        cparams,
    )
    .map_err(|e| anyhow!("loading whisper model {}: {e}", model_path.display()))?;

    let mut state = ctx.create_state().map_err(|e| anyhow!("creating whisper state: {e}"))?;

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

    state
        .full(params, &samples)
        .map_err(|e| anyhow!("whisper transcription failed: {e}"))?;

    let n = state
        .full_n_segments()
        .map_err(|e| anyhow!("reading segment count: {e}"))?;
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let text = state.full_get_segment_text(i).unwrap_or_default();
        let start_cs = state.full_get_segment_t0(i).unwrap_or(0);
        let end_cs = state.full_get_segment_t1(i).unwrap_or(0);
        out.push(LocalSegment { start_cs, end_cs, text });
    }
    Ok(out)
}

/// Decode any audio file to 16 kHz mono `f32` PCM via ffmpeg (which whisper.cpp
/// requires). Reads the raw stream directly rather than a temp file.
fn decode_audio(audio: &Path) -> Result<Vec<f32>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(audio)
        .args(["-ar", "16000", "-ac", "1", "-f", "f32le", "-"])
        .output()
        .with_context(|| "ffmpeg not found — install ffmpeg")?;
    if !out.status.success() {
        bail!("ffmpeg could not decode {}: {}", audio.display(), String::from_utf8_lossy(&out.stderr));
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
        s.push_str(&format!("{} --> {}\n", srt_time(seg.start_cs), srt_time(seg.end_cs)));
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

    #[test]
    fn srt_time_formats() {
        assert_eq!(srt_time(0), "00:00:00,000");
        assert_eq!(srt_time(150), "00:00:01,500");
        assert_eq!(srt_time(360_050), "01:00:00,500");
    }

    #[test]
    fn renders_segments() {
        let segs = vec![
            LocalSegment { start_cs: 0, end_cs: 100, text: " Hello ".into() },
            LocalSegment { start_cs: 100, end_cs: 200, text: "".into() },
            LocalSegment { start_cs: 200, end_cs: 300, text: "world".into() },
        ];
        assert_eq!(segments_to_text(&segs), "Hello\nworld\n");
        let srt = segments_to_srt(&segs);
        assert!(srt.contains("1\n00:00:00,000 --> 00:00:01,000\nHello"));
        assert!(srt.contains("2\n00:00:02,000 --> 00:00:03,000\nworld"));
    }
}
