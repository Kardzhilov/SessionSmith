//! Model registry: whisper ggml downloads + Ollama pull delegation.

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, CONTENT_LENGTH, ETAG, LAST_MODIFIED, RANGE};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub struct WhisperModel {
    pub id: &'static str,
    pub filename: &'static str,
    /// SHA-256 of the ggml file (lowercase hex). Empty string disables verification.
    pub sha256: &'static str,
    /// Approximate public release date (`YYYY-MM`) for the "age at a glance" column.
    pub released: &'static str,
    pub params: u64,
    pub langs: &'static str,
    pub languages: &'static [&'static str],
    pub license: &'static str,
    pub note: &'static str,
}

pub const WHISPER_MODELS: &[WhisperModel] = &[
    WhisperModel {
        id: "tiny",
        filename: "ggml-tiny.bin",
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        released: "2022-09",
        params: 39_000_000,
        langs: "99 langs",
        languages: &["Multilingual"],
        license: "MIT",
        note: "Local GGML speech-to-text model",
    },
    WhisperModel {
        id: "base",
        filename: "ggml-base.bin",
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        released: "2022-09",
        params: 74_000_000,
        langs: "99 langs",
        languages: &["Multilingual"],
        license: "MIT",
        note: "tiny + fast; low accuracy, good for smoke tests",
    },
    WhisperModel {
        id: "small",
        filename: "ggml-small.bin",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        released: "2022-09",
        params: 244_000_000,
        langs: "99 langs",
        languages: &["Multilingual"],
        license: "MIT",
        note: "Local GGML speech-to-text model",
    },
    WhisperModel {
        id: "medium",
        filename: "ggml-medium.bin",
        sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        released: "2022-09",
        params: 769_000_000,
        langs: "99 langs",
        languages: &["Multilingual"],
        license: "MIT",
        note: "lighter; good on CPU-only machines",
    },
    WhisperModel {
        id: "large-v3",
        filename: "ggml-large-v3.bin",
        sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
        released: "2023-11",
        params: 1_550_000_000,
        langs: "99 langs",
        languages: &["Multilingual"],
        license: "MIT",
        note: "most accurate whisper; slower than turbo",
    },
    WhisperModel {
        id: "large-v3-turbo",
        filename: "ggml-large-v3-turbo.bin",
        sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        released: "2024-10",
        params: 809_000_000,
        langs: "99 langs",
        languages: &["Multilingual"],
        license: "MIT",
        note: "fast multilingual default; runs on CPU or GPU",
    },
];

const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HfFileMeta {
    url: String,
    #[serde(default)]
    etag: Option<String>,
    #[serde(default)]
    last_modified: Option<String>,
    #[serde(default)]
    content_length: Option<u64>,
    #[serde(default)]
    checked_at: i64,
    #[serde(default)]
    downloaded_at: i64,
}

fn sidecar_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("model");
    path.with_file_name(format!("{name}.ssmeta.json"))
}

fn read_hf_meta_file(sidecar: &Path) -> Option<HfFileMeta> {
    let text = std::fs::read_to_string(sidecar).ok()?;
    serde_json::from_str(&text).ok()
}

fn read_hf_meta(path: &Path) -> Option<HfFileMeta> {
    read_hf_meta_file(&sidecar_path(path))
}

fn write_hf_meta_file(sidecar: &Path, meta: &HfFileMeta) -> Result<()> {
    let tmp = sidecar.with_extension("json.part");
    let text = serde_json::to_string_pretty(meta)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, sidecar)?;
    Ok(())
}

fn write_hf_meta(path: &Path, meta: &HfFileMeta) -> Result<()> {
    write_hf_meta_file(&sidecar_path(path), meta)
}

fn header_string(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn header_content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|n| *n > 0)
}

fn hf_meta_from_headers(url: &str, headers: &HeaderMap, content_length: Option<u64>) -> HfFileMeta {
    let content_length =
        header_content_length(headers).or_else(|| content_length.filter(|n| *n > 0));
    HfFileMeta {
        url: url.to_string(),
        etag: header_string(headers, ETAG),
        last_modified: header_string(headers, LAST_MODIFIED),
        content_length,
        checked_at: crate::meta::now_secs(),
        downloaded_at: 0,
    }
}

async fn fetch_hf_meta(client: &reqwest::Client, url: &str) -> Result<HfFileMeta> {
    let resp = client
        .head(url)
        .send()
        .await
        .with_context(|| format!("HEAD {url}"))?;
    if !resp.status().is_success() {
        bail!("HTTP {} checking {url}", resp.status());
    }
    Ok(hf_meta_from_headers(
        url,
        resp.headers(),
        resp.content_length(),
    ))
}

fn existing_len(path: &Path) -> Option<u64> {
    path.metadata().ok().map(|m| m.len())
}

