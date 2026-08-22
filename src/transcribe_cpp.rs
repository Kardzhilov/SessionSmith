//! Local transcribe.cpp GGUF runner.
//!
//! Used for ASR model families that are available as GGUF files, such as
//! Cohere Transcribe 03-2026. SessionSmith fetches/builds the local
//! `transcribe-cli` runtime into the cache when needed, then runs it on a
//! normalized 16 kHz mono WAV.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const REPO_URL: &str = "https://github.com/handy-computer/transcribe.cpp.git";
const TARGET_CHUNK_SECONDS: f64 = 30.0;
const MIN_RETRY_CHUNK_SECONDS: f64 = 1.0;
const CHUNK_OVERLAP_SECONDS: f64 = 5.0;
const MAX_BOUNDARY_OVERLAP_WORDS: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Auto,
    Cpu,
    Cuda,
    Vulkan,
    Metal,
}

impl Backend {
    fn cli_name(self) -> &'static str {
        match self {
            Backend::Auto => "auto",
            Backend::Cpu => "cpu",
            Backend::Cuda => "cuda",
            Backend::Vulkan => "vulkan",
            Backend::Metal => "metal",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ChunkTranscript {
    text: String,
    start: f64,
    end: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Checkpoint {
    model: String,
    audio_sha1_first_mb: String,
    chunk_s: f64,
    overlap_s: f64,
    segments: Vec<ChunkTranscript>,
}

fn checkpoint_path(out_prefix: &Path) -> PathBuf {
    PathBuf::from(format!("{}.progress.json", out_prefix.display()))
}

fn first_mb_sha1(path: &Path) -> Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("reading {} for checkpoint", path.display()))?;
    let mut bytes = vec![0; 1_048_576];
    let read = file.read(&mut bytes)?;
    Ok(hex::encode(Sha1::digest(&bytes[..read])))
}

fn load_checkpoint(path: &Path, model: &str, audio_sha1: &str) -> Option<Vec<ChunkTranscript>> {
    let checkpoint: Checkpoint = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let matches = checkpoint.model == model
        && checkpoint.audio_sha1_first_mb == audio_sha1
        && checkpoint.chunk_s == TARGET_CHUNK_SECONDS
        && checkpoint.overlap_s == CHUNK_OVERLAP_SECONDS;
    matches.then_some(checkpoint.segments)
}

fn write_checkpoint(path: &Path, checkpoint: &Checkpoint) -> Result<()> {
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, serde_json::to_vec_pretty(checkpoint)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn runtime_root() -> Result<PathBuf> {
    let base = dirs::cache_dir().ok_or_else(|| anyhow!("could not resolve XDG cache dir"))?;
    Ok(base.join("sessionsmith").join("transcribe.cpp"))
}

fn cached_binary() -> Result<PathBuf> {
    Ok(runtime_root()?
        .join("build")
        .join("bin")
        .join("transcribe-cli"))
}

fn which(bin: &str) -> Option<PathBuf> {
    crate::util::find_in_path(bin)
}

fn command_ok(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn nvidia_gpu_present() -> bool {
    Command::new("nvidia-smi")
        .arg("-L")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn vulkan_available() -> bool {
    command_ok("glslc") && Path::new("/usr/lib/x86_64-linux-gnu/libvulkan.so.1").exists()
}

fn requested_backend(device: Option<&str>) -> Backend {
    match device.unwrap_or("auto").to_ascii_lowercase().as_str() {
        "cpu" => Backend::Cpu,
        "cuda" => Backend::Cuda,
        "vulkan" => Backend::Vulkan,
        "metal" => Backend::Metal,
        _ => Backend::Auto,
    }
}

fn build_backend_for(device: Option<&str>) -> Backend {
    match requested_backend(device) {
        Backend::Cpu => Backend::Cpu,
        Backend::Cuda => {
            if command_ok("nvcc") {
                Backend::Cuda
            } else {
                Backend::Cpu
            }
        }
        Backend::Vulkan => {
            if vulkan_available() {
                Backend::Vulkan
            } else {
                Backend::Cpu
            }
        }
        Backend::Metal => {
            if cfg!(target_os = "macos") {
                Backend::Metal
            } else {
                Backend::Cpu
            }
        }
        Backend::Auto => {
            if command_ok("nvcc") && nvidia_gpu_present() {
                Backend::Cuda
            } else if cfg!(target_os = "macos") && std::env::consts::ARCH == "aarch64" {
                Backend::Metal
            } else if vulkan_available() {
                Backend::Vulkan
            } else {
                Backend::Cpu
            }
        }
    }
}

fn runtime_devices(binary: &Path) -> Option<String> {
    let out = Command::new(binary)
        .arg("--list-devices")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn runtime_has_backend(binary: &Path, backend: Backend) -> bool {
    match backend {
        Backend::Auto | Backend::Cpu => true,
        Backend::Cuda | Backend::Vulkan | Backend::Metal => runtime_devices(binary)
            .map(|out| out.contains(&format!("kind={}", backend.cli_name())))
            .unwrap_or(false),
    }
}

fn runtime_backend(binary: &Path, device: Option<&str>) -> Backend {
    let requested = requested_backend(device);
    match requested {
        Backend::Auto => Backend::Auto,
        Backend::Cpu => Backend::Cpu,
        Backend::Cuda | Backend::Vulkan | Backend::Metal => {
            if runtime_has_backend(binary, requested) {
                requested
            } else {
                crate::ui::warn(&format!(
                    "transcribe.cpp backend '{}' is not available in this build; falling back to auto",
                    requested.cli_name()
                ));
                Backend::Auto
            }
        }
    }
}

pub fn find_runtime() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("SESSIONSMITH_TRANSCRIBE_CPP") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(p) = cached_binary() {
        if p.exists() {
            return Some(p);
        }
    }
    which("transcribe-cli")
}

pub fn ensure_runtime() -> Result<PathBuf> {
    ensure_runtime_for(None)
}

pub fn ensure_runtime_for(device: Option<&str>) -> Result<PathBuf> {
    let desired_backend = build_backend_for(device);
    if let Some(p) = find_runtime() {
        if runtime_has_backend(&p, desired_backend) {
            warn_if_gpu_unavailable(device, desired_backend);
            return Ok(p);
        }
    }

    let root = runtime_root()?;
    if !root.exists() {
        if let Some(parent) = root.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::ui::info("fetching transcribe.cpp runtime");
        let status = Command::new("git")
            .args(["clone", "--depth", "1", "--recursive", REPO_URL])
            .arg(&root)
            .status()
            .with_context(|| "git not found; install git to fetch transcribe.cpp")?;
        if !status.success() {
            bail!("git clone {REPO_URL} failed");
        }
    }

    crate::ui::info("building transcribe.cpp runtime");
    let mut configure = Command::new("cmake");
    configure.current_dir(&root).args([
        "-B",
        "build",
        "-DCMAKE_BUILD_TYPE=Release",
        "-DTRANSCRIBE_CUDA=OFF",
        "-DTRANSCRIBE_VULKAN=OFF",
    ]);
    match desired_backend {
        Backend::Cuda => {
            configure.arg("-DTRANSCRIBE_CUDA=ON");
            crate::ui::info("configuring transcribe.cpp with CUDA backend");
        }
        Backend::Vulkan => {
            configure.arg("-DTRANSCRIBE_VULKAN=ON");
            crate::ui::info("configuring transcribe.cpp with Vulkan backend");
        }
        Backend::Metal => {
            configure.arg("-DTRANSCRIBE_METAL=ON");
            crate::ui::info("configuring transcribe.cpp with Metal backend");
        }
        Backend::Auto | Backend::Cpu => {}
    }
    let configure = configure
        .status()
        .with_context(|| "cmake not found; install cmake to build transcribe.cpp")?;
    if !configure.success() {
        bail!("cmake configure for transcribe.cpp failed");
    }

    let jobs = std::thread::available_parallelism()
        .map(|n| n.get().to_string())
        .unwrap_or_else(|_| "2".to_string());
    let build = Command::new("cmake")
        .current_dir(&root)
        .args([
            "--build",
            "build",
            "--target",
            "transcribe-cli",
            "--parallel",
            &jobs,
        ])
        .status()
        .with_context(|| "building transcribe.cpp")?;
    if !build.success() {
        bail!("cmake build for transcribe.cpp failed");
    }

    let bin = cached_binary()?;
    if !bin.exists() {
        bail!(
            "transcribe.cpp build completed but {} is missing",
            bin.display()
        );
    }
    warn_if_gpu_unavailable(device, desired_backend);
    Ok(bin)
}

fn warn_if_gpu_unavailable(device: Option<&str>, built_backend: Backend) {
    if matches!(requested_backend(device), Backend::Cpu) {
        return;
    }
    if built_backend != Backend::Cpu {
        return;
    }
    if nvidia_gpu_present() && !command_ok("nvcc") {
        crate::ui::warn(
            "NVIDIA GPU detected, but CUDA toolkit/nvcc is not on PATH; transcribe.cpp is using CPU. In the model manager, press 'g' to install CUDA toolkit and rebuild with CUDA.",
        );
    } else if !command_ok("glslc") {
        crate::ui::warn(
            "transcribe.cpp GPU backend was not built; install CUDA toolkit or Vulkan shader tools (glslc), then prepare the model again.",
        );
    }
}

fn normalize_audio(audio: &Path) -> Result<PathBuf> {
    let stem = audio
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("audio");
    let mut out = std::env::temp_dir();
    out.push(format!(
        "ss_transcribe_cpp_{}_{}.wav",
        stem,
        std::process::id()
    ));
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(audio)
        .args(["-ar", "16000", "-ac", "1"])
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| "ffmpeg not found; install ffmpeg")?;
    if !status.success() || !out.exists() {
        bail!(
            "ffmpeg could not convert {} to 16 kHz mono WAV",
            audio.display()
        );
    }
    Ok(out)
}

fn extract_text(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("text: "))
        .map(str::trim)
        .filter(|text| *text != "(empty)")
        .unwrap_or_else(|| stdout.trim())
        .trim()
        .to_string()
}

fn normalized_word(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn common_subsequence_len(left: &[String], right: &[String]) -> usize {
    let mut previous = vec![0; right.len() + 1];
    for left_word in left {
        let mut current = vec![0; right.len() + 1];
        for (idx, right_word) in right.iter().enumerate() {
            current[idx + 1] = if left_word == right_word {
                previous[idx] + 1
            } else {
                current[idx].max(previous[idx + 1])
            };
        }
        previous = current;
    }
    previous[right.len()]
}

fn trim_repeated_prefix(previous: &str, current: &str) -> String {
    let previous_words: Vec<&str> = previous.split_whitespace().collect();
    let current_words: Vec<&str> = current.split_whitespace().collect();
    let max_overlap = previous_words
        .len()
        .min(current_words.len())
        .min(MAX_BOUNDARY_OVERLAP_WORDS);

    for overlap in (3..=max_overlap).rev() {
        let prev_start = previous_words.len() - overlap;
        let matches = previous_words[prev_start..]
            .iter()
            .zip(current_words[..overlap].iter())
            .all(|(left, right)| normalized_word(left) == normalized_word(right));
        if matches {
            return current_words[overlap..].join(" ");
        }
    }

    let previous_normalized: Vec<String> = previous_words
        .iter()
        .map(|word| normalized_word(word))
        .collect();
    let current_normalized: Vec<String> = current_words
        .iter()
        .map(|word| normalized_word(word))
        .collect();
    let mut best_match: Option<(usize, usize)> = None;
    for previous_count in 5..=max_overlap {
        for current_count in 5..=max_overlap {
            let previous_tail = &previous_normalized[previous_normalized.len() - previous_count..];
            let current_prefix = &current_normalized[..current_count];
            let common = common_subsequence_len(previous_tail, current_prefix);
            let similarity = common * 2 * 100 / (previous_count + current_count);
            if common >= 5 && similarity >= 80 {
                let candidate = (common, current_count);
                if best_match.is_none_or(|best| candidate > best) {
                    best_match = Some(candidate);
                }
            }
        }
    }
    if let Some((_, overlap)) = best_match {
        return current_words[overlap..].join(" ");
    }
    current.trim().to_string()
}

fn has_repetition_loop(text: &str) -> bool {
    let words: Vec<String> = text
        .split_whitespace()
        .map(normalized_word)
        .filter(|word| !word.is_empty())
        .collect();

    if words
        .windows(8)
        .any(|window| window.iter().all(|word| word == &window[0]))
    {
        return true;
    }

    let max_phrase_words = 20.min(words.len() / 3);
    for phrase_words in 3..=max_phrase_words {
        for start in 0..=words.len() - phrase_words * 3 {
            let phrase = &words[start..start + phrase_words];
            if phrase == &words[start + phrase_words..start + phrase_words * 2]
                && phrase == &words[start + phrase_words * 2..start + phrase_words * 3]
            {
                return true;
            }
        }
    }
    false
}

fn deduplicate_chunk_boundaries(chunks: &mut [ChunkTranscript]) {
    for idx in 1..chunks.len() {
        let previous = chunks[idx - 1].text.clone();
        chunks[idx].text = trim_repeated_prefix(&previous, &chunks[idx].text);
    }
}

fn fmt_ts(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let h = ms / 3_600_000;
    let rem = ms % 3_600_000;
    let m = rem / 60_000;
    let rem = rem % 60_000;
    let s = rem / 1_000;
    let ms = rem % 1_000;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

fn audio_duration_seconds(audio: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(audio)
        .output();
    out.ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn make_chunk(wav: &Path, chunk_path: &Path, start: f64, duration: f64) -> Result<()> {
    let status = Command::new("ffmpeg")
        .arg("-y")
        .args(["-ss", &format!("{start:.3}")])
        .arg("-i")
        .arg(wav)
        .args(["-t", &format!("{duration:.3}")])
        .args(["-ar", "16000", "-ac", "1"])
        .arg(chunk_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| "ffmpeg not found; install ffmpeg")?;
    if !status.success() || !chunk_path.exists() {
        bail!("ffmpeg could not split transcribe.cpp audio chunk");
    }
    Ok(())
}

fn should_split_error(err: &str) -> bool {
    let err = err.to_ascii_lowercase();
    err.contains("output truncated")
        || err.contains("input too long")
        || err.contains("exceed")
        || err.contains("max audio")
        || err.contains("repetition loop")
}

fn run_chunk(
    binary: &Path,
    model_path: &Path,
    wav: &Path,
    language: &str,
    backend: Backend,
) -> Result<String> {
    let mut cmd = Command::new(binary);
    cmd.arg("-q")
        .args([
            "-m",
            model_path
                .to_str()
                .ok_or_else(|| anyhow!("non-UTF8 model path"))?,
        ])
        .args(["--timestamps", "none"])
        .args(["--backend", backend.cli_name()]);
    if !matches!(language, "auto" | "") {
        cmd.args(["-l", language]);
    }
    cmd.arg(wav);

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running {}", binary.display()))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("transcribe.cpp failed (exit {}):\n{err}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = extract_text(&stdout);
    if has_repetition_loop(&text) {
        bail!("transcribe.cpp repetition loop detected");
    }
    Ok(text)
}

#[derive(Clone, Copy)]
struct SpanJob<'a> {
    binary: &'a Path,
    model_path: &'a Path,
    wav: &'a Path,
    language: &'a str,
    backend: Backend,
    start: f64,
    end: f64,
    label: &'a str,
}

fn transcribe_span(job: SpanJob<'_>) -> Result<Vec<ChunkTranscript>> {
    let duration = (job.end - job.start).max(0.0);
    let overlap = CHUNK_OVERLAP_SECONDS.min(duration / 3.0);
    let audio_start = (job.start - overlap).max(0.0);
    let audio_duration = job.end - audio_start;
    let chunk = std::env::temp_dir().join(format!(
        "ss_transcribe_cpp_chunk_{}_{}_{}.wav",
        std::process::id(),
        (job.start * 1000.0).round() as u64,
        (job.end * 1000.0).round() as u64
    ));
    make_chunk(job.wav, &chunk, audio_start, audio_duration)?;
    let result = run_chunk(
        job.binary,
        job.model_path,
        &chunk,
        job.language,
        job.backend,
    );
    let _ = std::fs::remove_file(&chunk);

    match result {
        Ok(text) => Ok(vec![ChunkTranscript {
            text,
            start: job.start,
            end: job.end,
        }]),
        Err(err) => {
            let detail = format!("{err:#}");
            if duration > MIN_RETRY_CHUNK_SECONDS && should_split_error(&detail) {
                let mid = job.start + duration / 2.0;
                crate::ui::warn(&format!(
                    "{} span {:.1}-{:.1}s hit a transcribe.cpp limit; retrying as {:.1}s + {:.1}s chunks",
                    job.label,
                    job.start,
                    job.end,
                    mid - job.start,
                    job.end - mid
                ));
                let mut left = transcribe_span(SpanJob { end: mid, ..job })?;
                let right = transcribe_span(SpanJob { start: mid, ..job })?;
                left.extend(right);
                Ok(left)
            } else {
                Err(err)
            }
        }
    }
}

pub fn run_asr(
    model_path: &Path,
    audio: &Path,
    out_prefix: &Path,
    language: &str,
    device: Option<&str>,
) -> Result<()> {
    let binary = ensure_runtime_for(device)?;
    let backend = runtime_backend(&binary, device);
    let wav = normalize_audio(audio)?;
    let duration = audio_duration_seconds(&wav);
    let checkpoint_file = checkpoint_path(out_prefix);
    let audio_sha1 = first_mb_sha1(&wav)?;
    let model_id = model_path.display().to_string();
    let mut chunks = load_checkpoint(&checkpoint_file, &model_id, &audio_sha1).unwrap_or_default();
    if !chunks.is_empty() {
        let resumed_at = chunks.iter().map(|chunk| chunk.end).fold(0.0, f64::max);
        crate::ui::info(&format!(
            "resuming transcribe.cpp at {}",
            fmt_ts(resumed_at)
        ));
    }

    if duration <= TARGET_CHUNK_SECONDS || duration <= 0.0 {
        if chunks.is_empty() {
            let text = run_chunk(&binary, model_path, &wav, language, backend)?;
            chunks.push(ChunkTranscript {
                text,
                start: 0.0,
                end: duration,
            });
            write_checkpoint(
                &checkpoint_file,
                &Checkpoint {
                    model: model_id.clone(),
                    audio_sha1_first_mb: audio_sha1.clone(),
                    chunk_s: TARGET_CHUNK_SECONDS,
                    overlap_s: CHUNK_OVERLAP_SECONDS,
                    segments: chunks.clone(),
                },
            )?;
        }
    } else {
        let total_chunks = (duration / TARGET_CHUNK_SECONDS).ceil() as usize;
        crate::ui::info(&format!(
            "audio is {:.1} min; splitting into {total_chunks} transcribe.cpp chunks",
            duration / 60.0
        ));
        let completed_until = chunks.iter().map(|chunk| chunk.end).fold(0.0, f64::max);
        let first_pending = first_pending_chunk(completed_until, total_chunks);
        for idx in first_pending..total_chunks {
            crate::ui::step(
                idx + 1,
                total_chunks,
                &format!("transcribe.cpp chunk {}/{}", idx + 1, total_chunks),
            );
            let start = idx as f64 * TARGET_CHUNK_SECONDS;
            let end = (start + TARGET_CHUNK_SECONDS).min(duration);
            let label = format!("chunk {}/{}", idx + 1, total_chunks);
            chunks.extend(transcribe_span(SpanJob {
                binary: &binary,
                model_path,
                wav: &wav,
                language,
                backend,
                start,
                end,
                label: &label,
            })?);
            write_checkpoint(
                &checkpoint_file,
                &Checkpoint {
                    model: model_id.clone(),
                    audio_sha1_first_mb: audio_sha1.clone(),
                    chunk_s: TARGET_CHUNK_SECONDS,
                    overlap_s: CHUNK_OVERLAP_SECONDS,
                    segments: chunks.clone(),
                },
            )?;
        }
    }
    let _ = std::fs::remove_file(&wav);

    let (text, srt_body) = render_chunks(chunks)?;

    let txt = PathBuf::from(format!("{}.txt", out_prefix.display()));
    let srt = PathBuf::from(format!("{}.srt", out_prefix.display()));
    std::fs::write(&txt, format!("{text}\n"))?;
    std::fs::write(&srt, srt_body)?;
    std::fs::remove_file(&checkpoint_file).ok();
    Ok(())
}

fn first_pending_chunk(completed_until: f64, total_chunks: usize) -> usize {
    ((completed_until / TARGET_CHUNK_SECONDS).floor() as usize).min(total_chunks)
}

fn render_chunks(chunks: Vec<ChunkTranscript>) -> Result<(String, String)> {
    let mut non_empty: Vec<ChunkTranscript> = chunks
        .into_iter()
        .filter(|chunk| !chunk.text.trim().is_empty())
        .collect();
    if non_empty.is_empty() {
        bail!("transcribe.cpp produced no transcript text");
    }
    deduplicate_chunk_boundaries(&mut non_empty);
    non_empty.retain(|chunk| !chunk.text.trim().is_empty());
    let text = non_empty
        .iter()
        .map(|chunk| chunk.text.trim())
        .collect::<Vec<_>>()
        .join("\n\n");
    let srt = non_empty
        .iter()
        .enumerate()
        .map(|(idx, chunk)| {
            format!(
                "{}\n{} --> {}\n{}\n\n",
                idx + 1,
                fmt_ts(chunk.start),
                fmt_ts(chunk.end),
                chunk.text.trim()
            )
        })
        .collect();
    Ok((text, srt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_exact_overlap_ignoring_case_and_punctuation() {
        let previous = "We walk toward the old stone bridge.";
        let current = "the OLD stone bridge, and cross the river.";
        assert_eq!(
            trim_repeated_prefix(previous, current),
            "and cross the river."
        );
    }

    #[test]
    fn keeps_text_when_boundary_has_no_matching_overlap() {
        let previous = "I suggest that you toss a pebble.";
        let current = "That goes a little bit into its neck.";
        assert_eq!(trim_repeated_prefix(previous, current), current);
    }

    #[test]
    fn trims_approximate_overlap_with_filler_word_differences() {
        let previous = "You do not think about what you give to your players. You're like, shit. Wait, no. The math.";
        let current = "Players, so you're like, shit, right? No, the math, hold up. I also gave them shackles.";
        assert_eq!(
            trim_repeated_prefix(previous, current),
            "hold up. I also gave them shackles."
        );
    }

    #[test]
    fn detects_repeated_phrase_loop() {
        let text = "I want anti magic shackles for my players. ".repeat(4);
        assert!(has_repetition_loop(&text));
    }

    #[test]
    fn detects_single_word_loop() {
        assert!(has_repetition_loop(
            "Yeah. Yeah. Yeah. Yeah. Yeah. Yeah. Yeah. Yeah."
        ));
    }

    #[test]
    fn accepts_short_natural_repetition() {
        assert!(!has_repetition_loop(
            "No, no, no. That is not what I said. Yes, yes, we can continue."
        ));
    }

    #[test]
    fn checkpoint_requires_matching_model_audio_and_parameters() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("session.progress.json");
        write_checkpoint(
            &path,
            &Checkpoint {
                model: "model.gguf".into(),
                audio_sha1_first_mb: "fingerprint".into(),
                chunk_s: TARGET_CHUNK_SECONDS,
                overlap_s: CHUNK_OVERLAP_SECONDS,
                segments: vec![ChunkTranscript {
                    text: "notes".into(),
                    start: 0.0,
                    end: 30.0,
                }],
            },
        )
        .unwrap();
        assert_eq!(
            load_checkpoint(&path, "model.gguf", "fingerprint")
                .unwrap()
                .len(),
            1
        );
        assert!(load_checkpoint(&path, "other.gguf", "fingerprint").is_none());
        assert!(load_checkpoint(&path, "model.gguf", "other").is_none());
    }

    #[test]
    fn resumed_segments_render_the_same_srt_as_an_uninterrupted_run() {
        let all = vec![
            ChunkTranscript {
                text: "The party enters the crypt.".into(),
                start: 0.0,
                end: 30.0,
            },
            ChunkTranscript {
                text: "The party enters the crypt. They find a bell.".into(),
                start: 25.0,
                end: 55.0,
            },
        ];
        let checkpoint = all[..1].to_vec();
        assert_eq!(first_pending_chunk(checkpoint[0].end, 2), 1);
        let mut resumed = checkpoint;
        resumed.extend_from_slice(&all[1..]);
        assert_eq!(render_chunks(resumed).unwrap(), render_chunks(all).unwrap());
    }
}
