//! Ollama backend — streams NDJSON from `/api/chat`.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::config::GlobalConfig;
use super::{ChatMessage, ChatOptions, LlmBackend};

pub struct OllamaBackend {
    base_url: String,
    default_model: Option<String>,
    client: reqwest::Client,
}

impl OllamaBackend {
    pub fn from_config(g: &GlobalConfig) -> Result<Self> {
        let base_url = g.backend.base_url.clone().unwrap_or_else(|| "http://localhost:11434".into());
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            default_model: g.backend.model.clone(),
            // Use only a connect timeout so that long local generations (story,
            // campaign log) are never killed mid-stream.  If Ollama crashes the
            // TCP connection closes and the stream errors naturally.
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }
}

#[derive(Deserialize)]
struct PsResp {
    #[serde(default)]
    models: Vec<PsModel>,
}

#[derive(Deserialize)]
struct PsModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
}

#[derive(Serialize)]
struct UnloadReq<'a> {
    model: &'a str,
    /// `0` tells Ollama to evict the model from (V)RAM immediately.
    keep_alive: u32,
}

/// Unload every model Ollama currently holds resident, freeing its VRAM.
/// Best-effort and quick: if Ollama isn't running or is unresponsive the calls
/// simply time out and we return the names we managed to evict. Returns the
/// list of model names that were asked to unload.
pub async fn unload_all(base_url: &str) -> Vec<String> {
    let base = base_url.trim_end_matches('/');
    // Short timeouts — this runs on the hot path before/after heavy work and
    // must never block the pipeline if Ollama is down.
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let loaded: Vec<String> = match client.get(format!("{base}/api/ps")).send().await {
        Ok(resp) => match resp.json::<PsResp>().await {
            Ok(ps) => ps
                .models
                .into_iter()
                .map(|m| if m.name.is_empty() { m.model } else { m.name })
                .filter(|n| !n.is_empty())
                .collect(),
            Err(_) => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };

    let mut unloaded = Vec::new();
    for name in loaded {
        let ok = client
            .post(format!("{base}/api/generate"))
            .json(&UnloadReq { model: &name, keep_alive: 0 })
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ok {
            unloaded.push(name);
        }
    }
    unloaded
}

#[derive(Serialize)]
struct ChatReq<'a> {
    model: &'a str,
    messages: Vec<MsgOut<'a>>,
    stream: bool,
    options: OllamaOptions,
    /// When false, disables chain-of-thought reasoning in thinking models
    /// (Qwen3, etc). Dramatically reduces latency for extraction tasks.
    think: bool,
    /// Structured-output schema (Ollama `format`). Omitted when free-form.
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct MsgOut<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize, Default)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
    /// Context window. Without this Ollama falls back to a small default
    /// (often 4096) and silently truncates long transcripts.
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u32>,
    /// Force all model layers onto the GPU.  Without this Ollama may keep the
    /// model in VRAM but run matrix multiplications on CPU (worst of both worlds).
    /// -1 tells llama.cpp to offload as many layers as VRAM allows.
    num_gpu: i32,
}

#[derive(Deserialize)]
struct ChatChunk {
    #[serde(default)]
    message: Option<ChunkMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ChunkMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    thinking: Option<String>,
}

#[async_trait]
impl LlmBackend for OllamaBackend {
    fn name(&self) -> &'static str { "ollama" }

    async fn stream_chat(&self, messages: Vec<ChatMessage>, opts: ChatOptions)
        -> Result<mpsc::Receiver<Result<String>>>
    {
        let model = if !opts.model.is_empty() { opts.model.clone() }
                    else { self.default_model.clone().ok_or_else(|| anyhow!("no model configured"))? };
        let body = ChatReq {
            model: &model,
            messages: messages.iter()
                .map(|m| MsgOut { role: m.role.as_str(), content: &m.content })
                .collect(),
            stream: true,
            options: OllamaOptions { temperature: opts.temperature, num_predict: opts.max_tokens, num_ctx: opts.num_ctx, num_gpu: -1 },
            think: opts.think,
            format: opts.format.clone(),
        };
        let resp = crate::llm::send_with_retry("ollama", || {
            self.client.post(format!("{}/api/chat", self.base_url))
                .json(&body)
                .send()
        }).await?;

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut stream = resp.bytes_stream();
            let mut buf = Vec::new();
            let mut thinking_words = 0usize;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Err(e) => { let _ = tx.send(Err(e.into())).await; return; }
                    Ok(bytes) => {
                        buf.extend_from_slice(&bytes);
                        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                            let line = buf.drain(..=pos).collect::<Vec<_>>();
                            let line = &line[..line.len() - 1];
                            if line.is_empty() { continue; }
                            match serde_json::from_slice::<ChatChunk>(line) {
                                Ok(c) => {
                                    if let Some(err) = c.error {
                                        let _ = tx.send(Err(anyhow!("ollama error: {err}"))).await;
                                        return;
                                    }
                                    if let Some(ref m) = c.message {
                                        // Thinking phase: count tokens and send a spinner-only
                                        // sentinel so the UI stays alive during long reasoning.
                                        if let Some(ref t) = m.thinking {
                                            if !t.is_empty() {
                                                thinking_words += t.split_whitespace().count();
                                                let _ = tx.send(Ok(format!("\x00thinking:{thinking_words}"))).await;
                                            }
                                        }
                                        if !m.content.is_empty()
                                            && tx.send(Ok(m.content.clone())).await.is_err()
                                        {
                                            return;
                                        }
                                    }
                                    if c.done { return; }
                                }
                                Err(_) => { /* ignore stray non-JSON lines */ }
                            }
                        }
                    }
                }
            }
        });
        Ok(rx)
    }
}