fn hf_meta_matches(path: &Path, local: Option<&HfFileMeta>, remote: &HfFileMeta) -> bool {
    let Some(len) = existing_len(path) else {
        return false;
    };
    if let Some(remote_len) = remote.content_length {
        if len != remote_len {
            return false;
        }
    }

    let Some(local) = local else {
        return remote.content_length == Some(len);
    };

    if remote.etag.is_some() && local.etag.is_some() {
        return local.etag == remote.etag;
    }
    if remote.last_modified.is_some() && local.last_modified.is_some() {
        return local.last_modified == remote.last_modified;
    }
    remote.content_length == Some(len)
}

async fn download_hf_file(
    label: &str,
    url: &str,
    path: &Path,
    tmp: &Path,
    sha256: &str,
) -> Result<PathBuf> {
    ensure_download_not_cancelled()?;
    let client = reqwest::Client::builder()
        .user_agent("sessionsmith/0.1")
        .build()?;
    let mut remote = match fetch_hf_meta(&client, url).await {
        Ok(meta) => Some(meta),
        Err(err) if path.exists() => {
            crate::ui::warn(&format!(
                "could not check remote metadata for {label} ({err:#}); keeping existing file"
            ));
            return Ok(path.to_path_buf());
        }
        Err(err) => {
            crate::ui::warn(&format!(
                "could not check remote metadata for {label} ({err:#}); downloading anyway"
            ));
            None
        }
    };

    if let Some(remote) = &remote {
        let local = read_hf_meta(path);
        if hf_meta_matches(path, local.as_ref(), remote) {
            let mut checked = remote.clone();
            checked.downloaded_at = local
                .as_ref()
                .map(|m| m.downloaded_at)
                .unwrap_or_else(crate::meta::now_secs);
            if let Err(err) = write_hf_meta(path, &checked) {
                crate::ui::warn(&format!("could not write metadata for {label}: {err:#}"));
            }
            crate::ui::ok(&format!("{label} already current"));
            return Ok(path.to_path_buf());
        }
        if path.exists() {
            crate::ui::info(&format!(
                "remote metadata changed for {label}; downloading update"
            ));
        }
    }

    let part_meta_path = sidecar_path(tmp);
    let mut offset = existing_len(tmp).unwrap_or(0);
    if offset > 0 {
        let partial_meta = read_hf_meta_file(&part_meta_path);
        let etag_changed = matches!(
            (partial_meta.as_ref().and_then(|meta| meta.etag.as_deref()), remote.as_ref().and_then(|meta| meta.etag.as_deref())),
            (Some(previous), Some(current)) if previous != current
        );
        if partial_meta.is_none() || etag_changed {
            crate::ui::warn(&format!("discarding stale partial download for {label}"));
            std::fs::remove_file(tmp).ok();
            std::fs::remove_file(&part_meta_path).ok();
            offset = 0;
        }
    }
    if offset == 0 {
        if let Some(remote) = &remote {
            write_hf_meta_file(&part_meta_path, remote)?;
        }
    }
    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
        crate::ui::info(&format!("resuming {label} at {offset} bytes"));
    }
    ensure_download_not_cancelled()?;
    let mut resp = request.send().await.with_context(|| format!("GET {url}"))?;
    if offset > 0 && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let expected = read_hf_meta_file(&part_meta_path).and_then(|meta| meta.etag);
        let received = header_string(resp.headers(), ETAG);
        if matches!((expected.as_deref(), received.as_deref()), (Some(expected), Some(received)) if expected != received)
        {
            crate::ui::warn(&format!(
                "partial response changed for {label}; restarting download"
            ));
            std::fs::remove_file(tmp).ok();
            std::fs::remove_file(&part_meta_path).ok();
            offset = 0;
            ensure_download_not_cancelled()?;
            resp = client
                .get(url)
                .send()
                .await
                .with_context(|| format!("GET {url}"))?;
            remote = Some(hf_meta_from_headers(
                url,
                resp.headers(),
                resp.content_length(),
            ));
            if let Some(remote) = &remote {
                write_hf_meta_file(&part_meta_path, remote)?;
            }
        }
    }
    if resp.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        std::fs::remove_file(tmp).ok();
        std::fs::remove_file(&part_meta_path).ok();
        offset = 0;
        if let Some(remote) = &remote {
            write_hf_meta_file(&part_meta_path, remote)?;
        }
        ensure_download_not_cancelled()?;
        resp = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
    }
    if !resp.status().is_success() {
        bail!("HTTP {} fetching {url}", resp.status());
    }
    let appending = offset > 0 && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if !appending {
        offset = 0;
    }
    let headers = resp.headers().clone();
    let total = remote
        .as_ref()
        .and_then(|meta| meta.content_length)
        .or_else(|| resp.content_length().map(|length| length + offset))
        .unwrap_or(0);
    let pb = crate::ui::progress_bar(total, &format!("downloading {label}"));
    pb.set_position(offset);
    let mut stream = resp.bytes_stream();

    use tokio::io::AsyncWriteExt;
    let mut file = if appending {
        tokio::fs::OpenOptions::new().append(true).open(tmp).await?
    } else {
        tokio::fs::File::create(tmp).await?
    };
    let mut received = offset;
    let mut last_emit = offset;
    let progress_label = format!("downloading {label}");
    while let Some(chunk) = stream.next().await {
        ensure_download_not_cancelled()?;
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        pb.set_position(received);
        if received >= last_emit + 4_194_304 || received == total {
            last_emit = received;
            crate::ui::progress(&progress_label, received, total);
        }
    }
    ensure_download_not_cancelled()?;
    file.flush().await?;
    drop(file);
    pb.finish_and_clear();

    if total > 0 && received != total {
        std::fs::remove_file(tmp).ok();
        std::fs::remove_file(&part_meta_path).ok();
        bail!("download for {label} ended at {received} bytes, expected {total}");
    }

    if !sha256.is_empty() {
        use sha2::{Digest, Sha256};
        let got = hex::encode(Sha256::digest(std::fs::read(tmp)?));
        if got != sha256 {
            let _ = std::fs::remove_file(tmp);
            let _ = std::fs::remove_file(&part_meta_path);
            bail!("checksum mismatch for {label} (got {got})");
        }
    }
    std::fs::rename(tmp, path)?;
    std::fs::remove_file(&part_meta_path).ok();

    let mut meta = remote.unwrap_or_else(|| hf_meta_from_headers(url, &headers, Some(received)));
    meta.checked_at = crate::meta::now_secs();
    meta.downloaded_at = meta.checked_at;
    if meta.content_length.is_none() {
        meta.content_length = Some(received);
    }
    if let Err(err) = write_hf_meta(path, &meta) {
        crate::ui::warn(&format!("could not write metadata for {label}: {err:#}"));
    }

    crate::ui::ok(&format!("saved {}", path.display()));
    Ok(path.to_path_buf())
}

