//! Transcription: delegates to whisper.cpp's `whisper-cli` **or** `whisperx`
//! (whichever is found first), producing `transcripts/<stem>.txt` and `.srt`.

use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::config::GlobalConfig;
use crate::models;

/// PID of the currently-running whisperx child process (0 = none).
/// Set before spawn, cleared after wait. The Ctrl-C handler reads this
/// to send SIGTERM so VRAM is freed immediately on exit.
pub static WHISPERX_PID: AtomicU32 = AtomicU32::new(0);

/// Kill the current ASR child if one is running. Called from the Ctrl-C handler.
pub fn kill_current_asr() {
    let pid = WHISPERX_PID.load(Ordering::Relaxed);
    if pid > 0 {
        std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .ok();
    }
}

/// Which ASR engine was resolved.
enum AsrBackend {
    /// whisper.cpp's `whisper-cli`: takes a ggml model path via `-m`.
    WhisperCli(PathBuf),
    /// Python `whisperx`: takes a model *name* via `--model`, uses the HF cache.
    WhisperX(PathBuf),
}

#[derive(Debug, Clone)]
pub struct TranscribeOpts {
    pub model: String,
    pub language: String,
    pub force: bool,
}

#[derive(Debug)]
pub struct TranscribeOutput {
    pub txt: PathBuf,
    pub srt: PathBuf,
}

