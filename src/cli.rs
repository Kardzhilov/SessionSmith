//! Command-line interface definitions.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "sessionsmith",
    version,
    about = "World-class TTRPG session notes from raw audio.",
    long_about = "SessionSmith transcribes session recordings and generates GM-ready notes, \
                  recaps, summaries, story prose and quote reels via a pluggable LLM backend.\n\n\
                  Run with no subcommand for the interactive flow."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Path to the campaign config (default: ./campaign.toml).
    #[arg(long, global = true, env = "SESSIONSMITH_CAMPAIGN")]
    pub campaign: Option<PathBuf>,

    /// Disable ANSI colours.
    #[arg(long, global = true)]
    pub no_color: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// First-run wizard: detect hardware, recommend models, scaffold `campaign.toml`.
    Init(InitArgs),
    /// Re-check dependencies, hardware and backend reachability.
    Doctor(DoctorArgs),
    /// Phase 1 only: audio → transcript.
    Transcribe(TranscribeArgs),
    /// Phase 2 only: transcript → notes.
    Notes(NotesArgs),
    /// Full pipeline: audio → transcript → notes (default when no subcommand).
    Run(RunArgs),
    /// Inspect bundled game-system presets.
    Systems(SystemsArgs),
    /// Manage LLM / ASR models.
    Models(ModelsArgs),
    /// Show or rebuild the rolling campaign log.
    #[command(name = "log")]
    Log(LogArgs),
}

#[derive(Debug, Default, Args)]
pub struct InitArgs {
    /// Use a specific system preset (e.g. `dnd5e`, `pf2e`, `coc`, `blades`,
    /// `daggerheart`, `generic`, `wordsmith`).
    #[arg(long)]
    pub system: Option<String>,
    /// Overwrite an existing `campaign.toml`.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Default, Args)]
pub struct DoctorArgs {
    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TranscribeArgs {
    /// Audio file(s) to transcribe. Omit for interactive picker.
    pub files: Vec<PathBuf>,
    /// Override ASR model (e.g. `large-v3`, `medium`, `base`).
    #[arg(long)]
    pub asr_model: Option<String>,
    /// Force re-transcription even if outputs exist.
    #[arg(long)]
    pub force: bool,
    /// Language hint (e.g. `en`, `auto`).
    #[arg(long, default_value = "auto")]
    pub language: String,
}

#[derive(Debug, Args)]
pub struct NotesArgs {
    /// Transcript file (e.g. `transcripts/session1.txt`). Omit for picker.
    pub transcript: Option<PathBuf>,
    /// Backend to use (`ollama`, `openai`, `anthropic`).
    #[arg(long)]
    pub backend: Option<String>,
    /// Override LLM model name for this run.
    #[arg(long)]
    pub model: Option<String>,
    /// Comma-separated artifacts to emit (default: from `[outputs]` in campaign).
    /// Choices: bullets, dm-notes, recap, summary, story, quotes.
    #[arg(long)]
    pub artifacts: Option<String>,
    /// Skip artifacts whose output file already exists.
    #[arg(long)]
    pub resume: bool,
    /// Force overwrite of existing artifacts.
    #[arg(long)]
    pub force: bool,
    /// Skip updating the rolling campaign log.
    #[arg(long)]
    pub no_log: bool,
}

#[derive(Debug, Default, Args)]
pub struct RunArgs {
    /// Audio file(s). Omit for interactive picker over `audio/`.
    pub files: Vec<PathBuf>,
    /// Backend override.
    #[arg(long)]
    pub backend: Option<String>,
    /// LLM model override.
    #[arg(long)]
    pub model: Option<String>,
    /// ASR model override.
    #[arg(long)]
    pub asr_model: Option<String>,
    /// Comma-separated artifacts (see `notes --help`).
    #[arg(long)]
    pub artifacts: Option<String>,
    /// Skip steps whose outputs already exist.
    #[arg(long)]
    pub resume: bool,
    /// Force overwrite.
    #[arg(long)]
    pub force: bool,
    /// Skip campaign-log update.
    #[arg(long)]
    pub no_log: bool,
    /// Non-interactive: process every audio file newest-first without prompting.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct SystemsArgs {
    #[command(subcommand)]
    pub action: Option<SystemsAction>,
}

#[derive(Debug, Subcommand)]
pub enum SystemsAction {
    /// List bundled presets.
    List,
    /// Print a preset's contents.
    Show {
        /// Preset name (e.g. `dnd5e`).
        name: String,
    },
}

#[derive(Debug, Args)]
pub struct ModelsArgs {
    #[command(subcommand)]
    pub action: Option<ModelsAction>,
}

#[derive(Debug, Subcommand)]
pub enum ModelsAction {
    /// List local models (whisper + Ollama).
    List,
    /// Pull / download a model.
    Pull {
        /// Either a whisper ggml name (`base`, `small`, `medium`, `large-v3`, `large-v3-turbo`)
        /// or an `ollama:` prefixed model id.
        name: String,
    },
    /// Print hardware-based recommendations.
    Recommend,
}

#[derive(Debug, Args)]
pub struct LogArgs {
    #[command(subcommand)]
    pub action: Option<LogAction>,
}

#[derive(Debug, Subcommand)]
pub enum LogAction {
    /// Print the current campaign log.
    Show,
    /// Re-merge all session summaries from scratch.
    Rebuild,
}
