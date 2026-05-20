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

#[derive(Serialize)]
struct ChatReq<'a> {
    model: &'a str,
    messages: Vec<MsgOut<'a>>,
    stream: bool,
    options: OllamaOptions,
    /// When false, disables chain-of-thought reasoning in thinking models
    /// (Qwen3, etc). Dramatically reduces latency for extraction tasks.
    think: bool,
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
            options: OllamaOptions { temperature: opts.temperature, num_predict: opts.max_tokens, num_gpu: -1 },
            think: opts.think,
        };
        let resp = self.client.post(format!("{}/api/chat", self.base_url))
            .json(&body).send().await?;
        if !resp.status().is_success() {
            let s = resp.status();
            let t = resp.text().await.unwrap_or_default();
            return Err(anyhow!("ollama HTTP {s}: {t}"));
        }

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
                                        if !m.content.is_empty() {
                                            if tx.send(Ok(m.content.clone())).await.is_err() { return; }
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
