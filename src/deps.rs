//! Dependency checks: ffmpeg, whisper-cli, whisper model file, LLM backend reachability.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{BackendConfig, GlobalConfig};

#[derive(Debug, Clone)]
pub struct DepStatus {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

pub fn check_ffmpeg() -> DepStatus {
    match which("ffmpeg") {
        Some(p) => DepStatus { name: "ffmpeg".into(), ok: true, detail: p.display().to_string() },
        None => DepStatus {
            name: "ffmpeg".into(),
            ok: false,
            detail: "install via your package manager (apt/brew/winget)".into(),
        },
    }
}

pub fn check_ffprobe() -> DepStatus {
    match which("ffprobe") {
        Some(p) => DepStatus { name: "ffprobe".into(), ok: true, detail: p.display().to_string() },
        None => DepStatus {
            name: "ffprobe".into(),
            ok: false,
            detail: "ships with ffmpeg; install ffmpeg".into(),
        },
    }
}

pub fn check_whisper_cli(custom: Option<&Path>) -> DepStatus {
    // Explicit binary from config
    if let Some(p) = custom {
        if p.exists() {
            let label = asr_label(p);
            return DepStatus { name: label, ok: true, detail: p.display().to_string() };
        }
    }
    // whisper.cpp variants on PATH
    for candidate in ["whisper-cli", "whisper.cpp", "main"] {
        if let Some(p) = which(candidate) {
            return DepStatus { name: "asr (whisper-cli)".into(), ok: true, detail: p.display().to_string() };
        }
    }
    // whisperx in project .venv — no external install required
    let venv_wx = Path::new(".venv/bin/whisperx");
    if venv_wx.exists() {
        return DepStatus { name: "asr (whisperx)".into(), ok: true, detail: venv_wx.display().to_string() };
    }
    // whisperx anywhere on PATH
    if let Some(p) = which("whisperx") {
        return DepStatus { name: "asr (whisperx)".into(), ok: true, detail: p.display().to_string() };
    }
    DepStatus {
        name: "asr".into(),
        ok: false,
        detail: "whisper-cli (whisper.cpp) or whisperx required — see README".into(),
    }
}

fn asr_label(p: &Path) -> String {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.contains("whisperx") { "asr (whisperx)".into() } else { "asr (whisper-cli)".into() }
}

pub fn check_whisper_model(model: &str, cache_dir: &Path) -> DepStatus {
    match crate::models::whisper_path(model, cache_dir) {
        Ok(p) if p.exists() => DepStatus {
            name: format!("whisper model: {model}"),
            ok: true,
            detail: p.display().to_string(),
        },
        Ok(p) => DepStatus {
            name: format!("whisper model: {model}"),
            ok: false,
            detail: format!("missing at {} — run `sessionsmith models pull {model}`", p.display()),
        },
        Err(e) => DepStatus {
            name: format!("whisper model: {model}"),
            ok: false,
            detail: e.to_string(),
        },
    }
}

pub async fn check_backend(g: &GlobalConfig) -> DepStatus {
    let b = &g.backend;
    let label = format!("backend: {}", b.kind);
    match b.kind.as_str() {
        "ollama" => check_ollama(b).await.map(|d| DepStatus { name: label.clone(), ok: true, detail: d })
            .unwrap_or_else(|e| DepStatus { name: label, ok: false, detail: e }),
        "openai" => check_openai_like(b, g).await.map(|d| DepStatus { name: label.clone(), ok: true, detail: d })
            .unwrap_or_else(|e| DepStatus { name: label, ok: false, detail: e }),
        "anthropic" => check_anthropic(b, g).await.map(|d| DepStatus { name: label.clone(), ok: true, detail: d })
            .unwrap_or_else(|e| DepStatus { name: label, ok: false, detail: e }),
        other => DepStatus { name: label, ok: false, detail: format!("unknown backend '{other}'") },
    }
}

async fn check_ollama(b: &BackendConfig) -> std::result::Result<String, String> {
    let base = b.base_url.clone().unwrap_or_else(|| "http://localhost:11434".into());
    let url = format!("{}/api/tags", base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build().map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| format!("unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    Ok(base)
}

async fn check_openai_like(b: &BackendConfig, g: &GlobalConfig) -> std::result::Result<String, String> {
    let base = b.base_url.clone().unwrap_or_else(|| "https://api.openai.com".into());
    let key = g.resolved_api_key().unwrap_or_default();
    if key.is_empty() {
        return Err("missing api_key in global config (or unset env var)".into());
    }
    let url = format!("{}/v1/models", base.trim_end_matches('/'));
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).bearer_auth(&key).send().await
        .map_err(|e| format!("unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    Ok(base)
}

async fn check_anthropic(b: &BackendConfig, g: &GlobalConfig) -> std::result::Result<String, String> {
    let base = b.base_url.clone().unwrap_or_else(|| "https://api.anthropic.com".into());
    let key = g.resolved_api_key().unwrap_or_default();
    if key.is_empty() {
        return Err("missing api_key in global config (or unset env var)".into());
    }
    let url = format!("{}/v1/models", base.trim_end_matches('/'));
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .send().await
        .map_err(|e| format!("unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    Ok(base)
}

fn which(cmd: &str) -> Option<PathBuf> {
    let out = Command::new("which").arg(cmd).output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

pub fn ensure_dirs() -> Result<()> {
    for d in ["audio", "output"] {
        std::fs::create_dir_all(d)?;
    }
    Ok(())
}
