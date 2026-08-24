//! Shared job orchestration primitives.
//!
//! The first extraction keeps the legacy CLI/TUI behavior intact while giving
//! new hosts a UI-independent progress and cancellation contract.

pub mod import_audio;
pub mod manager;
pub mod procs;
pub mod record;
pub mod report;
