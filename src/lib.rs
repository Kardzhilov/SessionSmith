//! SessionSmith — world-class TTRPG session notes CLI.
//!
//! Library entrypoint exposes the modules used by the binary and tests.

pub mod audio;
pub mod cli;
pub mod commands;
pub mod config;
pub mod deps;
pub mod hardware;
pub mod llm;
pub mod models;
pub mod pipeline;
pub mod presets;
pub mod prompts;
pub mod session;
pub mod transcribe;
pub mod ui;

/// Convenience result alias used across the crate.
pub type Result<T> = anyhow::Result<T>;