fn ensure_download_not_cancelled() -> Result<()> {
    if crate::ui::cancellation_requested() {
        bail!("cancelled");
    }
    Ok(())
}

pub struct GgufAsrModel {
    pub id: &'static str,
    pub repo: &'static str,
    pub filename: &'static str,
    pub sha256: &'static str,
}

pub const GGUF_ASR_MODELS: &[GgufAsrModel] = &[GgufAsrModel {
    id: "cohere-transcribe-03-2026",
    repo: "handy-computer/cohere-transcribe-03-2026-gguf",
    filename: "cohere-transcribe-03-2026-Q5_K_M.gguf",
    sha256: "14d02f1ad6dd77b3a60f82639879012c3adb4fe25c50a5a47a2c4c661daf1558",
}];

pub fn whisper_cache_dir(override_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(d) = override_dir {
        return Ok(d.to_path_buf());
    }
    let base = dirs::cache_dir().ok_or_else(|| anyhow!("could not resolve XDG cache dir"))?;
    Ok(base.join("sessionsmith").join("whisper"))
}

pub fn whisper_path(id: &str, cache_dir: &Path) -> Result<PathBuf> {
    let model = WHISPER_MODELS
        .iter()
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
    download_whisper_with_verification(id, cache_dir, true).await
}

pub async fn download_whisper_with_verification(
    id: &str,
    cache_dir: &Path,
    verify: bool,
) -> Result<PathBuf> {
    let model = WHISPER_MODELS
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow!("unknown whisper model '{id}'"))?;
    std::fs::create_dir_all(cache_dir)?;
    let path = cache_dir.join(model.filename);
    let tmp = cache_dir.join(format!("{}.part", model.filename));
    let url = format!("{HF_BASE}/{}", model.filename);
    download_hf_file(
        model.filename,
        &url,
        &path,
        &tmp,
        if verify { model.sha256 } else { "" },
    )
    .await
}

pub fn gguf_asr_cache_dir() -> Result<PathBuf> {
    let base = dirs::cache_dir().ok_or_else(|| anyhow!("could not resolve XDG cache dir"))?;
    Ok(base.join("sessionsmith").join("asr-gguf"))
}

pub fn gguf_asr_path(id: &str, cache_dir: &Path) -> Result<PathBuf> {
    let model = GGUF_ASR_MODELS
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow!("unknown GGUF ASR model '{id}'"))?;
    Ok(cache_dir.join(model.filename))
}

