use anyhow::Result;
use clap::Parser;
use std::io::Write;

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
    install_crash_report_hook();

    // Install Ctrl-C handler: kills any running ASR child process first so
    // VRAM is freed immediately, then exits.
    ctrlc::set_handler(|| {
        if sessionsmith::commands::run::request_watch_stop() {
            eprintln!("stopping watch after the current recording; press Ctrl-C again to exit immediately");
            return;
        }
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
        Some(Command::Search(args)) => commands::search::run(args).await,
        Some(Command::Record(args)) => commands::record::run(args).await,
        Some(Command::Export(args)) => commands::export::run(args).await,
        None => {
            // Full-screen TUI by default; `--no-tui` (or `[ui] legacy_menu`)
            // falls back to the line-based menu.
            let legacy = cli.no_tui
                || sessionsmith::config::GlobalConfig::load_or_default()
                    .map(|g| g.ui.legacy_menu)
                    .unwrap_or(false);
            if legacy {
                commands::home::run().await
            } else {
                sessionsmith::tui::run().await
            }
        }
    };

    if let Err(err) = exit {
        sessionsmith::ui::error(&format!("{err:#}"));
        std::process::exit(1);
    }
    Ok(())
}

fn install_crash_report_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        original(info);
        sessionsmith::transcribe::kill_current_asr();
        let report = crash_report(info);
        eprintln!("\n{report}");
        if let Some(path) = write_crash_report(&report) {
            eprintln!("Crash report saved to {}", path.display());
        }
    }));
}

fn crash_report(info: &std::panic::PanicHookInfo<'_>) -> String {
    format!(
        "SessionSmith crashed.\n\n{info}\n\nVersion: {}\nPlatform: {}-{}\nRe-run with RUST_BACKTRACE=1 and report this output at {}.",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        env!("CARGO_PKG_REPOSITORY"),
    )
}

fn write_crash_report(report: &str) -> Option<std::path::PathBuf> {
    let directory = dirs::cache_dir()?.join("sessionsmith");
    std::fs::create_dir_all(&directory).ok()?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let path = directory.join(format!("crash-{timestamp}.log"));
    let mut file = std::fs::File::create(&path).ok()?;
    file.write_all(report.as_bytes()).ok()?;
    file.write_all(b"\n").ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_log_writer_persists_the_report() {
        let report = "test crash report";
        let path = write_crash_report(report).expect("cache directory should be available");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "test crash report\n"
        );
        std::fs::remove_file(path).unwrap();
    }
}
