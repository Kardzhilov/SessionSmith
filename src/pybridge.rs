//! Python bridge for the modern ASR engines (faster-whisper, NVIDIA Parakeet /
//! Canary, Mistral Voxtral).
//!
//! These models live in the Python ecosystem. Rather than requiring a manual
//! `pip install`, SessionSmith ships small self-contained bridge scripts and
//! runs them with [`uv`](https://docs.astral.sh/uv/). The scripts declare their
//! dependencies inline (PEP 723), so `uv run` transparently creates and caches
//! an isolated environment on first use and fetches the model weights lazily.
//!
//! Each script writes `<out_prefix>.txt` and `<out_prefix>.srt`, and streams
//! progress to stdout as lines of the form `@@P <pos> <total> <label>`, which
//! we forward to [`crate::ui::progress`] so the TUI shows a live bar.

use anyhow::{anyhow, bail, Context, Result};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;

use crate::asr::AsrEngine;
use crate::jobs::procs::{configure_command, ChildRegistry};

/// Locate the `uv` binary (PATH, then the common `~/.local/bin` install dir).
pub fn uv_path() -> Option<PathBuf> {
    if let Ok(p) = which("uv") {
        return Some(p);
    }
    let home = dirs::home_dir()?;
    for cand in ["/.local/bin/uv", "/.cargo/bin/uv"] {
        let p = home.join(cand.trim_start_matches('/'));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn which(bin: &str) -> Result<PathBuf> {
    crate::util::find_in_path(bin).ok_or_else(|| anyhow!("{bin} not found"))
}

/// Directory where bridge scripts are materialised.
fn bridge_dir() -> Result<PathBuf> {
    let base = dirs::cache_dir().ok_or_else(|| anyhow!("no cache dir"))?;
    let dir = base.join("sessionsmith").join("bridges");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn hf_cache_model_dir(repo: &str) -> Option<PathBuf> {
    let cache = dirs::cache_dir()?;
    let safe = repo.replace('/', "--");
    Some(
        cache
            .join("huggingface")
            .join("hub")
            .join(format!("models--{safe}")),
    )
}

fn bridge_model_cache(engine: AsrEngine, model_ref: &str) -> Option<PathBuf> {
    match engine {
        AsrEngine::FasterWhisper => {
            hf_cache_model_dir(&format!("Systran/faster-whisper-{model_ref}"))
        }
        AsrEngine::Parakeet
        | AsrEngine::CanaryQwen
        | AsrEngine::Voxtral
        | AsrEngine::GraniteSpeech
        | AsrEngine::MossTranscribeDiarize => hf_cache_model_dir(model_ref),
        AsrEngine::TranscribeCpp | AsrEngine::WhisperCpp => None,
    }
}

/// Best-effort deletion of local files SessionSmith can confidently attribute
/// to one Python bridge ASR model.
pub fn delete_asr_cache(engine: AsrEngine, model_ref: &str) -> Result<()> {
    delete_bridge_model_cache(bridge_model_cache(engine, model_ref))?;
    let (script_name, _) = script_for(engine)?;
    if let Some(cache_dir) = dirs::cache_dir() {
        let script = cache_dir
            .join("sessionsmith")
            .join("bridges")
            .join(script_name);
        if script.exists() {
            if let Err(error) = std::fs::remove_file(&script) {
                crate::ui::warn(&format!(
                    "could not remove unused bridge script {}: {error}",
                    script.display()
                ));
            }
        }
    }
    Ok(())
}

fn delete_bridge_model_cache(model_dir: Option<PathBuf>) -> Result<()> {
    if let Some(model_dir) = model_dir {
        if model_dir.exists() {
            std::fs::remove_dir_all(&model_dir)
                .with_context(|| format!("deleting {}", model_dir.display()))?;
        }
    }
    Ok(())
}

/// Materialise `contents` at `<bridge_dir>/<name>`, rewriting only when changed
/// (keeps `uv`'s per-script environment cache warm across runs).
fn write_script(name: &str, contents: &str) -> Result<PathBuf> {
    let path = bridge_dir()?.join(name);
    let needs_write = std::fs::read_to_string(&path)
        .map(|existing| existing != contents)
        .unwrap_or(true);
    if needs_write {
        std::fs::write(&path, contents)?;
    }
    Ok(path)
}

/// The bridge script name + assembled source (with [`COMMON_PY`] inlined) for
/// an engine.
fn script_for(engine: AsrEngine) -> Result<(&'static str, String)> {
    let (name, template) = match engine {
        AsrEngine::FasterWhisper => ("asr_faster_whisper.py", FASTER_WHISPER_PY),
        AsrEngine::Parakeet | AsrEngine::CanaryQwen => ("asr_nemo.py", NEMO_PY),
        AsrEngine::Voxtral => ("asr_voxtral.py", VOXTRAL_PY),
        AsrEngine::GraniteSpeech => ("asr_granite_speech.py", GRANITE_SPEECH_PY),
        AsrEngine::MossTranscribeDiarize => {
            ("asr_moss_transcribe_diarize.py", MOSS_TRANSCRIBE_DIARIZE_PY)
        }
        AsrEngine::TranscribeCpp => bail!("transcribe.cpp does not use the Python bridge"),
        AsrEngine::WhisperCpp => bail!("whisper.cpp does not use the Python bridge"),
    };
    // Inline the shared helpers where the `COMMON` marker line appears.
    let assembled = template.replacen("\nCOMMON\n", &format!("\n{COMMON_PY}\n"), 1);
    Ok((name, assembled))
}

/// Run transcription for a bridge engine. On success `<out_prefix>.txt` (and
/// `.srt`) exist on disk.
#[allow(clippy::too_many_arguments)]
pub fn run_asr(
    engine: AsrEngine,
    model_ref: &str,
    audio: &Path,
    out_prefix: &Path,
    device: &str,
    language: &str,
    initial_prompt: Option<&str>,
    diarize: bool,
) -> Result<()> {
    run_asr_with_children(
        engine,
        model_ref,
        audio,
        out_prefix,
        device,
        language,
        initial_prompt,
        diarize,
        None,
    )
}

/// Run a bridge ASR engine while associating its `uv` process with a
/// host-owned job. Existing CLI callers use [`run_asr`] without a registry.
#[allow(clippy::too_many_arguments)]
pub fn run_asr_with_children(
    engine: AsrEngine,
    model_ref: &str,
    audio: &Path,
    out_prefix: &Path,
    device: &str,
    language: &str,
    initial_prompt: Option<&str>,
    diarize: bool,
    children: Option<&ChildRegistry>,
) -> Result<()> {
    let (name, contents) = script_for(engine)?;
    let script = write_script(name, &contents)?;
    let uv = require_uv(engine)?;

    // Most speech-language bridges expect a decodable 16 kHz mono WAV.
    // faster-whisper decodes internally, so it keeps the original file.
    let temp_wav = match engine {
        AsrEngine::Parakeet
        | AsrEngine::CanaryQwen
        | AsrEngine::Voxtral
        | AsrEngine::GraniteSpeech
        | AsrEngine::MossTranscribeDiarize => normalize_audio(audio, children),
        AsrEngine::FasterWhisper | AsrEngine::TranscribeCpp | AsrEngine::WhisperCpp => None,
    };
    let effective_audio = temp_wav.as_deref().unwrap_or(audio);

    let mut cmd = Command::new(&uv);
    cmd.arg("run").arg("--quiet").arg(&script);
    cmd.arg("--audio").arg(effective_audio);
    cmd.arg("--out").arg(out_prefix);
    cmd.args(["--model", model_ref]);
    cmd.args(["--device", device]);
    cmd.args(["--language", language]);
    if matches!(
        engine,
        AsrEngine::FasterWhisper | AsrEngine::GraniteSpeech | AsrEngine::MossTranscribeDiarize
    ) {
        if let Some(prompt) = initial_prompt.filter(|prompt| !prompt.is_empty()) {
            cmd.args(["--initial-prompt", prompt]);
        }
    }
    if diarize {
        cmd.arg("--diarize");
    }
    if let Some(arch) = arch_of(engine) {
        cmd.args(["--arch", arch]);
    }
    crate::ui::info(&format!(
        "{} · preparing environment with uv (first run downloads dependencies)…",
        engine.label()
    ));
    let res = stream_uv(engine, &mut cmd, children);
    if let Some(tmp) = temp_wav {
        let _ = std::fs::remove_file(&tmp);
    }
    res?;

    // The bridge writes `<out_prefix>.txt` by *string* concatenation, so build
    // the same path here — `Path::with_extension` would mangle stems that
    // contain a dot (e.g. `session.part1`).
    let txt = PathBuf::from(format!("{}.txt", out_prefix.display()));
    if !txt.exists() {
        bail!(
            "{} produced no transcript at {}",
            engine.label(),
            txt.display()
        );
    }
    Ok(())
}

/// Convert `audio` to a temporary 16 kHz mono WAV via ffmpeg, returning the temp
/// path (or `None` if ffmpeg is unavailable/fails, so the caller falls back to
/// the original file).
fn normalize_audio(audio: &Path, children: Option<&ChildRegistry>) -> Option<PathBuf> {
    let stem = audio
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("audio");
    let mut out = std::env::temp_dir();
    out.push(format!("ss_bridge_{}_{}.wav", stem, std::process::id()));
    let mut command = Command::new("ffmpeg");
    command
        .arg("-y")
        .arg("-i")
        .arg(audio)
        .args(["-ar", "16000", "-ac", "1"])
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let status = run_command_status(&mut command, children, "ffmpeg bridge normalization").ok()?;
    if status.success() {
        Some(out)
    } else {
        None
    }
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

/// Download the environment + model weights for a bridge engine without
/// transcribing (the in-app "prepare/install" action).
pub fn run_asr_prepare(engine: AsrEngine, model_ref: &str, device: &str) -> Result<()> {
    run_asr_prepare_with_children(engine, model_ref, device, None)
}

/// Prepare a bridge ASR engine while associating the `uv` process with a
/// host-owned job. Existing CLI callers use [`run_asr_prepare`] without a
/// registry.
pub fn run_asr_prepare_with_children(
    engine: AsrEngine,
    model_ref: &str,
    device: &str,
    children: Option<&ChildRegistry>,
) -> Result<()> {
    let (name, contents) = script_for(engine)?;
    let script = write_script(name, &contents)?;
    let uv = require_uv(engine)?;

    let mut cmd = Command::new(&uv);
    cmd.arg("run").arg("--quiet").arg(&script);
    cmd.args(["--model", model_ref]);
    cmd.args(["--device", device]);
    cmd.arg("--prepare");
    if let Some(arch) = arch_of(engine) {
        cmd.args(["--arch", arch]);
    }
    crate::ui::info(&format!(
        "{} · downloading environment + weights with uv (this can take a while)…",
        engine.label()
    ));
    stream_uv(engine, &mut cmd, children)
}

fn require_uv(engine: AsrEngine) -> Result<PathBuf> {
    uv_path().ok_or_else(|| {
        anyhow!(
            "`uv` is required to run {} models but was not found.\n  \
             Install it with: curl -LsSf https://astral.sh/uv/install.sh | sh",
            engine.label()
        )
    })
}

fn arch_of(engine: AsrEngine) -> Option<&'static str> {
    match engine {
        AsrEngine::CanaryQwen => Some("canary"),
        AsrEngine::Parakeet => Some("parakeet"),
        _ => None,
    }
}

/// Spawn `cmd` (a `uv run …` bridge invocation), stream its `@@P` progress lines
/// to the UI, and surface stderr on failure.
fn stream_uv(engine: AsrEngine, cmd: &mut Command, children: Option<&ChildRegistry>) -> Result<()> {
    cmd.env("PYTHONUNBUFFERED", "1");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    if children.is_some() {
        configure_command(cmd);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawning uv for {}", engine.label()))?;
    let registration = children
        .map(|children| children.register(&child, format!("uv bridge: {}", engine.label())));

    // Track the PID so the Ctrl-C handler can free the GPU immediately.
    crate::transcribe::WHISPERX_PID.store(child.id(), Ordering::Relaxed);

    // Drain stderr on a thread (prevents pipe-buffer deadlock).
    let stderr = child.stderr.take();
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut e) = stderr {
            let _ = e.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(out) = child.stdout.take() {
        let reader = BufReader::new(out);
        for line in reader.lines().map_while(std::result::Result::ok) {
            if let Some(rest) = line.strip_prefix("@@P ") {
                let mut it = rest.splitn(3, ' ');
                let pos = it.next().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                let total = it.next().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                let label = it.next().unwrap_or("working").to_string();
                crate::ui::progress(&label, pos as u64, total as u64);
            }
        }
    }

    let status = child.wait();
    crate::transcribe::WHISPERX_PID.store(0, Ordering::Relaxed);
    drop(registration);
    let stderr_text = stderr_handle.join().unwrap_or_default();
    let status = status.with_context(|| "waiting on uv bridge process")?;

    if !status.success() {
        // Surface the most informative lines: a Python traceback's final
        // message and/or a C++ abort message (`what(): ...`, `RuntimeError`,
        // CUDA errors) rather than the low-level stack frames, which are noise.
        let keywords = [
            "error",
            "exception",
            "traceback",
            "runtimeerror",
            "valueerror",
            "what():",
            "terminate called",
            "cuda",
            "assert",
            "oom",
            "out of memory",
        ];
        let lines: Vec<&str> = stderr_text.lines().collect();
        let mut highlights: Vec<&str> = lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let low = line.to_ascii_lowercase();
                if keywords.iter().any(|k| low.contains(k))
                    && !line.trim_start().starts_with("frame #")
                {
                    Some(index)
                } else {
                    None
                }
            })
            .flat_map(|index| lines[index..lines.len().min(index + 3)].iter().copied())
            .filter(|line| !line.trim_start().starts_with("frame #"))
            .collect();
        // De-dupe consecutive repeats and cap length.
        highlights.dedup();
        let summary = if highlights.is_empty() {
            // Fall back to the tail if nothing matched.
            stderr_text
                .lines()
                .rev()
                .take(20)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
        } else {
            highlights
                .into_iter()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
        }
        .join("\n");
        bail!("{} failed:\n{summary}", engine.label());
    }
    Ok(())
}

// ===========================================================================
// Embedded bridge scripts (PEP 723 inline dependencies).
// ===========================================================================

/// Shared Python helpers (SRT formatting, arg parsing, progress).
const COMMON_PY: &str = r#"
import argparse, sys

def emit(pos, total, label):
    print(f"@@P {pos} {total} {label}", flush=True)

def parse_args():
    ap = argparse.ArgumentParser()
    ap.add_argument("--audio", default="")
    ap.add_argument("--out", default="")
    ap.add_argument("--model", required=True)
    ap.add_argument("--device", default="auto")
    ap.add_argument("--language", default="auto")
    ap.add_argument("--initial-prompt", default="")
    ap.add_argument("--arch", default="")
    ap.add_argument("--diarize", action="store_true")
    # Prepare mode: create the environment and download the model weights, then
    # exit without transcribing (used by the in-app "prepare" action).
    ap.add_argument("--prepare", action="store_true")
    return ap.parse_args()

def fmt_ts(seconds):
    if seconds < 0:
        seconds = 0
    ms = int(round(seconds * 1000))
    h = ms // 3600000; ms -= h * 3600000
    m = ms // 60000; ms -= m * 60000
    s = ms // 1000; ms -= s * 1000
    return f"{h:02d}:{m:02d}:{s:02d},{ms:03d}"

def write_outputs(out_prefix, segments):
    # segments: list of (start, end, text)
    text = "\n".join(seg[2].strip() for seg in segments if seg[2].strip())
    with open(out_prefix + ".txt", "w", encoding="utf-8") as f:
        f.write(text + ("\n" if text else ""))
    with open(out_prefix + ".srt", "w", encoding="utf-8") as f:
        for i, (start, end, t) in enumerate(segments, 1):
            t = t.strip()
            if not t:
                continue
            f.write(f"{i}\n{fmt_ts(start)} --> {fmt_ts(end)}\n{t}\n\n")
"#;

/// faster-whisper (CTranslate2) bridge.
const FASTER_WHISPER_PY: &str = r#"# /// script
# requires-python = ">=3.9"
# dependencies = ["faster-whisper>=1.1.0", "nvidia-cublas-cu12", "nvidia-cudnn-cu12"]
# ///
import sys, os, glob
COMMON
def ensure_cuda_ld_path():
    # CTranslate2 dlopen()s libcublas.so.12 / libcudnn*.so by soname, which the
    # dynamic loader resolves via LD_LIBRARY_PATH — but that env var is only read
    # at process start, so setting it at runtime is too late. Compute the
    # pip-installed NVIDIA lib dirs, prepend them, and re-exec ourselves once so
    # the loader can actually find the CUDA libraries.
    if os.environ.get("SS_CUDA_REEXEC"):
        return
    try:
        import nvidia
        # `nvidia` is a namespace package (no __init__), so __file__ is None —
        # use __path__ to find the per-library `lib` dirs (cublas, cudnn, ...).
        libdirs = []
        for base in list(getattr(nvidia, "__path__", [])):
            for d in os.listdir(base):
                p = os.path.join(base, d, "lib")
                if os.path.isdir(p):
                    libdirs.append(p)
    except Exception:
        libdirs = []
    if not libdirs:
        return
    cur = os.environ.get("LD_LIBRARY_PATH", "")
    new = os.pathsep.join(libdirs + ([cur] if cur else []))
    if new != cur:
        os.environ["LD_LIBRARY_PATH"] = new
        os.environ["SS_CUDA_REEXEC"] = "1"
        os.execv(sys.executable, [sys.executable] + sys.argv)

def cuda_available():
    try:
        import ctranslate2
        return ctranslate2.get_cuda_device_count() > 0
    except Exception:
        return False

def transcribe_with(model_name, device, audio, language, initial_prompt):
    from faster_whisper import WhisperModel
    compute = "float16" if device == "cuda" else "int8"
    emit(0, 0, f"loading {model_name} on {device}")
    model = WhisperModel(model_name, device=device, compute_type=compute)
    lang = None if language in ("auto", "") else language
    prompt = initial_prompt or None
    segments, info = model.transcribe(audio, language=lang, vad_filter=True, initial_prompt=prompt)
    total = getattr(info, "duration", 0) or 0
    out = []
    for seg in segments:
        out.append((seg.start, seg.end, seg.text))
        emit(round(seg.end, 2), round(total, 2), "transcribing")
    return out, total

def main():
    args = parse_args()
    device = args.device
    # Make CUDA libs discoverable (re-execs once) before probing / loading.
    if device in ("auto", "cuda", ""):
        ensure_cuda_ld_path()
    if device in ("auto", ""):
        device = "cuda" if cuda_available() else "cpu"
    if args.prepare:
        from faster_whisper import WhisperModel
        emit(0, 0, f"downloading {args.model}")
        WhisperModel(args.model, device="cpu", compute_type="int8")
        emit(1, 1, "ready")
        return
    try:
        out, total = transcribe_with(args.model, device, args.audio, args.language, args.initial_prompt)
    except Exception as e:
        if device == "cuda":
            sys.stderr.write(f"CUDA path failed ({e}); retrying on CPU\n")
            out, total = transcribe_with(args.model, "cpu", args.audio, args.language, args.initial_prompt)
        else:
            raise
    write_outputs(args.out, out)
    emit(round(total, 2), round(total, 2), "done")

if __name__ == "__main__":
    main()
"#;

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn deletes_an_advanced_model_cache_directory() {
        let temp = tempdir().expect("temporary cache root should be available");
        let model_dir = temp
            .path()
            .join("huggingface/hub/models--OpenMOSS-Team--MOSS-Transcribe-Diarize");
        std::fs::create_dir_all(model_dir.join("snapshots/example"))
            .expect("model snapshot directory should be created");
        std::fs::write(model_dir.join("snapshots/example/config.json"), "{}")
            .expect("model snapshot should be written");

        delete_bridge_model_cache(Some(model_dir.clone()))
            .expect("model cache deletion should succeed");

        assert!(!model_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn registry_aware_status_command_can_be_cancelled() {
        use std::time::Duration;

        let registry = ChildRegistry::default();
        let worker_registry = registry.clone();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 2"]);
            run_command_status(&mut command, Some(&worker_registry), "test normalization")
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
            panic!("test process should be registered");
        }

        assert_eq!(registry.kill_all(), 1);
        let status = worker.join().expect("test worker should complete");
        assert!(!status.success());
        assert!(registry.active_children().is_empty());
    }
}

/// IBM Granite Speech bridge. The model supports keyword list biasing through
/// its transcription prompt; SessionSmith's `Glossary:` prompt is translated to
/// its documented `Keywords:` syntax here.
const GRANITE_SPEECH_PY: &str = r#"# /// script
# requires-python = ">=3.10,<3.13"
# dependencies = ["transformers>=4.52.1", "torch", "torchaudio", "soundfile", "numpy", "accelerate"]
# ///
import sys
COMMON

def load_audio_16k_mono(path):
    import soundfile as sf
    import numpy as np
    data, sr = sf.read(path, dtype="float32", always_2d=False)
    if getattr(data, "ndim", 1) > 1:
        data = data.mean(axis=1)
    if sr != 16000:
        n = int(round(len(data) * 16000 / sr))
        if n > 0:
            x_old = np.linspace(0, 1, num=len(data), endpoint=False)
            x_new = np.linspace(0, 1, num=n, endpoint=False)
            data = np.interp(x_new, x_old, data).astype("float32")
    return data

# Short windows are required for faithfulness: on long windows the small LLM
# decoder silently condenses/skips content (measured: 66 deleted words at 540s
# vs 7 at 30s on a 4-min read).
def chunk_indices(total_samples, sr, window_s=30.0, overlap_s=2.0):
    win = int(window_s * sr); overlap = int(overlap_s * sr)
    step = max(win - overlap, 1)
    start = 0
    while start < total_samples:
        yield start, min(start + win, total_samples)
        if start + win >= total_samples:
            break
        start += step

def dedup_overlap(prev_text, next_text, max_words=12):
    # Trim words re-transcribed from the 2s chunk overlap off the next chunk.
    def key(word):
        return "".join(ch for ch in word.lower() if ch.isalnum())
    prev, nxt = prev_text.split(), next_text.split()
    for n in range(min(max_words, len(prev), len(nxt)), 1, -1):
        if [key(w) for w in prev[-n:]] == [key(w) for w in nxt[:n]]:
            return " ".join(nxt[n:])
    return next_text

def transcription_prompt(initial_prompt):
    glossary = initial_prompt.strip()
    if glossary.lower().startswith("glossary:"):
        terms = glossary.split(":", 1)[1].strip().rstrip(".").strip()
        if terms:
            return "transcribe the speech to text with proper punctuation and capitalization. Keywords: " + terms
    return "transcribe the speech with proper punctuation and capitalization."

def transcribe_chunk(model, processor, tokenizer, audio, prompt, device):
    import torch
    chat = [{"role": "user", "content": "<|audio|>" + prompt}]
    rendered = tokenizer.apply_chat_template(chat, tokenize=False, add_generation_prompt=True)
    wav = torch.from_numpy(audio).unsqueeze(0)
    inputs = processor(rendered, wav, device=str(device), return_tensors="pt").to(device)
    outputs = model.generate(**inputs, max_new_tokens=4096, do_sample=False, num_beams=1)
    input_count = inputs["input_ids"].shape[-1]
    generated = outputs[0, input_count:].unsqueeze(0)
    decoded = tokenizer.batch_decode(generated, add_special_tokens=False, skip_special_tokens=True)
    return decoded[0].strip() if decoded else ""

def main():
    args = parse_args()
    import torch
    from transformers import AutoModelForSpeechSeq2Seq, AutoProcessor

    requested = args.device
    device = torch.device("cuda:0" if requested in ("auto", "cuda", "") and torch.cuda.is_available() else "cpu")
    dtype = torch.bfloat16 if device.type == "cuda" else torch.float32
    emit(0, 0, f"loading {args.model} on {device}")
    processor = AutoProcessor.from_pretrained(args.model)
    tokenizer = processor.tokenizer
    model = AutoModelForSpeechSeq2Seq.from_pretrained(args.model, torch_dtype=dtype)
    model.to(device).eval()
    if args.prepare:
        emit(1, 1, "ready")
        return

    data = load_audio_16k_mono(args.audio)
    sr = 16000
    windows = list(chunk_indices(len(data), sr))
    prompt = transcription_prompt(args.initial_prompt)
    segments = []
    for index, (start, end) in enumerate(windows):
        emit(index, len(windows), "transcribing (granite)")
        if end - start < int(0.5 * sr):
            continue
        text = transcribe_chunk(model, processor, tokenizer, data[start:end], prompt, device)
        if text and segments:
            text = dedup_overlap(segments[-1][2], text)
        if text:
            segments.append((start / sr, end / sr, text))
        if device.type == "cuda":
            torch.cuda.empty_cache()
    write_outputs(args.out, segments)
    emit(len(windows), len(windows), "done")

if __name__ == "__main__":
    main()
"#;

/// MOSS jointly generates transcript text, timestamps, and speaker labels. Its
/// helper package turns the native segment format into the shared SRT contract.
const MOSS_TRANSCRIBE_DIARIZE_PY: &str = r#"# /// script
# requires-python = ">=3.10,<3.13"
# dependencies = ["moss-transcribe-diarize @ git+https://github.com/OpenMOSS/MOSS-Transcribe-Diarize.git", "torch", "transformers", "soundfile", "numpy", "accelerate"]
# ///
import shutil, tempfile
COMMON

DEFAULT_PROMPT = "Transcribe the audio. For each segment, start with the timestamp and speaker ID ([S01], [S02], [S03], ...), then the spoken text, and end with the segment timestamp."
PLAIN_PROMPT = "Transcribe the audio as text."

def transcription_prompt(initial_prompt, diarize):
    prompt = DEFAULT_PROMPT if diarize else PLAIN_PROMPT
    glossary = initial_prompt.strip()
    if glossary.lower().startswith("glossary:"):
        terms = glossary.split(":", 1)[1].strip().rstrip(".").strip()
        if terms:
            return prompt + " Hotwords: " + terms
    return prompt

def chunk_indices(total_samples, sr, window_s=4800.0, overlap_s=2.0):
    win = int(window_s * sr); overlap = int(overlap_s * sr)
    step = max(win - overlap, 1)
    start = 0
    while start < total_samples:
        yield start, min(start + win, total_samples)
        if start + win >= total_samples:
            break
        start += step

def speaker_text(segment):
    label = str(getattr(segment, "speaker", "")).strip()
    text = str(getattr(segment, "text", "")).strip()
    if label and not label.startswith("["):
        label = "[" + label + "]"
    return (label + " " + text).strip()

def parse_segments(text, offset, duration, parse_transcript, diarize):
    if not diarize:
        return [(offset, offset + duration, text.strip())]
    try:
        parsed = parse_transcript(text)
    except Exception:
        parsed = []
    segments = []
    for segment in parsed:
        start = float(getattr(segment, "start", getattr(segment, "start_time", 0)))
        end = float(getattr(segment, "end", getattr(segment, "end_time", start)))
        rendered = speaker_text(segment)
        if rendered:
            segments.append((offset + start, offset + max(end, start), rendered))
    return segments or [(offset, offset + duration, text.strip())]

def main():
    args = parse_args()
    import torch, soundfile as sf
    from transformers import AutoModelForCausalLM, AutoProcessor
    from moss_transcribe_diarize import parse_transcript
    from moss_transcribe_diarize.inference_utils import build_transcription_messages, generate_transcription

    requested = args.device
    device = torch.device("cuda:0" if requested in ("auto", "cuda", "") and torch.cuda.is_available() else "cpu")
    dtype = torch.bfloat16 if device.type == "cuda" else torch.float32
    emit(0, 0, f"loading {args.model} on {device}")
    model = AutoModelForCausalLM.from_pretrained(
        args.model, trust_remote_code=True, torch_dtype=dtype
    ).to(device).eval()
    processor = AutoProcessor.from_pretrained(args.model, trust_remote_code=True)
    if args.prepare:
        emit(1, 1, "ready")
        return

    data, sr = sf.read(args.audio, dtype="float32", always_2d=False)
    if getattr(data, "ndim", 1) > 1:
        data = data.mean(axis=1)
    windows = list(chunk_indices(len(data), sr))
    prompt = transcription_prompt(args.initial_prompt, args.diarize)
    temp_dir = tempfile.mkdtemp(prefix="ss_moss_")
    segments = []
    try:
        for index, (start, end) in enumerate(windows):
            emit(index, len(windows), "transcribing (moss)")
            if end - start < int(0.5 * sr):
                continue
            clip = f"{temp_dir}/m{index}.wav"
            sf.write(clip, data[start:end], sr)
            messages = build_transcription_messages(clip, prompt=prompt)
            result = generate_transcription(
                model, processor, messages, max_new_tokens=65536,
                do_sample=False, device=device, dtype=dtype,
            )
            raw_text = result.get("text", "")
            segments.extend(
                parse_segments(
                    raw_text, start / sr, (end - start) / sr, parse_transcript, args.diarize
                )
            )
            if device.type == "cuda":
                torch.cuda.empty_cache()
    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)
    write_outputs(args.out, segments)
    emit(len(windows), len(windows), "done")

if __name__ == "__main__":
    main()
"#;

/// NVIDIA NeMo bridge — handles both `parakeet` (ASRModel) and `canary`
/// (SALM speech-LLM). Long audio is processed in windows.
const NEMO_PY: &str = r#"# /// script
# requires-python = ">=3.10,<3.13"
# dependencies = ["nemo_toolkit[asr]>=2.2.0", "soundfile", "numpy", "cuda-python"]
# ///
import sys, math
COMMON
def load_audio_16k_mono(path):
    import soundfile as sf
    import numpy as np
    data, sr = sf.read(path, dtype="float32", always_2d=False)
    if getattr(data, "ndim", 1) > 1:
        data = data.mean(axis=1)
    if sr != 16000:
        # resample via linear interpolation (dependency-free)
        import numpy as np
        n = int(round(len(data) * 16000 / sr))
        if n > 0:
            x_old = np.linspace(0, 1, num=len(data), endpoint=False)
            x_new = np.linspace(0, 1, num=n, endpoint=False)
            data = np.interp(x_new, x_old, data).astype("float32")
        sr = 16000
    return data, sr

def chunk_indices(total_samples, sr, window_s, overlap_s):
    win = int(window_s * sr); ov = int(overlap_s * sr)
    step = max(win - ov, 1)
    start = 0
    while start < total_samples:
        yield start, min(start + win, total_samples)
        if start + win >= total_samples:
            break
        start += step

def run_parakeet(args):
    import nemo.collections.asr as nemo_asr, soundfile as sf, tempfile, os
    emit(0, 0, f"loading {args.model}")
    m = nemo_asr.models.ASRModel.from_pretrained(model_name=args.model)
    # Reduce encoder peak memory on long inputs (the subsampling conv is the
    # usual culprit for CUDA OOM on multi-minute audio).
    try:
        m.change_subsampling_conv_chunking_factor(1)
    except Exception:
        pass
    # The TDT greedy decoder defaults to a CUDA-graph implementation that throws
    # "CUDA error: an illegal memory access" on many driver/toolkit combos.
    # Disable it — decoding stays on-GPU and fast, just without graph capture.
    try:
        from omegaconf import open_dict
        dcfg = m.cfg.decoding
        with open_dict(dcfg):
            if "greedy" not in dcfg or dcfg.greedy is None:
                dcfg.greedy = {}
            dcfg.greedy.use_cuda_graph_decoder = False
        m.change_decoding_strategy(dcfg)
    except Exception as e:
        emit(0, 0, f"note: could not disable cuda-graph decoder ({e})")
    try:
        import torch
    except Exception:
        torch = None
    data, sr = load_audio_16k_mono(args.audio)
    # Transcribe in bounded windows so VRAM use stays flat regardless of the
    # total length (a full 3-hour file would otherwise OOM the GPU).
    windows = list(chunk_indices(len(data), sr, 300.0, 2.0))
    n = len(windows)
    tmp = tempfile.mkdtemp(prefix="ss_parakeet_")
    segs = []
    for i, (a, b) in enumerate(windows):
        emit(i, n, "transcribing (parakeet)")
        off = a / sr
        # Skip degenerate tail windows that are too short to subsample.
        if (b - a) < int(0.5 * sr):
            continue
        clip = os.path.join(tmp, f"p{i}.wav")
        sf.write(clip, data[a:b], sr)
        res = m.transcribe([clip], timestamps=True)
        r = res[0]
        ts = getattr(r, "timestamp", None)
        if ts and ts.get("segment"):
            for s in ts["segment"]:
                segs.append((off + s["start"], off + s["end"], s["segment"]))
        else:
            text = getattr(r, "text", str(r))
            if text:
                segs.append((off, off, text))
        if torch is not None:
            try:
                torch.cuda.empty_cache()
            except Exception:
                pass
    write_outputs(args.out, segs)
    emit(n, n, "done")

def run_canary(args):
    import numpy as np, soundfile as sf, tempfile, os
    from nemo.collections.speechlm2.models import SALM
    emit(0, 0, f"loading {args.model}")
    model = SALM.from_pretrained(args.model)
    data, sr = load_audio_16k_mono(args.audio)
    windows = list(chunk_indices(len(data), sr, 30.0, 1.0))
    n = len(windows)
    segs = []
    tmp = tempfile.mkdtemp(prefix="ss_canary_")
    for i, (a, b) in enumerate(windows):
        emit(i, n, "transcribing (canary)")
        clip = os.path.join(tmp, f"c{i}.wav")
        sf.write(clip, data[a:b], sr)
        ids = model.generate(
            prompts=[[{"role": "user",
                       "content": f"Transcribe the following: {model.audio_locator_tag}",
                       "audio": [clip]}]],
            max_new_tokens=256,
        )
        text = model.tokenizer.ids_to_text(ids[0].cpu())
        segs.append((a / sr, b / sr, text))
    write_outputs(args.out, segs)
    emit(n, n, "done")

def main():
    args = parse_args()
    if args.prepare:
        emit(0, 0, f"downloading {args.model}")
        if args.arch == "canary":
            from nemo.collections.speechlm2.models import SALM
            SALM.from_pretrained(args.model)
        else:
            import nemo.collections.asr as nemo_asr
            nemo_asr.models.ASRModel.from_pretrained(model_name=args.model)
        emit(1, 1, "ready")
        return
    if args.arch == "canary":
        run_canary(args)
    else:
        run_parakeet(args)

if __name__ == "__main__":
    main()
"#;

/// Mistral Voxtral bridge (Transformers). Long audio is windowed at ~25 min.
const VOXTRAL_PY: &str = r#"# /// script
# requires-python = ">=3.10,<3.13"
# dependencies = ["transformers>=4.54.0", "mistral-common[audio]>=1.8.1", "torch", "soundfile", "numpy", "accelerate", "librosa"]
# ///
import sys
COMMON
def main():
    args = parse_args()
    import torch, soundfile as sf, numpy as np, tempfile, os
    from transformers import VoxtralForConditionalGeneration, AutoProcessor
    device = args.device
    if device == "auto":
        device = "cuda" if torch.cuda.is_available() else "cpu"
    dtype = torch.bfloat16 if device == "cuda" else torch.float32
    emit(0, 0, f"loading {args.model} on {device}")
    processor = AutoProcessor.from_pretrained(args.model)
    model = VoxtralForConditionalGeneration.from_pretrained(
        args.model, torch_dtype=dtype, device_map=device
    )
    if args.prepare:
        emit(1, 1, "ready")
        return
    data, sr = sf.read(args.audio, dtype="float32", always_2d=False)
    if getattr(data, "ndim", 1) > 1:
        data = data.mean(axis=1)
    dur = len(data) / sr if sr else 0
    win = int(25 * 60 * sr)  # 25-minute windows
    tmp = tempfile.mkdtemp(prefix="ss_voxtral_")
    lang = None if args.language in ("auto", "") else args.language
    segs = []
    starts = list(range(0, max(len(data), 1), win))
    n = len(starts)
    for i, a in enumerate(starts):
        b = min(a + win, len(data))
        emit(i, n, "transcribing (voxtral)")
        clip = os.path.join(tmp, f"v{i}.wav")
        sf.write(clip, data[a:b], sr)
        inputs = processor.apply_transcription_request(
            language=lang or "en", audio=clip, model_id=args.model
        )
        inputs = inputs.to(device, dtype=dtype)
        outputs = model.generate(**inputs, max_new_tokens=8192)
        decoded = processor.batch_decode(
            outputs[:, inputs.input_ids.shape[1]:], skip_special_tokens=True
        )
        text = decoded[0] if decoded else ""
        segs.append((a / sr, b / sr, text))
    write_outputs(args.out, segs)
    emit(n, n, "done")

if __name__ == "__main__":
    main()
"#;
