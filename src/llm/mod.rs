//! LLM backend abstraction with three implementations: Ollama, OpenAI-compatible,
//! Anthropic. All stream tokens via an async channel.

pub mod ollama;
pub mod openai;
pub mod anthropic;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::future::Future;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::config::GlobalConfig;

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy)]
pub enum Role { System, User, Assistant }

impl Role {
    pub fn as_str(self) -> &'static str {
        match self { Role::System => "system", Role::User => "user", Role::Assistant => "assistant" }
    }
}

#[derive(Debug, Clone)]
pub struct ChatOptions {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub timeout: Duration,
    /// Allow thinking/reasoning models to use chain-of-thought.
    pub think: bool,
    /// Context window size (Ollama `num_ctx`). `None` uses the backend default.
    pub num_ctx: Option<u32>,
    /// When set, request structured output constrained to this JSON schema
    /// (Ollama `format`, OpenAI `response_format`). Ignored by backends that
    /// don't support it.
    pub format: Option<serde_json::Value>,
}

impl ChatOptions {
    /// Construct options with the extended fields defaulted (no context
    /// override, free-form output).
    pub fn new(model: String, temperature: Option<f32>, max_tokens: Option<u32>, timeout: Duration, think: bool) -> Self {
        Self { model, temperature, max_tokens, timeout, think, num_ctx: None, format: None }
    }
}

#[async_trait]
pub trait LlmBackend: Send + Sync {
    fn name(&self) -> &'static str;

    /// Returns a channel receiver of token chunks. Sender closes on completion.
    /// Errors mid-stream are sent as `Err`.
    async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        opts: ChatOptions,
    ) -> Result<mpsc::Receiver<Result<String>>>;
}

pub fn build(g: &GlobalConfig) -> Result<Box<dyn LlmBackend>> {
    match g.backend.kind.as_str() {
        "ollama" => Ok(Box::new(ollama::OllamaBackend::from_config(g)?)),
        "openai" => Ok(Box::new(openai::OpenAIBackend::from_config(g)?)),
        "anthropic" => Ok(Box::new(anthropic::AnthropicBackend::from_config(g)?)),
        other => Err(anyhow!("unknown backend '{other}'")),
    }
}

fn retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 529)
}

/// Retry request initiation only. Once a response is returned, each backend's
/// stream parser owns it and any later failure is surfaced without replaying
/// partially emitted content.
pub async fn send_with_retry<F, Fut>(backend: &str, mut send: F) -> Result<reqwest::Response>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<reqwest::Response, reqwest::Error>>,
{
    const RETRY_DELAYS: [Duration; 3] = [
        Duration::from_secs(1),
        Duration::from_secs(4),
        Duration::from_secs(10),
    ];
    for (attempt, fallback_delay) in RETRY_DELAYS.into_iter().enumerate() {
        match send().await {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => {
                let status = response.status();
                if !retryable_status(status) {
                    let text = response.text().await.unwrap_or_default();
                    return Err(anyhow!("{backend} HTTP {status}: {text}"));
                }
                let delay = response.headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(fallback_delay);
                crate::ui::warn(&format!(
                    "{backend}: attempt {}/4 failed with HTTP {status}; retrying in {}s",
                    attempt + 1,
                    delay.as_secs(),
                ));
                tokio::time::sleep(delay).await;
            }
            Err(error) if error.is_connect() || error.is_timeout() => {
                crate::ui::warn(&format!(
                    "{backend}: attempt {}/4 failed ({error}); retrying in {}s",
                    attempt + 1,
                    fallback_delay.as_secs(),
                ));
                tokio::time::sleep(fallback_delay).await;
            }
            Err(error) => return Err(error.into()),
        }
    }

    match send().await {
        Ok(response) if response.status().is_success() => Ok(response),
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(anyhow!("{backend} HTTP {status} after retries: {text}"))
        }
        Err(error) => Err(error.into()),
    }
}

/// Release any GPU memory the configured LLM backend is holding so the ASR
/// stage (or the next session) has VRAM to work with. For Ollama this unloads
/// all resident models; remote API backends hold no local VRAM, so it's a
/// no-op. Controlled by `runtime.auto_free_vram` (on by default) and always
/// best-effort — failures never interrupt the pipeline.
pub async fn free_vram(g: &GlobalConfig) {
    if !g.runtime.auto_free_vram {
        return;
    }
    if !g.backend.kind.eq_ignore_ascii_case("ollama") {
        return;
    }
    let base = g
        .backend
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434".into());
    let freed = ollama::unload_all(&base).await;
    if !freed.is_empty() {
        crate::ui::info(&format!("freed GPU memory: unloaded {}", freed.join(", ")));
    }
}

/// Collect a streamed chat into a String while updating an optional spinner with
/// a running token count.
pub async fn collect(
    backend: &dyn LlmBackend,
    messages: Vec<ChatMessage>,
    opts: ChatOptions,
    spinner: Option<&indicatif::ProgressBar>,
) -> Result<String> {
    let mut rx = backend.stream_chat(messages, opts).await?;
    let mut out = String::new();
    let mut tokens = 0usize;
    let mut last_emit = 0usize;
    while let Some(chunk) = rx.recv().await {
        let chunk = chunk?;
        // Sentinel chunks starting with NUL are spinner-only progress updates
        // (e.g. thinking-token counts from Qwen3). Don't include in output.
        if let Some(rest) = chunk.strip_prefix('\x00') {
            if let Some(n) = rest.strip_prefix("thinking:") {
                if let Some(pb) = spinner {
                    pb.set_message(format!("thinking · ~{n} tokens (reasoning…)"));
                }
                crate::ui::progress(&format!("thinking · ~{n} tokens (reasoning…)"), 0, 0);
            }
            continue;
        }
        tokens += chunk.split_whitespace().count();
        out.push_str(&chunk);
        if let Some(pb) = spinner {
            pb.set_message(format!("streaming · ~{tokens} tokens"));
        }
        // Throttle progress events to the TUI so we don't flood the channel.
        if tokens >= last_emit + 16 {
            last_emit = tokens;
            crate::ui::progress(&format!("generating · ~{tokens} tokens"), 0, 0);
        }
    }
    Ok(out)
}