pub async fn transcribe(audio: &Path, out_dir: &Path, g: &GlobalConfig, opts: &TranscribeOpts)
    -> Result<TranscribeOutput>
{
    std::fs::create_dir_all(out_dir)?;
    let stem = audio.file_stem().ok_or_else(|| anyhow!("no stem for {}", audio.display()))?
        .to_string_lossy().to_string();
    let out_txt = out_dir.join(format!("{stem}.txt"));
    let out_srt = out_dir.join(format!("{stem}.srt"));

    if !opts.force && out_txt.exists() && out_srt.exists() {
        crate::ui::ok(&format!("transcript exists: {}", out_txt.display()));
        return Ok(TranscribeOutput { txt: out_txt, srt: out_srt });
    }

    let backend = resolve_asr_backend(g)?;

    // Only the whisper.cpp path needs the ggml model file; whisperx manages its
    // own model cache via Hugging Face.
    let model_path_opt = if matches!(backend, AsrBackend::WhisperCli(_)) {
        let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
        Some(models::ensure_whisper(&opts.model, &cache).await?)
    } else {
        None
    };

    let spinner = crate::ui::spinner(&format!("transcribing {stem} with whisper-{}", opts.model));

    let (binary_path, output, asr_device_opt) = match &backend {
        AsrBackend::WhisperCli(binary) => {
            let model_path = model_path_opt.as_ref().unwrap();
            let threads = g.asr.threads.unwrap_or_else(|| {
                std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4).min(8)
            });
            // whisper-cli writes <prefix>.txt and <prefix>.srt.
            let prefix = out_dir.join(&stem);
            let mut cmd = Command::new(binary);
            cmd.args(["-m", model_path.to_str().unwrap()])
                .arg("-f").arg(audio)
                .args(["-otxt", "-osrt"])
                .arg("-of").arg(&prefix)
                .args(["-t", &threads.to_string()])
                .args(["-p", "1"]);
            if opts.language != "auto" {
                cmd.args(["-l", &opts.language]);
            }
            cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
            let o = cmd.output().with_context(|| format!("running {}", binary.display()))?;
            (binary.clone(), o, None::<&'static str>)
        }
        AsrBackend::WhisperX(binary) => {
            // whisperx names outputs after the audio stem, which matches <out_dir>/<stem>.*
            //
            // We call the venv's Python interpreter directly with `-m whisperx` rather than
            // the whisperx entry-point script.  Entry-point scripts embed an absolute shebang
            // written at install time; if the venv was copied or created from a different Python
            // that shebang can point to the wrong interpreter, pulling in wrong site-packages.
            // Using `<venv>/bin/python3 -m whisperx` always uses the correct interpreter.
            let python = binary.parent()
                .map(|p| p.join("python3"))
                .filter(|p| p.exists())
                .unwrap_or_else(|| binary.clone());
            let use_module = python != *binary; // true when we found a sibling python3

            let free_mb = free_vram_mb();
            let device = if free_mb >= 4096 { "cuda" } else { "cpu" };
            let compute = if device == "cuda" { "float16" } else { "int8" };

            let run_whisperx = |device: &str, compute: &str| -> std::io::Result<std::process::Output> {
                let mut cmd = if use_module {
                    let mut c = Command::new(&python);
                    c.args(["-m", "whisperx"]);
                    c
                } else {
                    Command::new(binary)
                };
                cmd.arg(audio)
                    .args(["--model", &opts.model])
                    .args(["--output_dir", out_dir.to_str().unwrap()])
                    .args(["--output_format", "all"])
                    .arg("--no_align")
                    .args(["--device", device, "--compute_type", compute])
                    .env_remove("PYTHONPATH"); // prevent stale PYTHONPATH from leaking in
                if opts.language != "auto" {
                    cmd.args(["--language", &opts.language]);
                }
                cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
                // Use spawn() so we can track the PID and kill it cleanly on Ctrl-C.
                let child = cmd.spawn()?;
                WHISPERX_PID.store(child.id(), Ordering::Relaxed);
                let out = child.wait_with_output()?;
                WHISPERX_PID.store(0, Ordering::Relaxed);
                Ok(out)
            };

            let mut o = run_whisperx(device, compute)
                .with_context(|| format!("running {}", binary.display()))?;

            // Retry on CUDA OOM — GPU may be occupied by the LLM backend.
            let mut used_cpu_fallback = false;
            if !o.status.success() {
                let err_text = String::from_utf8_lossy(&o.stderr);
                if device == "cuda" && (err_text.contains("out of memory") || err_text.contains("CUDA")) {
                    crate::ui::warn("CUDA OOM — retrying whisperx on CPU");
                    o = run_whisperx("cpu", "int8")
                        .with_context(|| format!("running {} (cpu retry)", binary.display()))?;
                    used_cpu_fallback = true;
                }
            }

            let device_label: &'static str = match (device, used_cpu_fallback, free_mb) {
                ("cuda", false, _) => "cuda",
                (_, true, _)       => "cpu (fallback — VRAM full)",
                (_, false, 0)      => "cpu (no GPU detected)",
                _                  => "cpu (VRAM low — GPU occupied)",
            };
            (binary.clone(), o, Some(device_label))
        }
    };

    spinner.finish_and_clear();
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        bail!("ASR failed (exit {}):\n{err}", output.status);
    }

    if let Some(dev) = asr_device_opt {
        crate::ui::info(&format!("ASR device: {dev}"));
    }
    if !out_txt.exists() {
        bail!("ASR did not produce {} — check stderr above", out_txt.display());
    }
    crate::ui::ok(&format!("wrote {}", out_txt.display()));
    if out_srt.exists() {
        crate::ui::ok(&format!("wrote {}", out_srt.display()));
    }
    let _ = binary_path; // used only for error context above
    Ok(TranscribeOutput { txt: out_txt, srt: out_srt })
}

/// Concatenate multiple audio files into one using ffmpeg's concat demuxer.
/// Returns the path to the merged file in `out_dir/<stem>.wav`.
/// If only one file is provided, returns it directly (no concat).
pub async fn concat_audio_files(files: &[PathBuf], stem: &str, out_dir: &Path) -> Result<PathBuf> {
    if files.is_empty() {
        anyhow::bail!("concat_audio_files: no input files");
    }
    if files.len() == 1 {
        return Ok(files[0].clone());
    }
    std::fs::create_dir_all(out_dir)?;
    let out = out_dir.join(format!("{stem}.wav"));
    if out.exists() {
        return Ok(out);
    }
    let list_path = out_dir.join(format!("_{stem}_concat.txt"));
    let content: String = files.iter()
        .map(|f| {
            let abs = f.canonicalize().unwrap_or_else(|_| f.clone());
            format!("file '{}'\n", abs.display())
        })
        .collect();
    std::fs::write(&list_path, &content)?;
    let pb = crate::ui::spinner(&format!("merging {} files with ffmpeg", files.len()));
    let status = Command::new("ffmpeg")
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path)
        .args(["-c", "copy"])
        .arg(&out)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| "ffmpeg not found — install ffmpeg")?;
    pb.finish_and_clear();
    std::fs::remove_file(&list_path).ok();
    if !status.success() {
        // Re-encode fallback (handles mismatched codecs/sample rates).
        let list2 = out_dir.join(format!("_{stem}_concat2.txt"));
        std::fs::write(&list2, &content)?;
        let pb2 = crate::ui::spinner("re-encoding merge (codec mismatch)");
        let st2 = Command::new("ffmpeg")
            .args(["-y", "-f", "concat", "-safe", "0", "-i"])
            .arg(&list2)
            .args(["-ar", "16000", "-ac", "1"])
            .arg(&out)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        pb2.finish_and_clear();
        std::fs::remove_file(&list2).ok();
        if !st2.success() {
            bail!("ffmpeg could not concatenate audio files");
        }
    }
    crate::ui::ok(&format!("merged audio → {}", out.display()));
    Ok(out)
}

