use anyhow::Result;
use clap::Parser;

use sessionsmith::cli::{Cli, Command};
use sessionsmith::commands;

#[tokio::main]
async fn main() -> Result<()> {
    // Honour RUST_LOG; default to warn so the UI stays clean.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .compact()
        .init();

    // Install Ctrl-C handler: kills any running ASR child process first so
    // VRAM is freed immediately, then exits.
    ctrlc::set_handler(|| {
        eprintln!();
        sessionsmith::transcribe::kill_current_asr();
        std::process::exit(130);
    })
    .ok();

    let cli = Cli::parse();

    // Propagate --campaign / -C into env so resolve_campaign() picks it up
    // regardless of which sub-command is running.
    if let Some(ref p) = cli.campaign {
        std::env::set_var("SESSIONSMITH_CAMPAIGN", p);
    }

    let exit = match cli.command {
        Some(Command::Init(args)) => commands::init::run(args).await,
        Some(Command::Doctor(args)) => commands::doctor::run(args).await,
        Some(Command::Transcribe(args)) => commands::transcribe::run(args).await,
        Some(Command::Notes(args)) => commands::notes::run(args).await,
        Some(Command::Run(args)) => commands::run::run(args).await,
        Some(Command::Systems(args)) => commands::systems::run(args).await,
        Some(Command::Models(args)) => commands::models::run(args).await,
        Some(Command::Log(args)) => commands::log_cmd::run(args).await,
        None => commands::home::run().await,
    };

    if let Err(err) = exit {
        sessionsmith::ui::error(&format!("{err:#}"));
        std::process::exit(1);
    }
    Ok(())
}