/// Delete a downloaded GGUF ASR model. No error if it is not present.
pub fn delete_gguf_asr(id: &str, cache_dir: &Path) -> Result<()> {
    let path = gguf_asr_path(id, cache_dir)?;
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("deleting {}", path.display()))?;
    }
    let sidecar = sidecar_path(&path);
    if sidecar.exists() {
        std::fs::remove_file(&sidecar)
            .with_context(|| format!("deleting {}", sidecar.display()))?;
    }
    let tmp = cache_dir.join(format!(
        "{}.part",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    std::fs::remove_file(&tmp).ok();
    std::fs::remove_file(sidecar_path(&tmp)).ok();
    Ok(())
}

pub async fn ensure_gguf_asr(id: &str, cache_dir: &Path) -> Result<PathBuf> {
    let path = gguf_asr_path(id, cache_dir)?;
    if path.exists() {
        return Ok(path);
    }
    download_gguf_asr(id, cache_dir).await
}

pub async fn download_gguf_asr(id: &str, cache_dir: &Path) -> Result<PathBuf> {
    download_gguf_asr_with_verification(id, cache_dir, true).await
}

pub async fn download_gguf_asr_with_verification(
    id: &str,
    cache_dir: &Path,
    verify: bool,
) -> Result<PathBuf> {
    let model = GGUF_ASR_MODELS
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow!("unknown GGUF ASR model '{id}'"))?;
    std::fs::create_dir_all(cache_dir)?;
    let path = cache_dir.join(model.filename);
    let tmp = cache_dir.join(format!("{}.part", model.filename));
    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        model.repo, model.filename
    );
    download_hf_file(
        model.filename,
        &url,
        &path,
        &tmp,
        if verify { model.sha256 } else { "" },
    )
    .await
}

