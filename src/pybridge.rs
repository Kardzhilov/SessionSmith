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
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin}"))
        .output()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Ok(PathBuf::from(s));
        }
    }
    bail!("{bin} not found")
}

/// Directory where bridge scripts are materialised.
fn bridge_dir() -> Result<PathBuf> {
    let base = dirs::cache_dir().ok_or_else(|| anyhow!("no cache dir"))?;
    let dir = base.join("sessionsmith").join("bridges");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
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
        AsrEngine::WhisperCpp => bail!("whisper.cpp does not use the Python bridge"),
    };
    // Inline the shared helpers where the `COMMON` marker line appears.
    let assembled = template.replacen("\nCOMMON\n", &format!("\n{COMMON_PY}\n"), 1);
    Ok((name, assembled))
}

/// Run transcription for a bridge engine. On success `<out_prefix>.txt` (and
/// `.srt`) exist on disk.
pub fn run_asr(
    engine: AsrEngine,
    model_ref: &str,
    audio: &Path,
    out_prefix: &Path,
    device: &str,
    language: &str,
) -> Result<()> {
    let (name, contents) = script_for(engine)?;
    let script = write_script(name, &contents)?;
    let uv = require_uv(engine)?;

    // NeMo (lhotse) and Voxtral are fragile about input formats — they expect a
    // decodable 16 kHz mono WAV. Normalise via ffmpeg first; faster-whisper
    // decodes internally so it keeps the original file.
    let temp_wav = match engine {
        AsrEngine::Parakeet | AsrEngine::CanaryQwen | AsrEngine::Voxtral => normalize_audio(audio),
        AsrEngine::FasterWhisper | AsrEngine::WhisperCpp => None,
    };
    let effective_audio = temp_wav.as_deref().unwrap_or(audio);

    let mut cmd = Command::new(&uv);
    cmd.arg("run").arg("--quiet").arg(&script);
    cmd.arg("--audio").arg(effective_audio);
    cmd.arg("--out").arg(out_prefix);
    cmd.args(["--model", model_ref]);
    cmd.args(["--device", device]);
    cmd.args(["--language", language]);
    if let Some(arch) = arch_of(engine) {
        cmd.args(["--arch", arch]);
    }
    crate::ui::info(&format!(
        "{} · preparing environment with uv (first run downloads dependencies)…",
        engine.label()
    ));
    let res = stream_uv(engine, &mut cmd);
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
fn normalize_audio(audio: &Path) -> Option<PathBuf> {
    let stem = audio
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("audio");
    let mut out = std::env::temp_dir();
    out.push(format!("ss_bridge_{}_{}.wav", stem, std::process::id()));
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(audio)
        .args(["-ar", "16000", "-ac", "1"])
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    if status.success() {
        Some(out)
    } else {
        None
    }
}

/// Download the environment + model weights for a bridge engine without
/// transcribing (the in-app "prepare/install" action).
pub fn run_asr_prepare(engine: AsrEngine, model_ref: &str, device: &str) -> Result<()> {
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
    stream_uv(engine, &mut cmd)
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
fn stream_uv(engine: AsrEngine, cmd: &mut Command) -> Result<()> {
    cmd.env("PYTHONUNBUFFERED", "1");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawning uv for {}", engine.label()))?;

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
    let stderr_text = stderr_handle.join().unwrap_or_default();
    let status = status.with_context(|| "waiting on uv bridge process")?;

    if !status.success() {
        let tail: String = stderr_text
            .lines()
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        bail!("{} failed:\n{tail}", engine.label());
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
    ap.add_argument("--arch", default="")
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
def preload_cuda_libs():
    # CTranslate2 dlopen()s libcublas/libcudnn lazily; make the pip-installed
    # NVIDIA libs discoverable and preload them globally so the GPU path works.
    try:
        import nvidia, ctypes
        base = os.path.dirname(nvidia.__file__)
        libdirs = [os.path.join(base, s) for s in ("cublas/lib", "cudnn/lib")]
        libdirs = [d for d in libdirs if os.path.isdir(d)]
        if libdirs:
            os.environ["LD_LIBRARY_PATH"] = os.pathsep.join(
                libdirs + [os.environ.get("LD_LIBRARY_PATH", "")])
        for d in libdirs:
            for so in sorted(glob.glob(os.path.join(d, "*.so*"))):
                try:
                    ctypes.CDLL(so, mode=ctypes.RTLD_GLOBAL)
                except OSError:
                    pass
    except Exception:
        pass

def cuda_available():
    try:
        import ctranslate2
        return ctranslate2.get_cuda_device_count() > 0
    except Exception:
        return False

def transcribe_with(model_name, device, audio, language):
    from faster_whisper import WhisperModel
    compute = "float16" if device == "cuda" else "int8"
    emit(0, 0, f"loading {model_name} on {device}")
    model = WhisperModel(model_name, device=device, compute_type=compute)
    lang = None if language in ("auto", "") else language
    segments, info = model.transcribe(audio, language=lang, vad_filter=True)
    total = getattr(info, "duration", 0) or 0
    out = []
    for seg in segments:
        out.append((seg.start, seg.end, seg.text))
        emit(round(seg.end, 2), round(total, 2), "transcribing")
    return out, total

def main():
    args = parse_args()
    device = args.device
    if device in ("auto", ""):
        preload_cuda_libs()
        device = "cuda" if cuda_available() else "cpu"
    elif device == "cuda":
        preload_cuda_libs()
    if args.prepare:
        from faster_whisper import WhisperModel
        emit(0, 0, f"downloading {args.model}")
        WhisperModel(args.model, device="cpu", compute_type="int8")
        emit(1, 1, "ready")
        return
    try:
        out, total = transcribe_with(args.model, device, args.audio, args.language)
    except Exception as e:
        if device == "cuda":
            sys.stderr.write(f"CUDA path failed ({e}); retrying on CPU\n")
            out, total = transcribe_with(args.model, "cpu", args.audio, args.language)
        else:
            raise
    write_outputs(args.out, out)
    emit(round(total, 2), round(total, 2), "done")

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
