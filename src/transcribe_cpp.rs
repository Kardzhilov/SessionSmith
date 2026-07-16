//! Local transcribe.cpp GGUF runner.
//!
//! Used for ASR model families that are available as GGUF files, such as
//! Cohere Transcribe 03-2026. SessionSmith fetches/builds the local
//! `transcribe-cli` runtime into the cache when needed, then runs it on a
//! normalized 16 kHz mono WAV.

use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const REPO_URL: &str = "https://github.com/handy-computer/transcribe.cpp.git";

fn runtime_root() -> Result<PathBuf> {
    let base = dirs::cache_dir().ok_or_else(|| anyhow!("could not resolve XDG cache dir"))?;
    Ok(base.join("sessionsmith").join("transcribe.cpp"))
}

fn cached_binary() -> Result<PathBuf> {
    Ok(runtime_root()?.join("build").join("bin").join("transcribe-cli"))
}

fn which(bin: &str) -> Option<PathBuf> {
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin}"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
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
    if let Some(p) = find_runtime() {
        return Ok(p);
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
    let configure = Command::new("cmake")
        .current_dir(&root)
        .args(["-B", "build", "-DCMAKE_BUILD_TYPE=Release"])
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
        .args(["--build", "build", "--target", "transcribe-cli", "--parallel", &jobs])
        .status()
        .with_context(|| "building transcribe.cpp")?;
    if !build.success() {
        bail!("cmake build for transcribe.cpp failed");
    }

    let bin = cached_binary()?;
    if !bin.exists() {
        bail!("transcribe.cpp build completed but {} is missing", bin.display());
    }
    Ok(bin)
}

fn normalize_audio(audio: &Path) -> Result<PathBuf> {
    let stem = audio
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("audio");
    let mut out = std::env::temp_dir();
    out.push(format!("ss_transcribe_cpp_{}_{}.wav", stem, std::process::id()));
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
        bail!("ffmpeg could not convert {} to 16 kHz mono WAV", audio.display());
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
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(audio)
        .output();
    out.ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

pub fn run_asr(
    model_path: &Path,
    audio: &Path,
    out_prefix: &Path,
    language: &str,
    device: Option<&str>,
) -> Result<()> {
    let binary = ensure_runtime()?;
    let wav = normalize_audio(audio)?;
    let duration = audio_duration_seconds(&wav);

    let mut cmd = Command::new(&binary);
    cmd.arg("-q")
        .args(["-m", model_path.to_str().ok_or_else(|| anyhow!("non-UTF8 model path"))?])
        .args(["--timestamps", "none"]);
    if !matches!(language, "auto" | "") {
        cmd.args(["-l", language]);
    }
    if let Some(device) = device {
        match device.to_ascii_lowercase().as_str() {
            "cpu" | "cuda" | "vulkan" | "metal" => {
                cmd.args(["--backend", device]);
            }
            _ => {}
        }
    }
    cmd.arg(&wav);

    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running {}", binary.display()))?;
    let _ = std::fs::remove_file(&wav);

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("transcribe.cpp failed (exit {}):\n{err}", output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = extract_text(&stdout);
    if text.is_empty() {
        bail!("transcribe.cpp produced no transcript text");
    }

    let txt = PathBuf::from(format!("{}.txt", out_prefix.display()));
    let srt = PathBuf::from(format!("{}.srt", out_prefix.display()));
    std::fs::write(&txt, format!("{text}\n"))?;
    std::fs::write(
        &srt,
        format!("1\n{} --> {}\n{}\n\n", fmt_ts(0.0), fmt_ts(duration), text),
    )?;
    Ok(())
}