pub fn ollama_pull(name: &str) -> Result<()> {
    crate::ui::info(&format!("ollama pull {name}"));
    let status = Command::new("ollama")
        .args(["pull", name])
        .status()
        .with_context(|| "ollama not found on PATH")?;
    if !status.success() {
        bail!("ollama pull {name} failed");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Curated Ollama LLM catalog for the model manager. Each model may offer
// several installable options (parameter-size variants). Cloud-only tags are
// deliberately excluded. Pull ids are used verbatim with `ollama pull`.
// ---------------------------------------------------------------------------

/// Month when the curated Ollama catalog was last reviewed.
pub const OLLAMA_CATALOG_UPDATED: &str = "2026-08";

pub struct OllamaOption {
    /// Short label shown when the model is expanded (e.g. `"9b"`).
    pub label: &'static str,
    /// The exact id passed to `ollama pull`.
    pub pull: &'static str,
    /// Approximate download size in bytes (refined from Ollama after a pull).
    pub size: u64,
    /// Published parameter count represented by this exact pull option.
    pub params: u64,
}

pub struct OllamaModel {
    pub display: &'static str,
    pub released: &'static str,
    pub langs: Option<&'static str>,
    pub languages: &'static [&'static str],
    pub license: Option<&'static str>,
    pub note: Option<&'static str>,
    pub options: &'static [OllamaOption],
}

pub const OLLAMA_CATALOG: &[OllamaModel] = &[
    OllamaModel {
        display: "qwen3.8",
        released: "2026-08",
        langs: Some("Multilingual"),
        languages: &["Multilingual", "English"],
        license: None,
        note: Some("Curated 27B Ollama option."),
        options: &[OllamaOption {
            label: "27b",
            pull: "qwen3.8:27b",
            size: 18_000_000_000,
            params: 27_000_000_000,
        }],
    },
    OllamaModel {
        display: "qwen3.5",
        released: "2026-03",
        langs: Some("Multilingual"),
        languages: &["Multilingual", "English"],
        license: None,
        note: Some("Curated Ollama options from 0.8B through 122B."),
        options: &[
            OllamaOption {
                label: "0.8b",
                pull: "qwen3.5:0.8b",
                size: 1_000_000_000,
                params: 800_000_000,
            },
            OllamaOption {
                label: "2b",
                pull: "qwen3.5:2b",
                size: 2_700_000_000,
                params: 2_000_000_000,
            },
            OllamaOption {
                label: "4b",
                pull: "qwen3.5:4b",
                size: 3_400_000_000,
                params: 4_000_000_000,
            },
            OllamaOption {
                label: "9b",
                pull: "qwen3.5:9b",
                size: 6_600_000_000,
                params: 9_000_000_000,
            },
            OllamaOption {
                label: "27b",
                pull: "qwen3.5:27b",
                size: 17_000_000_000,
                params: 27_000_000_000,
            },
            OllamaOption {
                label: "35b",
                pull: "qwen3.5:35b",
                size: 24_000_000_000,
                params: 35_000_000_000,
            },
            OllamaOption {
                label: "122b",
                pull: "qwen3.5:122b",
                size: 81_000_000_000,
                params: 122_000_000_000,
            },
        ],
    },
    OllamaModel {
        display: "ornith",
        released: "2026-06",
        langs: Some("English"),
        languages: &["English"],
        license: None,
        note: Some("Curated 9B and 35B Ollama options."),
        options: &[
            OllamaOption {
                label: "9b",
                pull: "ornith:9b",
                size: 5_600_000_000,
                params: 9_000_000_000,
            },
            OllamaOption {
                label: "35b",
                pull: "ornith:35b",
                size: 21_000_000_000,
                params: 35_000_000_000,
            },
        ],
    },
    OllamaModel {
        display: "Agents-A1",
        released: "2026-06",
        langs: Some("English"),
        languages: &["English"],
        license: None,
        note: Some("Curated 35B Q4_K_M GGUF option served through Ollama."),
        options: &[OllamaOption {
            label: "35b Q4_K_M",
            pull: "hf.co/InternScience/Agents-A1-Q4_K_M-GGUF",
            size: 22_800_000_000,
            params: 35_000_000_000,
        }],
    },
    OllamaModel {
        display: "llama3.3",
        released: "2024-12",
        langs: Some("8 languages"),
        languages: &[
            "English",
            "German",
            "French",
            "Italian",
            "Portuguese",
            "Hindi",
            "Spanish",
            "Thai",
        ],
        license: None,
        note: Some("Curated 70B Ollama option."),
        options: &[OllamaOption {
            label: "70b",
            pull: "llama3.3:70b",
            size: 43_000_000_000,
            params: 70_000_000_000,
        }],
    },
    OllamaModel {
        display: "llama3.2",
        released: "2024-09",
        langs: Some("8 languages"),
        languages: &[
            "English",
            "German",
            "French",
            "Italian",
            "Portuguese",
            "Hindi",
            "Spanish",
            "Thai",
        ],
        license: None,
        note: Some("Curated compact 1B and 3B Ollama options."),
        options: &[
            OllamaOption {
                label: "1b",
                pull: "llama3.2:1b",
                size: 1_300_000_000,
                params: 1_000_000_000,
            },
            OllamaOption {
                label: "3b",
                pull: "llama3.2:3b",
                size: 2_000_000_000,
                params: 3_000_000_000,
            },
        ],
    },
    OllamaModel {
        display: "gemma3",
        released: "2025-03",
        langs: Some("140+ languages"),
        languages: &["Multilingual", "English"],
        license: None,
        note: Some("Curated 4B, 12B, and 27B Ollama options."),
        options: &[
            OllamaOption {
                label: "4b",
                pull: "gemma3:4b",
                size: 3_300_000_000,
                params: 4_000_000_000,
            },
            OllamaOption {
                label: "12b",
                pull: "gemma3:12b",
                size: 8_100_000_000,
                params: 12_000_000_000,
            },
            OllamaOption {
                label: "27b",
                pull: "gemma3:27b",
                size: 17_000_000_000,
                params: 27_000_000_000,
            },
        ],
    },
    OllamaModel {
        display: "mistral",
        released: "2023-09",
        langs: Some("English"),
        languages: &["English"],
        license: None,
        note: Some("Curated 7B Ollama option."),
        options: &[OllamaOption {
            label: "7b",
            pull: "mistral:7b",
            size: 4_100_000_000,
            params: 7_000_000_000,
        }],
    },
    OllamaModel {
        display: "phi4",
        released: "2024-12",
        langs: Some("English"),
        languages: &["English"],
        license: None,
        note: Some("Curated 14B Ollama option."),
        options: &[OllamaOption {
            label: "14b",
            pull: "phi4:14b",
            size: 9_100_000_000,
            params: 14_000_000_000,
        }],
    },
    OllamaModel {
        display: "phi4-mini",
        released: "2025-02",
        langs: Some("24 languages"),
        languages: &["Multilingual", "English"],
        license: None,
        note: Some("Curated compact 3.8B Ollama option."),
        options: &[OllamaOption {
            label: "3.8b",
            pull: "phi4-mini:3.8b",
            size: 2_500_000_000,
            params: 3_800_000_000,
        }],
    },
];

/// Delete a downloaded whisper ggml model. No error if it isn't present.
pub fn delete_whisper(id: &str, cache_dir: &Path) -> Result<()> {
    let path = whisper_path(id, cache_dir)?;
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("deleting {}", path.display()))?;
    }
    let sidecar = sidecar_path(&path);
    if sidecar.exists() {
        std::fs::remove_file(&sidecar)
            .with_context(|| format!("deleting {}", sidecar.display()))?;
    }
    let tmp = cache_dir.join(format!(
        "{}.part",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    std::fs::remove_file(&tmp).ok();
    std::fs::remove_file(sidecar_path(&tmp)).ok();
    Ok(())
}

/// Approximate on-disk size of a whisper ggml model, for display before it is
/// downloaded (bytes).
pub fn whisper_approx_size(id: &str) -> u64 {
    match id {
        "tiny" => 78_000_000,
        "base" => 148_000_000,
        "small" => 488_000_000,
        "medium" => 1_530_000_000,
        "large-v3" => 3_100_000_000,
        "large-v3-turbo" => 1_620_000_000,
        _ => 0,
    }
}

/// Pull (or update) an Ollama model over the HTTP API, streaming progress via
/// [`crate::ui::progress`]. Used by the TUI so nothing writes to the terminal.
pub async fn ollama_pull_stream(name: &str, base_url: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("sessionsmith/0.1")
        .build()?;
    let url = format!("{}/api/pull", base_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "name": name, "stream": true }))
        .send()
        .await
        .with_context(|| format!("POST {url} (is Ollama running?)"))?;
    if !resp.status().is_success() {
        bail!("ollama pull {name}: HTTP {}", resp.status());
    }
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                    if err.contains("requires a newer version") || err.contains("412") {
                        bail!(
                            "Ollama is out of date for this model — press 'u' in the \
                             model manager (or the command palette → Update Ollama) to \
                             update it, then try again. ({err})"
                        );
                    }
                    bail!("ollama: {err}");
                }
                let status = v
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("pulling");
                let completed = v.get("completed").and_then(|s| s.as_u64()).unwrap_or(0);
                let total = v.get("total").and_then(|s| s.as_u64()).unwrap_or(0);
                crate::ui::progress(&format!("{name}: {status}"), completed, total);
            }
        }
    }
    crate::ui::ok(&format!("pulled {name}"));
    Ok(())
}

