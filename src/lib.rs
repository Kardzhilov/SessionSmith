//! SessionSmith — world-class TTRPG session notes CLI.
//!
//! Library entrypoint exposes the modules used by the binary and tests.

pub mod audio;
pub mod asr;
pub mod campaign_log;
pub mod cli;
pub mod commands;
pub mod config;
pub mod deps;
pub mod hardware;
pub mod index;
pub mod llm;
pub mod meta;
pub mod models;
pub mod pipeline;
pub mod presets;
pub mod prompts;
pub mod pybridge;
pub mod session;
pub mod speakers;
pub mod transcribe;
pub mod transcribe_cpp;
pub mod tui;
pub mod ui;
pub mod util;
#[cfg(feature = "local-whisper")]
pub mod whisper_local;

/// Convenience result alias used across the crate.
pub type Result<T> = anyhow::Result<T>;
