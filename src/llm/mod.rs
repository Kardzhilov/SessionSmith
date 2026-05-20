//! LLM backend abstraction with three implementations: Ollama, OpenAI-compatible,
//! Anthropic. All stream tokens via an async channel.

pub mod ollama;
pub mod openai;
pub mod anthropic;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
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
    while let Some(chunk) = rx.recv().await {
        let chunk = chunk?;
        // Sentinel chunks starting with NUL are spinner-only progress updates
        // (e.g. thinking-token counts from Qwen3). Don't include in output.
        if let Some(rest) = chunk.strip_prefix('\x00') {
            if let Some(pb) = spinner {
                if let Some(n) = rest.strip_prefix("thinking:") {
                    pb.set_message(format!("thinking · ~{n} tokens (reasoning…)"));
                }
            }
            continue;
        }
        tokens += chunk.split_whitespace().count();
        out.push_str(&chunk);
        if let Some(pb) = spinner {
            pb.set_message(format!("streaming · ~{tokens} tokens"));
        }
    }
    Ok(out)
}
