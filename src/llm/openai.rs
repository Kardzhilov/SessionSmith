//! OpenAI-compatible backend (works with OpenAI, OpenRouter, LM Studio, vLLM).
//! Streams SSE from `/v1/chat/completions`.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::{ChatMessage, ChatOptions, LlmBackend, LlmUsage, StreamEvent};
use crate::config::GlobalConfig;

pub struct OpenAIBackend {
    base_url: String,
    api_key: String,
    default_model: Option<String>,
    client: reqwest::Client,
}

impl OpenAIBackend {
    pub fn from_config(g: &GlobalConfig) -> Result<Self> {
        let base_url = g
            .backend
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com".into());
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
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct MsgOut<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    usage: Option<Usage>,
}
#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Delta,
}
#[derive(Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
}
#[derive(Deserialize)]
struct Usage {
    prompt_tokens: usize,
    completion_tokens: usize,
}

#[async_trait]
impl LlmBackend for OpenAIBackend {
    fn name(&self) -> &'static str {
        "openai"
    }

    async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        opts: ChatOptions,
    ) -> Result<mpsc::Receiver<Result<StreamEvent>>> {
        let model = if !opts.model.is_empty() {
            opts.model.clone()
        } else {
            self.default_model
                .clone()
                .ok_or_else(|| anyhow!("no model configured"))?
        };
        // Map a JSON schema request onto OpenAI's `response_format`.
        let response_format = opts.format.as_ref().map(|schema| {
            serde_json::json!({
                "type": "json_schema",
                "json_schema": { "name": "artifact", "schema": schema, "strict": false }
            })
        });
        let body = ChatReq {
            model: &model,
            messages: messages
                .iter()
                .map(|m| MsgOut {
                    role: m.role.as_str(),
                    content: &m.content,
                })
                .collect(),
            stream: true,
            temperature: opts.temperature,
            max_tokens: opts.max_tokens,
            response_format,
            stream_options: StreamOptions {
                include_usage: true,
            },
        };
        let resp = crate::llm::send_with_retry("openai", || {
            self.client
                .post(format!("{}/v1/chat/completions", self.base_url))
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
        })
        .await?;
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut stream = resp.bytes_stream().eventsource();
            while let Some(ev) = stream.next().await {
                let event = match ev {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = tx.send(Err(anyhow!("sse: {e}"))).await;
                        return;
                    }
                };
                let data = event.data;
                if data == "[DONE]" {
                    return;
                }
                if data.is_empty() {
                    continue;
                }
                match serde_json::from_str::<StreamChunk>(&data) {
                    Ok(c) => {
                        if let Some(usage) = c.usage {
                            if tx
                                .send(Ok(StreamEvent::Usage(LlmUsage {
                                    input_tokens: usage.prompt_tokens,
                                    output_tokens: usage.completion_tokens,
                                })))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        for ch in c.choices {
                            if let Some(text) = ch.delta.content {
                                if !text.is_empty()
                                    && tx.send(Ok(StreamEvent::Text(text))).await.is_err()
                                {
                                    return;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_final_stream_usage_chunk() {
        let chunk: StreamChunk = serde_json::from_str(
            r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":34}}"#,
        )
        .unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 34);
    }
}
