//! Model registry: whisper ggml downloads + Ollama pull delegation.

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub struct WhisperModel {
    pub id: &'static str,
    pub filename: &'static str,
    /// SHA-256 of the ggml file (lowercase hex). Empty string disables verification.
    pub sha256: &'static str,
}

// Hashes intentionally left empty by default; verification runs only when set.
// Users can pin hashes in their own forks if they want strict supply-chain checks.
pub const WHISPER_MODELS: &[WhisperModel] = &[
    WhisperModel { id: "tiny",            filename: "ggml-tiny.bin",            sha256: "" },
    WhisperModel { id: "base",            filename: "ggml-base.bin",            sha256: "" },
    WhisperModel { id: "small",           filename: "ggml-small.bin",           sha256: "" },
    WhisperModel { id: "medium",          filename: "ggml-medium.bin",          sha256: "" },
    WhisperModel { id: "large-v3",        filename: "ggml-large-v3.bin",        sha256: "" },
    WhisperModel { id: "large-v3-turbo",  filename: "ggml-large-v3-turbo.bin",  sha256: "" },
];

const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

pub fn whisper_cache_dir(override_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(d) = override_dir {
        return Ok(d.to_path_buf());
    }
    let base = dirs::cache_dir()
        .ok_or_else(|| anyhow!("could not resolve XDG cache dir"))?;
    Ok(base.join("sessionsmith").join("whisper"))
}

pub fn whisper_path(id: &str, cache_dir: &Path) -> Result<PathBuf> {
    let model = WHISPER_MODELS.iter()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow!("unknown whisper model '{id}'"))?;
    Ok(cache_dir.join(model.filename))
}

pub async fn ensure_whisper(id: &str, cache_dir: &Path) -> Result<PathBuf> {
    let path = whisper_path(id, cache_dir)?;
    if path.exists() {
        return Ok(path);
    }
    download_whisper(id, cache_dir).await
}

pub async fn download_whisper(id: &str, cache_dir: &Path) -> Result<PathBuf> {
    let model = WHISPER_MODELS.iter()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow!("unknown whisper model '{id}'"))?;
    std::fs::create_dir_all(cache_dir)?;
    let path = cache_dir.join(model.filename);
    let tmp = cache_dir.join(format!("{}.part", model.filename));
    let url = format!("{HF_BASE}/{}", model.filename);

    let client = reqwest::Client::builder()
        .user_agent("sessionsmith/0.1")
        .build()?;
    let resp = client.get(&url).send().await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        bail!("HTTP {} fetching {url}", resp.status());
    }
    let total = resp.content_length().unwrap_or(0);
    let pb = crate::ui::progress_bar(total, &format!("downloading {}", model.filename));
    let mut stream = resp.bytes_stream();

    use sha2::{Digest, Sha256};
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        pb.set_position(received);
    }
    file.flush().await?;
    drop(file);
    pb.finish_and_clear();

    if !model.sha256.is_empty() {
        let got = hex::encode(hasher.finalize());
        if got != model.sha256 {
            let _ = std::fs::remove_file(&tmp);
            bail!("checksum mismatch for {} (got {got})", model.filename);
        }
    }
    std::fs::rename(&tmp, &path)?;
    crate::ui::ok(&format!("saved {}", path.display()));
    Ok(path)
}

pub fn ollama_pull(name: &str) -> Result<()> {
    crate::ui::info(&format!("ollama pull {name}"));
    let status = Command::new("ollama").args(["pull", name]).status()
        .with_context(|| "ollama not found on PATH")?;
    if !status.success() {
        bail!("ollama pull {name} failed");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Approximate sizes for common Ollama models (bytes). Used as fallback when
// the model hasn't been pulled yet and Ollama's registry can't provide info.
// ---------------------------------------------------------------------------
pub const OLLAMA_KNOWN_SIZES: &[(&str, u64)] = &[
    ("qwen2.5:0.5b",  397_000_000),
    ("qwen2.5:1.5b",  986_000_000),
    ("qwen2.5:3b",  1_900_000_000),
    ("qwen2.5:7b",  4_700_000_000),
    ("qwen2.5:14b", 9_000_000_000),
    ("qwen2.5:32b", 20_000_000_000),
    ("qwen2.5:72b", 47_000_000_000),
    ("llama3.3:70b", 43_000_000_000),
    ("llama3.2:3b",  2_000_000_000),
    ("llama3.2:1b",    738_000_000),
    ("gemma3:4b",    3_300_000_000),
    ("gemma3:12b",   8_100_000_000),
    ("gemma3:27b",  17_000_000_000),
    ("mistral:7b",   4_100_000_000),
    ("phi4:14b",     9_100_000_000),
    ("phi4-mini:3.8b", 2_500_000_000),
];

/// Fetch the actual file size of a whisper ggml model via HTTP HEAD on HuggingFace.
/// Returns `None` on network error or if Content-Length is absent.
pub async fn fetch_hf_size(id: &str) -> Option<u64> {
    let model = WHISPER_MODELS.iter().find(|m| m.id == id)?;
    let url = format!("{HF_BASE}/{}", model.filename);
    let client = reqwest::Client::builder()
        .user_agent("sessionsmith/0.1")
        .timeout(Duration::from_secs(8))
        .build().ok()?;
    let resp = client.head(&url).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    resp.content_length()
}

/// Fetch the size of an Ollama model.
/// Tries the local Ollama registry first (works even for already-pulled models),
/// then falls back to the built-in size table for common models.
pub async fn fetch_ollama_size(name: &str, base_url: &str) -> Option<u64> {
    // Query /api/tags (lists all local models with their sizes)
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build().ok()?;
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    if let Ok(resp) = client.get(&url).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = v.get("models").and_then(|m| m.as_array()) {
                    for m in arr {
                        if m.get("name").and_then(|n| n.as_str()) == Some(name) {
                            if let Some(sz) = m.get("size").and_then(|s| s.as_u64()) {
                                return Some(sz);
                            }
                        }
                    }
                }
            }
        }
    }
    // Fall back to known-sizes table
    OLLAMA_KNOWN_SIZES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Human-readable byte size.
pub fn human_bytes(n: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;
    if n >= GB {
        format!("{:.1} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.0} MB", n as f64 / MB as f64)
    } else {
        format!("{} B", n)
    }
}

pub fn list_local_whisper(cache_dir: &Path) -> Vec<String> {
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(cache_dir) {
        for e in entries.flatten() {
            if let Some(s) = e.file_name().to_str() {
                if s.starts_with("ggml-") && (s.ends_with(".bin") || s.ends_with(".gguf")) {
                    out.push(s.to_string());
                }
            }
        }
    }
    out.sort();
    out
}