/// Delete an Ollama model over the HTTP API.
pub async fn ollama_delete(name: &str, base_url: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/delete", base_url.trim_end_matches('/'));
    let resp = client
        .delete(&url)
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .with_context(|| format!("DELETE {url} (is Ollama running?)"))?;
    if !resp.status().is_success() {
        bail!("ollama delete {name}: HTTP {}", resp.status());
    }
    Ok(())
}

/// Names of Ollama models currently installed locally (via `/api/tags`).
pub async fn ollama_local_names(base_url: &str) -> Vec<String> {
    ollama_local_models(base_url)
        .await
        .into_iter()
        .map(|(n, _)| n)
        .collect()
}

/// Locally-installed Ollama models with their on-disk size (via `/api/tags`).
pub async fn ollama_local_models(base_url: &str) -> Vec<(String, u64)> {
    ollama_local_models_checked(base_url)
        .await
        .unwrap_or_default()
}

/// Locally-installed Ollama models with their on-disk size (via `/api/tags`).
/// Unlike [`ollama_local_models`], this preserves a connectivity or protocol
/// failure so interactive hosts can show a useful service status.
pub async fn ollama_local_models_checked(base_url: &str) -> Result<Vec<(String, u64)>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url} (is Ollama running?)"))?;
    if !response.status().is_success() {
        bail!("Ollama returned HTTP {}", response.status());
    }
    let value = response
        .json::<serde_json::Value>()
        .await
        .with_context(|| format!("decoding {url}"))?;
    let mut models = Vec::new();
    if let Some(items) = value.get("models").and_then(|models| models.as_array()) {
        for item in items {
            if let Some(name) = item.get("name").and_then(|name| name.as_str()) {
                let size = item.get("size").and_then(|size| size.as_u64()).unwrap_or(0);
                models.push((name.to_string(), size));
            }
        }
    }
    Ok(models)
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        sync::Notify,
    };
    use tokio_util::sync::CancellationToken;
    use wiremock::{
        matchers::{header, method},
        Mock, MockServer, ResponseTemplate,
    };

    use crate::jobs::report::ChannelReporter;

    #[test]
    fn every_catalog_model_has_a_sha256_pin() {
        for model in WHISPER_MODELS {
            assert_eq!(model.sha256.len(), 64, "{} has no SHA-256", model.id);
            assert!(model
                .sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit()));
        }
        for model in GGUF_ASR_MODELS {
            assert_eq!(model.sha256.len(), 64, "{} has no SHA-256", model.id);
            assert!(model
                .sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn catalog_metadata_is_structured_at_the_source() {
        let whisper = WHISPER_MODELS
            .iter()
            .find(|model| model.id == "large-v3-turbo")
            .expect("Whisper turbo is cataloged");
        assert_eq!(whisper.params, 809_000_000);
        assert_eq!(whisper.languages, &["Multilingual"]);
        assert_eq!(whisper.license, "MIT");
        assert!(!whisper.note.is_empty());

        let ollama = OLLAMA_CATALOG
            .iter()
            .find(|model| model.display == "qwen3.5")
            .expect("qwen3.5 is cataloged");
        let option = ollama
            .options
            .iter()
            .find(|option| option.pull == "qwen3.5:9b")
            .expect("qwen3.5:9b is cataloged");
        assert_eq!(option.params, 9_000_000_000);
        assert_eq!(ollama.languages, &["Multilingual", "English"]);
        assert_eq!(ollama.license, None);
        assert!(ollama.note.is_some());
    }

    fn remote_meta(url: String, etag: &str, length: u64) -> HfFileMeta {
        HfFileMeta {
            url,
            etag: Some(etag.into()),
            last_modified: None,
            content_length: Some(length),
            checked_at: 0,
            downloaded_at: 0,
        }
    }

    async fn mount_head(server: &MockServer, etag: &str, length: u64) {
        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ETag", etag)
                    .insert_header("Content-Length", length.to_string()),
            )
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn resumes_from_partial_file_with_a_range_request() {
        let server = MockServer::start().await;
        mount_head(&server, "v1", 10).await;
        Mock::given(method("GET"))
            .and(header("range", "bytes=5-"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("ETag", "v1")
                    .set_body_bytes(b"world"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("model.bin");
        let part = temp.path().join("model.bin.part");
        std::fs::write(&part, b"hello").unwrap();
        write_hf_meta_file(&sidecar_path(&part), &remote_meta(server.uri(), "v1", 10)).unwrap();

        download_hf_file("model", &server.uri(), &path, &part, "")
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"helloworld");
        assert!(!part.exists());
    }

    #[tokio::test]
    async fn changed_partial_response_etag_restarts_without_stale_bytes() {
        let server = MockServer::start().await;
        mount_head(&server, "v1", 10).await;
        Mock::given(method("GET"))
            .and(header("range", "bytes=5-"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("ETag", "v2")
                    .set_body_bytes(b"world"),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ETag", "v2")
                    .set_body_bytes(b"new-model!"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("model.bin");
        let part = temp.path().join("model.bin.part");
        std::fs::write(&part, b"hello").unwrap();
        write_hf_meta_file(&sidecar_path(&part), &remote_meta(server.uri(), "v1", 10)).unwrap();

        download_hf_file("model", &server.uri(), &path, &part, "")
            .await
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"new-model!");
        assert!(!part.exists());
    }

    #[tokio::test]
    async fn checksum_mismatch_removes_downloaded_bytes() {
        let server = MockServer::start().await;
        mount_head(&server, "v1", 5).await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ETag", "v1")
                    .set_body_bytes(b"world"),
            )
            .mount(&server)
            .await;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("model.bin");
        let part = temp.path().join("model.bin.part");
        let sha256_of_hello = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

        let error = download_hf_file("model", &server.uri(), &path, &part, sha256_of_hello)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("checksum mismatch"));
        assert!(!path.exists());
        assert!(!part.exists());
    }

    #[tokio::test]
    async fn truncated_download_removes_partial_and_sidecar() {
        let server = MockServer::start().await;
        mount_head(&server, "v1", 10).await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("ETag", "v1")
                    .set_body_bytes(b"short"),
            )
            .mount(&server)
            .await;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("model.bin");
        let part = temp.path().join("model.bin.part");

        let error = download_hf_file("model", &server.uri(), &path, &part, "")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("ended at 5 bytes, expected 10"));
        assert!(!path.exists());
        assert!(!part.exists());
        assert!(!sidecar_path(&part).exists());
    }

    #[tokio::test]
    async fn cancelled_download_keeps_a_resumable_partial_file() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test server should bind");
        let url = format!("http://{}/model.bin", listener.local_addr().unwrap());
        let first_chunk_sent = Arc::new(Notify::new());
        let finish_response = Arc::new(Notify::new());
        let server_first_chunk_sent = first_chunk_sent.clone();
        let server_finish_response = finish_response.clone();
        let server = tokio::spawn(async move {
            let (mut head, _) = listener.accept().await.expect("HEAD should connect");
            read_http_request(&mut head).await;
            head.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nETag: v1\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("HEAD response should write");

            let (mut get, _) = listener.accept().await.expect("GET should connect");
            read_http_request(&mut get).await;
            get.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nETag: v1\r\nConnection: close\r\n\r\nhello",
            )
            .await
            .expect("first response chunk should write");
            get.flush()
                .await
                .expect("first response chunk should flush");
            server_first_chunk_sent.notify_one();
            server_finish_response.notified().await;
            get.write_all(b"world")
                .await
                .expect("second response chunk should write");
        });

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("model.bin");
        let part = temp.path().join("model.bin.part");
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();
        let reporter = Arc::new(ChannelReporter::new(sender, cancellation.clone()));
        let download_path = path.clone();
        let download_part = part.clone();
        let download = tokio::spawn(crate::ui::with_reporter(reporter, async move {
            download_hf_file("model", &url, &download_path, &download_part, "").await
        }));

        first_chunk_sent.notified().await;
        for _ in 0..100 {
            if std::fs::metadata(&part).map(|meta| meta.len()).ok() == Some(5) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(std::fs::read(&part).unwrap(), b"hello");

        cancellation.cancel();
        finish_response.notify_one();
        let error = download
            .await
            .expect("download task should complete")
            .expect_err("download should observe cancellation");
        assert!(error.to_string().contains("cancelled"));
        assert_eq!(std::fs::read(&part).unwrap(), b"hello");
        assert!(!path.exists());
        server.await.expect("test server should complete");
    }

    async fn read_http_request(stream: &mut TcpStream) {
        let mut request = Vec::new();
        let mut buffer = [0; 512];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).await.expect("request should read");
            assert!(count > 0, "request ended before its headers");
            request.extend_from_slice(&buffer[..count]);
        }
    }

    #[tokio::test]
    async fn checked_ollama_inventory_preserves_model_sizes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [
                    { "name": "qwen3:14b", "size": 9_000_000_000u64 },
                    { "name": "custom:latest", "size": 123u64 }
                ]
            })))
            .mount(&server)
            .await;

        assert_eq!(
            ollama_local_models_checked(&server.uri()).await.unwrap(),
            vec![
                ("qwen3:14b".to_string(), 9_000_000_000),
                ("custom:latest".to_string(), 123),
            ]
        );
    }
}