/// Locate the best available ASR engine, preferring whisper.cpp then whisperx.
///
/// Resolution order:
///   1. `[asr].binary` from global config (explicit override)
///   2. `whisper-cli` / `whisper.cpp` / `main` on PATH  (whisper.cpp)
///   3. `./whisper.cpp/build/bin/whisper-cli` or `./build/bin/whisper-cli`  (local build)
///   4. `.venv/bin/whisperx`  (project-local Python venv — works out of the box)
///   5. `whisperx` on PATH
fn resolve_asr_backend(g: &GlobalConfig) -> Result<AsrBackend> {
    // 1. Explicit binary from config
    if let Some(b) = &g.asr.binary {
        if b.exists() {
            let name = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
            return if name.contains("whisperx") {
                Ok(AsrBackend::WhisperX(b.clone()))
            } else {
                Ok(AsrBackend::WhisperCli(b.clone()))
            };
        }
    }

    // 2. whisper.cpp variants on PATH
    for candidate in ["whisper-cli", "whisper.cpp", "main"] {
        if let Some(p) = path_of(candidate) {
            return Ok(AsrBackend::WhisperCli(p));
        }
    }

    // 3. Common local whisper.cpp build locations
    for extra in ["./whisper.cpp/build/bin/whisper-cli", "./build/bin/whisper-cli"] {
        let p = Path::new(extra);
        if p.exists() {
            return Ok(AsrBackend::WhisperCli(p.to_path_buf()));
        }
    }

    // 4. whisperx in the project's .venv (no external install required)
    let venv_wx = Path::new(".venv/bin/whisperx");
    if venv_wx.exists() {
        // Canonicalize to an absolute path so that the sibling `python3` lookup
        // succeeds regardless of the working directory, and so Python can locate
        // `pyvenv.cfg` unambiguously when invoked via an absolute symlink path.
        let abs = std::fs::canonicalize(venv_wx).unwrap_or_else(|_| venv_wx.to_path_buf());
        return Ok(AsrBackend::WhisperX(abs));
    }

    // 5. whisperx anywhere on PATH
    if let Some(p) = path_of("whisperx") {
        return Ok(AsrBackend::WhisperX(p));
    }

    bail!(
        "No ASR engine found.\n\n\
         Option A — whisper.cpp (faster, no Python needed):\n  \
           git clone https://github.com/ggerganov/whisper.cpp\n  \
           cd whisper.cpp && make -j\n  \
           sudo cp main /usr/local/bin/whisper-cli\n\n\
         Option B — whisperx (already in .venv if present):\n  \
           python -m venv .venv && .venv/bin/pip install whisperx\n\n\
         Or set an explicit path in ~/.config/sessionsmith/config.toml:\n  \
           [asr]\n  \
           binary = \"/path/to/whisper-cli\"  # or whisperx\n"
    )
}

/// Returns free VRAM in MiB on the first GPU, or 0 if no GPU / nvidia-smi unavailable.
fn free_vram_mb() -> u64 {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .and_then(|l| l.trim().parse::<u64>().ok())
                .unwrap_or(0)
        }
        _ => 0,
    }
}

/// Resolve a command name to its full path via `which`.
fn path_of(cmd: &str) -> Option<PathBuf> {
    let out = Command::new("which").arg(cmd).output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}
