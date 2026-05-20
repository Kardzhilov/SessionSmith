//! Anthropic Messages API backend with SSE streaming.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::config::GlobalConfig;
use super::{ChatMessage, ChatOptions, LlmBackend, Role};

pub struct AnthropicBackend {
    base_url: String,
    api_key: String,
    default_model: Option<String>,
    client: reqwest::Client,
}

impl AnthropicBackend {
    pub fn from_config(g: &GlobalConfig) -> Result<Self> {
        let base_url = g.backend.base_url.clone().unwrap_or_else(|| "https://api.anthropic.com".into());
        let api_key = g.resolved_api_key().unwrap_or_default();
        if api_key.is_empty() {
            return Err(anyhow!("anthropic backend requires api_key in global config"));
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
struct MsgReq<'a> {
    model: &'a str,
    system: String,
    messages: Vec<MsgOut<'a>>,
    stream: bool,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct MsgOut<'a> { role: &'a str, content: &'a str }

#[derive(Deserialize)]
#[serde(tag = "type")]
enum SseEvent {
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { delta: TextDelta },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum TextDelta {
    #[serde(rename = "text_delta")]
    Text { text: String },
    #[serde(other)]
    Other,
}

#[async_trait]
impl LlmBackend for AnthropicBackend {
    fn name(&self) -> &'static str { "anthropic" }

    async fn stream_chat(&self, messages: Vec<ChatMessage>, opts: ChatOptions)
        -> Result<mpsc::Receiver<Result<String>>>
    {
        let model = if !opts.model.is_empty() { opts.model.clone() }
                    else { self.default_model.clone().ok_or_else(|| anyhow!("no model configured"))? };

        // Pull out system messages — Anthropic uses a separate top-level field.
        let mut system = String::new();
        let mut chat: Vec<MsgOut> = Vec::new();
        for m in &messages {
            match m.role {
                Role::System => {
                    if !system.is_empty() { system.push_str("\n\n"); }
                    system.push_str(&m.content);
                }
                Role::User => chat.push(MsgOut { role: "user", content: &m.content }),
                Role::Assistant => chat.push(MsgOut { role: "assistant", content: &m.content }),
            }
        }

        let body = MsgReq {
            model: &model,
            system,
            messages: chat,
            stream: true,
            max_tokens: opts.max_tokens.unwrap_or(8192),
            temperature: opts.temperature,
        };

        let resp = self.client.post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body).send().await?;
        if !resp.status().is_success() {
            let s = resp.status();
            let t = resp.text().await.unwrap_or_default();
            return Err(anyhow!("anthropic HTTP {s}: {t}"));
        }

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut stream = resp.bytes_stream().eventsource();
            while let Some(ev) = stream.next().await {
                let ev = match ev {
                    Ok(e) => e,
                    Err(e) => { let _ = tx.send(Err(anyhow!("sse: {e}"))).await; return; }
                };
                if ev.data.is_empty() { continue; }
                match serde_json::from_str::<SseEvent>(&ev.data) {
                    Ok(SseEvent::ContentBlockDelta { delta: TextDelta::Text { text } }) => {
                        if !text.is_empty() {
                            if tx.send(Ok(text)).await.is_err() { return; }
                        }
                    }
                    Ok(SseEvent::MessageStop) => return,
                    _ => {}
                }
            }
        });
        Ok(rx)
    }
}