// ---------------------------------------------------------------------------
// Approximate sizes for common Ollama models (bytes). Used as fallback when
// the model hasn't been pulled yet and Ollama's registry can't provide info.
// ---------------------------------------------------------------------------
pub const OLLAMA_KNOWN_SIZES: &[(&str, u64)] = &[
    ("qwen3.8:27b", 18_000_000_000),
    ("qwen2.5:0.5b", 397_000_000),
    ("qwen2.5:1.5b", 986_000_000),
    ("qwen2.5:3b", 1_900_000_000),
    ("qwen2.5:7b", 4_700_000_000),
    ("qwen2.5:14b", 9_000_000_000),
    ("qwen2.5:32b", 20_000_000_000),
    ("qwen2.5:72b", 47_000_000_000),
    ("llama3.3:70b", 43_000_000_000),
    ("llama3.2:3b", 2_000_000_000),
    ("llama3.2:1b", 738_000_000),
    ("gemma3:4b", 3_300_000_000),
    ("gemma3:12b", 8_100_000_000),
    ("gemma3:27b", 17_000_000_000),
    ("mistral:7b", 4_100_000_000),
    ("phi4:14b", 9_100_000_000),
    ("phi4-mini:3.8b", 2_500_000_000),
    // HuggingFace GGUF models, pulled via Ollama's `hf.co/` prefix. Sizes are the
    // Q4_K_M build; they refine from Ollama after a pull.
    ("hf.co/InternScience/Agents-A1-Q4_K_M-GGUF", 22_800_000_000),
    ("hf.co/deepreinforce-ai/Ornith-1.0-35B-GGUF", 22_760_000_000),
];

