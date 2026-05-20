//! OpenAI-compatible backend (works with OpenAI, OpenRouter, LM Studio, vLLM).
//! Streams SSE from `/v1/chat/completions`.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::config::GlobalConfig;
use super::{ChatMessage, ChatOptions, LlmBackend};

pub struct OpenAIBackend {
    base_url: String,
    api_key: String,
    default_model: Option<String>,
    client: reqwest::Client,
}

impl OpenAIBackend {
    pub fn from_config(g: &GlobalConfig) -> Result<Self> {
        let base_url = g.backend.base_url.clone().unwrap_or_else(|| "https://api.openai.com".into());
        let api_key = g.resolved_api_key().unwrap_or_default();
        if api_key.is_empty() {
            return Err(anyhow!("openai backend requires api_key in global config"));
        }
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            default_model: g.backend.model.clone(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(g.runtime.timeout_secs))
                .build()?,
        })
    }
}

#[derive(Serialize)]
struct ChatReq<'a> {
    model: &'a str,
    messages: Vec<MsgOut<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct MsgOut<'a> { role: &'a str, content: &'a str }

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}
#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Delta,
}
#[derive(Deserialize, Default)]
struct Delta { #[serde(default)] content: Option<String> }

#[async_trait]
impl LlmBackend for OpenAIBackend {
    fn name(&self) -> &'static str { "openai" }

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
            temperature: opts.temperature,
            max_tokens: opts.max_tokens,
        };
        let resp = self.client.post(format!("{}/v1/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body).send().await?;
        if !resp.status().is_success() {
            let s = resp.status();
            let t = resp.text().await.unwrap_or_default();
            return Err(anyhow!("openai HTTP {s}: {t}"));
        }
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut stream = resp.bytes_stream().eventsource();
            while let Some(ev) = stream.next().await {
                let event = match ev {
                    Ok(e) => e,
                    Err(e) => { let _ = tx.send(Err(anyhow!("sse: {e}"))).await; return; }
                };
                let data = event.data;
                if data == "[DONE]" { return; }
                if data.is_empty() { continue; }
                match serde_json::from_str::<StreamChunk>(&data) {
                    Ok(c) => {
                        for ch in c.choices {
                            if let Some(text) = ch.delta.content {
                                if !text.is_empty() {
                                    if tx.send(Ok(text)).await.is_err() { return; }
                                }
                            }
                        }
                    }
                    Err(_) => { /* ignore non-JSON pings */ }
                }
            }
        });
        Ok(rx)
    }
}
