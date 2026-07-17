//! Transcription: delegates to whisper.cpp's `whisper-cli` **or** `whisperx`
//! (whichever is found first), producing `transcripts/<stem>.txt` and `.srt`.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::asr::AsrEngine;
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
    /// In-process whisper.cpp via `whisper-rs` (no external process).
    #[cfg(feature = "local-whisper")]
    Local,
    /// A modern engine run through the `uv` Python bridge (faster-whisper,
    /// NVIDIA Parakeet / Canary, Voxtral). Carries the
    /// engine and its backend model reference (HF / NeMo id).
    Bridge(AsrEngine, String),
    /// Local GGUF ASR through transcribe.cpp.
    TranscribeCpp(String),
}

impl AsrBackend {
    /// Whether this engine needs a downloaded ggml model file.
    fn needs_ggml_model(&self) -> bool {
        match self {
            AsrBackend::WhisperCli(_) => true,
            #[cfg(feature = "local-whisper")]
            AsrBackend::Local => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TranscribeOpts {
    pub model: String,
    pub language: String,
    pub force: bool,
    pub replacements: BTreeMap<String, String>,
    /// Enable speaker diarization (whisperX only). Off by default.
    pub diarize: bool,
    /// Run an ffmpeg silence-removal (VAD) pre-pass before ASR.
    pub vad: bool,
}

impl TranscribeOpts {
    /// Options for a plain transcription, diarization/VAD taken from config.
    pub fn from_config(model: String, language: String, force: bool, g: &GlobalConfig) -> Self {
        Self {
            model,
            language,
            force,
            replacements: BTreeMap::new(),
            diarize: g.asr.diarize,
            vad: g.asr.vad,
        }
    }
}

fn apply_replacements(text: &str, replacements: &BTreeMap<String, String>) -> String {
    let mut ordered: Vec<(&String, &String)> = replacements.iter().collect();
    ordered.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
    ordered.into_iter().fold(text.to_string(), |text, (from, to)| {
        if from.is_empty() { text } else { text.replace(from, to) }
    })
}

fn correct_transcript_files(
    txt: &Path,
    srt: &Path,
    replacements: &BTreeMap<String, String>,
) -> Result<()> {
    if replacements.is_empty() {
        return Ok(());
    }
    for path in [txt, srt] {
        if path.exists() {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading transcript corrections input: {}", path.display()))?;
            std::fs::write(path, apply_replacements(&text, replacements))
                .with_context(|| format!("writing transcript corrections: {}", path.display()))?;
        }
    }
    crate::ui::info(&format!(
        "applied {} campaign transcript correction(s)",
        replacements.len()
    ));
    Ok(())
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

    // Reuse an existing transcript only when the model matches (or no metadata
    // is recorded, for backward compatibility). A different ASR model forces a
    // re-transcription even without `--force`.
    let prior_meta = crate::meta::load(out_dir, &stem);
    let same_model = prior_meta
        .as_ref()
        .map(|m| m.model == opts.model)
        .unwrap_or(true);
    if !opts.force && out_txt.exists() && out_srt.exists() && same_model {
        crate::ui::ok(&format!("transcript exists: {}", out_txt.display()));
        return Ok(TranscribeOutput { txt: out_txt, srt: out_srt });
    }

    // Transcription is usually GPU-bound; make sure the LLM backend isn't
    // holding VRAM (e.g. an Ollama model left resident) before we start.
    crate::llm::free_vram(g).await;

    let backend = resolve_asr_backend(g, opts)?;

    // Records model + source audio after a successful transcription, so a
    // re-run can skip transcription (same model) and the player can seek.
    let write_meta = |engine: &str| {
        let _ = crate::meta::save(
            out_dir,
            &stem,
            &crate::meta::SessionMeta {
                model: opts.model.clone(),
                engine: engine.to_string(),
                language: opts.language.clone(),
                source_audio: Some(audio.to_path_buf()),
                created: crate::meta::now_secs(),
            },
        );
    };

    // whisper.cpp (external or in-process) needs a ggml model file; whisperx
    // manages its own model cache via Hugging Face.
    let model_path_opt = if backend.needs_ggml_model() {
        let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
        Some(models::ensure_whisper(&opts.model, &cache).await?)
    } else {
        None
    };

    // Optional VAD (silence-removal) pre-pass. Produces a temp file with the
    // same stem (so whisperx names its outputs correctly) that is fed to ASR.
    let hf_token = g.resolved_hf_token();
    let asr_input: PathBuf = if opts.vad {
        match apply_vad(audio, &stem) {
            Ok(p) => p,
            Err(e) => {
                crate::ui::warn(&format!("VAD pre-pass failed ({e}); using original audio"));
                audio.to_path_buf()
            }
        }
    } else {
        audio.to_path_buf()
    };
    let vad_temp = if asr_input != *audio { Some(asr_input.clone()) } else { None };

    // Modern engines run through the uv Python bridge and write the outputs
    // themselves.
    if let AsrBackend::Bridge(engine, model_ref) = &backend {
        let device = g.asr.device.clone().unwrap_or_else(|| "auto".to_string());
        let prefix = out_dir.join(&stem);
        let pb = crate::ui::spinner(&format!(
            "transcribing {stem} with {} ({})",
            engine.label(),
            model_ref
        ));
        let res = crate::pybridge::run_asr(
            *engine,
            model_ref,
            &asr_input,
            &prefix,
            &device,
            &opts.language,
        );
        pb.finish_and_clear();
        if let Some(tmp) = &vad_temp {
            std::fs::remove_file(tmp).ok();
        }
        res?;
        correct_transcript_files(&out_txt, &out_srt, &opts.replacements)?;
        crate::ui::info(&format!("ASR engine: {}", engine.label()));
        crate::ui::ok(&format!("wrote {}", out_txt.display()));
        if out_srt.exists() {
            crate::ui::ok(&format!("wrote {}", out_srt.display()));
        }
        write_meta(engine.label());
        return Ok(TranscribeOutput { txt: out_txt, srt: out_srt });
    }

    if let AsrBackend::TranscribeCpp(model_id) = &backend {
        let cache = models::gguf_asr_cache_dir()?;
        let model_path = models::ensure_gguf_asr(model_id, &cache).await?;
        let prefix = out_dir.join(&stem);
        let device = g.asr.device.as_deref();
        let pb = crate::ui::spinner(&format!(
            "transcribing {stem} with transcribe.cpp ({model_id})"
        ));
        let res = crate::transcribe_cpp::run_asr(
            &model_path,
            &asr_input,
            &prefix,
            &opts.language,
            device,
        );
        pb.finish_and_clear();
        if let Some(tmp) = &vad_temp {
            std::fs::remove_file(tmp).ok();
        }
        res?;
        correct_transcript_files(&out_txt, &out_srt, &opts.replacements)?;
        crate::ui::info("ASR engine: transcribe.cpp");
        crate::ui::ok(&format!("wrote {}", out_txt.display()));
        crate::ui::ok(&format!("wrote {}", out_srt.display()));
        write_meta("transcribe.cpp");
        return Ok(TranscribeOutput { txt: out_txt, srt: out_srt });
    }

    // In-process transcription via whisper-rs — handled entirely here.
    #[cfg(feature = "local-whisper")]
    if matches!(backend, AsrBackend::Local) {
        let model_path = model_path_opt
            .as_ref()
            .ok_or_else(|| anyhow!("local engine requires a ggml model"))?;
        let threads = g.asr.threads.unwrap_or_else(|| {
            std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4).min(8)
        }) as i32;
        let use_gpu = !matches!(g.asr.device.as_deref(), Some(d) if d.eq_ignore_ascii_case("cpu"));

        // This build's whisper.cpp may be CPU-only (no `cuda`/`vulkan`/`metal`
        // feature). On a long recording that means hours of CPU work while the
        // GPU sits idle — warn and point at the GPU-accelerated alternative.
        if use_gpu
            && crate::whisper_local::gpu_label().starts_with("cpu")
            && crate::hardware::detect().gpu.is_some()
        {
            crate::ui::warn(
                "in-process whisper is running on CPU (this build has no GPU backend) — \
                 long recordings can take hours. For GPU speed with the same model, pick \
                 the 'faster-whisper large-v3-turbo' engine, or rebuild with `--features cuda`.",
            );
        }

        let pb = crate::ui::spinner(&format!("transcribing {stem} with whisper-{} (in-process)", opts.model));
        let result = crate::whisper_local::transcribe_file(model_path, &asr_input, &opts.language, threads, use_gpu);
        pb.finish_and_clear();

        if let Some(tmp) = &vad_temp {
            std::fs::remove_file(tmp).ok();
        }
        let segments = result?;
        std::fs::write(&out_txt, crate::whisper_local::segments_to_text(&segments))?;
        std::fs::write(&out_srt, crate::whisper_local::segments_to_srt(&segments))?;
        correct_transcript_files(&out_txt, &out_srt, &opts.replacements)?;
        crate::ui::info(&format!("ASR device: {}", crate::whisper_local::gpu_label()));
        crate::ui::ok(&format!("wrote {}", out_txt.display()));
        crate::ui::ok(&format!("wrote {}", out_srt.display()));
        write_meta("whisper.cpp");
        return Ok(TranscribeOutput { txt: out_txt, srt: out_srt });
    }

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
                .arg("-f").arg(&asr_input)
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
            let via_uvx = binary.to_str() == Some("uvx:whisperx");

            // whisperX expects a whisper model name; if the configured default
            // is a non-whisper engine id (e.g. `parakeet-v3`), fall back to
            // `large-v3` for the diarized transcription.
            let wx_model: String = if crate::asr::engine_of(&opts.model).is_bridge() {
                "large-v3".to_string()
            } else {
                opts.model.clone()
            };

            let free_mb = free_vram_mb();
            let (device, compute): (&str, &str) = match g.asr.device.as_deref() {
                Some(d) if d.eq_ignore_ascii_case("cuda") => ("cuda", "float16"),
                Some(d) if d.eq_ignore_ascii_case("cpu") => ("cpu", "int8"),
                _ if free_mb >= 4096 => ("cuda", "float16"),
                _ => ("cpu", "int8"),
            };

            let run_whisperx = |device: &str, compute: &str| -> std::io::Result<std::process::Output> {
                let mut cmd = if via_uvx {
                    // `uv tool run whisperx …` — ephemeral, auto-installed env.
                    let uv = crate::pybridge::uv_path()
                        .unwrap_or_else(|| PathBuf::from("uv"));
                    let mut c = Command::new(uv);
                    c.args(["tool", "run", "whisperx"]);
                    c
                } else if use_module {
                    let mut c = Command::new(&python);
                    c.args(["-m", "whisperx"]);
                    c
                } else {
                    Command::new(binary)
                };
                cmd.arg(&asr_input)
                    .args(["--model", &wx_model])
                    .args(["--output_dir", out_dir.to_str().unwrap()])
                    .args(["--output_format", "all"])
                    .args(["--device", device, "--compute_type", compute])
                    .env_remove("PYTHONPATH"); // prevent stale PYTHONPATH from leaking in
                if opts.diarize {
                    // Diarization needs word alignment, so we must NOT pass
                    // --no_align here. A HF token is required for the pyannote
                    // models; pass it when configured.
                    cmd.arg("--diarize");
                    // Pin the diarization pipeline to community-1 (much better
                    // than 3.1, unlimited speakers) rather than relying on the
                    // whisperX default.
                    let diar = crate::asr::diarize_spec(crate::asr::DEFAULT_DIARIZE);
                    cmd.args(["--diarize_model", diar.model_ref]);
                    if let Some(tok) = &hf_token {
                        cmd.args(["--hf_token", tok]);
                    }
                } else {
                    cmd.arg("--no_align");
                }
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
        #[cfg(feature = "local-whisper")]
        AsrBackend::Local => unreachable!("local engine handled before this match"),
        AsrBackend::Bridge(..) => unreachable!("bridge engine handled before this match"),
        AsrBackend::TranscribeCpp(..) => unreachable!("transcribe.cpp engine handled before this match"),
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
    correct_transcript_files(&out_txt, &out_srt, &opts.replacements)?;
    if opts.diarize {
        crate::ui::info("diarization enabled (whisperX) — transcript includes speaker labels");
    }
    // Remove the temporary VAD-processed file if one was created.
    if let Some(tmp) = &vad_temp {
        std::fs::remove_file(tmp).ok();
    }
    crate::ui::ok(&format!("wrote {}", out_txt.display()));
    if out_srt.exists() {
        crate::ui::ok(&format!("wrote {}", out_srt.display()));
    }
    let meta_engine = match &backend {
        AsrBackend::WhisperX(_) => "whisperx",
        _ => "whisper.cpp",
    };
    write_meta(meta_engine);
    let _ = binary_path; // used only for error context above
    Ok(TranscribeOutput { txt: out_txt, srt: out_srt })
}

/// Apply a Voice-Activity-Detection style silence-removal pass with ffmpeg,
/// writing a 16 kHz mono file with the same stem into a temp directory. This
/// trims long gaps so ASR spends time only on speech.
fn apply_vad(audio: &Path, stem: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("sessionsmith_vad");
    std::fs::create_dir_all(&dir)?;
    let out = dir.join(format!("{stem}.wav"));
    let pb = crate::ui::spinner("VAD: removing silence with ffmpeg");
    // Trim leading/trailing and internal silences longer than ~1s below -35dB.
    let filter = "silenceremove=start_periods=1:start_silence=0.3:start_threshold=-35dB:\
                  stop_periods=-1:stop_silence=1:stop_threshold=-35dB";
    let status = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(audio)
        .args(["-af", filter, "-ar", "16000", "-ac", "1"])
        .arg(&out)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| "ffmpeg not found — install ffmpeg")?;
    pb.finish_and_clear();
    if !status.success() || !out.exists() {
        bail!("ffmpeg silence-removal failed");
    }
    crate::ui::ok(&format!("VAD → {}", out.display()));
    Ok(out)
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

/// Locate the ASR engine to use, honouring the `[asr] engine` preference and
/// the diarization requirement.
///
/// - Diarization is only available via whisperX, so it forces that engine.
/// - `engine = "local"` uses the in-process whisper-rs engine (requires the
///   `local-whisper` build feature).
/// - `engine = "whisper-cli"` / `"whisperx"` force an external engine.
/// - Unset / `"auto"` prefers the in-process engine when compiled in, then
///   falls back to an external binary.
fn resolve_asr_backend(g: &GlobalConfig, opts: &TranscribeOpts) -> Result<AsrBackend> {
    // Diarization is a whisperX-only capability.
    if opts.diarize {
        return resolve_whisperx(g).ok_or_else(|| anyhow!(
            "diarization requires whisperX (pyannote community-1).\n  \
             Install `uv` so it can run automatically \
             (curl -LsSf https://astral.sh/uv/install.sh | sh), or install whisperX \
             manually. It also needs a Hugging Face token in [asr] hf_token and \
             acceptance of the community-1 model terms."
        ));
    }

    // Modern engines are selected by model id. Python-ecosystem engines run
    // through the `uv` bridge; GGUF ASR models run through transcribe.cpp.
    let engine = crate::asr::engine_of(&opts.model);
    if engine == AsrEngine::TranscribeCpp {
        return Ok(AsrBackend::TranscribeCpp(opts.model.clone()));
    }
    if engine.is_bridge() {
        let model_ref = crate::asr::find(&opts.model)
            .map(|m| m.model_ref.to_string())
            .unwrap_or_else(|| opts.model.clone());
        return Ok(AsrBackend::Bridge(engine, model_ref));
    }

    match g.asr.engine.as_deref().map(|s| s.to_lowercase()) {
        Some(ref e) if e == "local" => {
            #[cfg(feature = "local-whisper")]
            { Ok(AsrBackend::Local) }
            #[cfg(not(feature = "local-whisper"))]
            { bail!("engine = \"local\" but this binary was built without the `local-whisper` feature") }
        }
        Some(ref e) if e == "whisper-cli" => {
            resolve_whisper_cli(g).ok_or_else(|| anyhow!("whisper-cli not found (engine = \"whisper-cli\")"))
        }
        Some(ref e) if e == "whisperx" => {
            resolve_whisperx(g).ok_or_else(|| anyhow!("whisperx not found (engine = \"whisperx\")"))
        }
        _ => {
            // auto: prefer the in-process engine when available.
            #[cfg(feature = "local-whisper")]
            { Ok(AsrBackend::Local) }
            #[cfg(not(feature = "local-whisper"))]
            {
                if let Some(b) = resolve_whisper_cli(g) {
                    return Ok(b);
                }
                if let Some(b) = resolve_whisperx(g) {
                    return Ok(b);
                }
                bail!(
                    "No ASR engine found.\n\n\
                     Option A — build with the in-process engine (default):\n  \
                       cargo build --release   # needs clang/libclang + cmake\n\n\
                     Option B — whisper.cpp binary:\n  \
                       git clone https://github.com/ggerganov/whisper.cpp\n  \
                       cd whisper.cpp && make -j && sudo cp main /usr/local/bin/whisper-cli\n\n\
                     Option C — whisperx (needed for diarization):\n  \
                       python -m venv .venv && .venv/bin/pip install whisperx\n\n\
                     Or set an explicit path in ~/.config/sessionsmith/config.toml:\n  \
                       [asr]\n  \
                       binary = \"/path/to/whisper-cli\"  # or whisperx\n"
                )
            }
        }
    }
}

/// Resolve a whisper.cpp `whisper-cli` engine, if present.
fn resolve_whisper_cli(g: &GlobalConfig) -> Option<AsrBackend> {
    // Explicit non-whisperx binary from config.
    if let Some(b) = &g.asr.binary {
        if b.exists() {
            let name = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.contains("whisperx") {
                return Some(AsrBackend::WhisperCli(b.clone()));
            }
        }
    }
    for candidate in ["whisper-cli", "whisper.cpp", "main"] {
        if let Some(p) = path_of(candidate) {
            return Some(AsrBackend::WhisperCli(p));
        }
    }
    for extra in ["./whisper.cpp/build/bin/whisper-cli", "./build/bin/whisper-cli"] {
        let p = Path::new(extra);
        if p.exists() {
            return Some(AsrBackend::WhisperCli(p.to_path_buf()));
        }
    }
    None
}

/// Resolve a whisperX engine, if present.
fn resolve_whisperx(g: &GlobalConfig) -> Option<AsrBackend> {
    if let Some(b) = &g.asr.binary {
        if b.exists() {
            let name = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.contains("whisperx") {
                return Some(AsrBackend::WhisperX(b.clone()));
            }
        }
    }
    let venv_wx = Path::new(".venv/bin/whisperx");
    if venv_wx.exists() {
        let abs = std::fs::canonicalize(venv_wx).unwrap_or_else(|_| venv_wx.to_path_buf());
        return Some(AsrBackend::WhisperX(abs));
    }
    if let Some(b) = path_of("whisperx") {
        return Some(AsrBackend::WhisperX(b));
    }
    // Last resort: run whisperx ephemerally via `uv` (auto-installs on first
    // use, like the other bridge engines). Uses pyannote community-1 by default.
    if crate::pybridge::uv_path().is_some() {
        return Some(AsrBackend::WhisperX(PathBuf::from("uvx:whisperx")));
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_replacements_apply_longest_first() {
        let replacements = BTreeMap::from([
            ("the Mosses".to_string(), "Damasus".to_string()),
            ("the Mosses or Ports".to_string(), "Damasus".to_string()),
        ]);
        assert_eq!(
            apply_replacements("the Mosses or Ports and the Mosses", &replacements),
            "Damasus and Damasus"
        );
    }
}