/// Approximate public release date (`YYYY-MM`) per known model, for the model
/// manager's age column. `"—"` when unknown.
pub const OLLAMA_RELEASED: &[(&str, &str)] = &[
    ("qwen3.8:27b", "2026-08"),
    ("qwen2.5:0.5b", "2024-09"),
    ("qwen2.5:1.5b", "2024-09"),
    ("qwen2.5:3b", "2024-09"),
    ("qwen2.5:7b", "2024-09"),
    ("qwen2.5:14b", "2024-09"),
    ("qwen2.5:32b", "2024-09"),
    ("qwen2.5:72b", "2024-09"),
    ("llama3.3:70b", "2024-12"),
    ("llama3.2:3b", "2024-09"),
    ("llama3.2:1b", "2024-09"),
    ("gemma3:4b", "2025-03"),
    ("gemma3:12b", "2025-03"),
    ("gemma3:27b", "2025-03"),
    ("mistral:7b", "2023-09"),
    ("phi4:14b", "2024-12"),
    ("phi4-mini:3.8b", "2025-02"),
    ("hf.co/InternScience/Agents-A1-Q4_K_M-GGUF", "2026-06"),
    ("hf.co/deepreinforce-ai/Ornith-1.0-35B-GGUF", "2026-06"),
];

/// Release date (`YYYY-MM`) of an Ollama model, or `"—"` if unknown.
pub fn ollama_released(name: &str) -> &'static str {
    OLLAMA_RELEASED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, d)| *d)
        .unwrap_or("—")
}

/// Release date (`YYYY-MM`) of a whisper model, or `"—"` if unknown.
pub fn whisper_released(id: &str) -> &'static str {
    WHISPER_MODELS
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.released)
        .unwrap_or("—")
}

/// Fetch the actual file size of a whisper ggml model via HTTP HEAD on HuggingFace.
/// Returns `None` on network error or if Content-Length is absent.
pub async fn fetch_hf_size(id: &str) -> Option<u64> {
    let model = WHISPER_MODELS.iter().find(|m| m.id == id)?;
    let url = format!("{HF_BASE}/{}", model.filename);
    let client = reqwest::Client::builder()
        .user_agent("sessionsmith/0.1")
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    let resp = client.head(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.content_length()
}

/// Fetch the size of an Ollama model.
/// Tries the local Ollama registry first (works even for already-pulled models),
/// then falls back to the built-in size table for common models.
pub async fn fetch_ollama_size(name: &str, base_url: &str) -> Option<u64> {
    // Query /api/tags (lists all local models with their sizes)
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;
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
    OLLAMA_KNOWN_SIZES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
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
