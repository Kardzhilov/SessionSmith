//! Rich-style UI helpers: panels, status lines, progress bars, tables.

use comfy_table::{presets, Cell, Color as TableColor, ContentArrangement, Table};
use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;
use owo_colors::OwoColorize;
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

/// Backwards-compatible name for the shared job event contract.
pub use crate::jobs::report::JobEvent as UiEvent;
use crate::jobs::report::Reporter;

static COLOR_ENABLED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(true));

tokio::task_local! {
    static TASK_REPORTER: Arc<dyn Reporter>;
}

/// Set global terminal colour output. The CLI calls this for `--no-color`.
pub fn set_color_enabled(enabled: bool) {
    if let Ok(mut value) = COLOR_ENABLED.lock() {
        *value = enabled;
    }
    owo_colors::set_override(enabled);
}

/// Whether command output may include ANSI styling.
pub fn color_enabled() -> bool {
    COLOR_ENABLED.lock().map(|value| *value).unwrap_or(true)
}

/// Run a future with an explicit structured reporter instead of terminal
/// output. The scope is task-local, so concurrent jobs cannot overwrite one
/// another's event destination.
pub async fn with_reporter<T>(reporter: Arc<dyn Reporter>, future: impl Future<Output = T>) -> T {
    TASK_REPORTER.scope(reporter, future).await
}

/// Preserve the current task's reporter when spawning a child task. Call this
/// before `tokio::spawn`, since task-local values do not propagate by default.
pub fn inherit_reporter<F>(future: F) -> impl Future<Output = F::Output>
where
    F: Future,
{
    let reporter = TASK_REPORTER.try_with(Clone::clone).ok();
    async move {
        if let Some(reporter) = reporter {
            with_reporter(reporter, future).await
        } else {
            future.await
        }
    }
}

/// Preserve the current task's reporter while running synchronous work on
/// Tokio's blocking pool. Task-local values otherwise do not cross the thread
/// boundary created by `spawn_blocking`.
pub async fn spawn_blocking_with_reporter<F, T>(operation: F) -> Result<T, tokio::task::JoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let reporter = TASK_REPORTER.try_with(Clone::clone).ok();
    tokio::task::spawn_blocking(move || {
        if let Some(reporter) = reporter {
            TASK_REPORTER.sync_scope(reporter, operation)
        } else {
            operation()
        }
    })
    .await
}

/// Whether the current task reports structured events instead of terminal UI.
pub fn sink_active() -> bool {
    TASK_REPORTER.try_with(|_| ()).is_ok()
}

/// Whether the current structured job has requested cooperative cancellation.
pub fn cancellation_requested() -> bool {
    TASK_REPORTER
        .try_with(|reporter| reporter.is_cancelled())
        .unwrap_or(false)
}

fn emit(ev: UiEvent) -> bool {
    TASK_REPORTER
        .try_with(|reporter| reporter.event(ev))
        .is_ok()
}

/// Emit a progress update to the TUI (no-op outside the TUI). `total == 0`
/// marks the length as unknown.
pub fn progress(label: &str, pos: u64, total: u64) {
    progress_with_rate(label, pos, total, None);
}

/// Emit a progress update with an optional unit-per-second rate.
pub fn progress_with_rate(label: &str, pos: u64, total: u64, rate: Option<f64>) {
    emit(UiEvent::Progress {
        label: label.to_string(),
        pos,
        total,
        rate,
    });
}

/// Announce a high-level pipeline phase. Drives the TUI stage timeline; a no-op
/// (aside from an info line) on the plain CLI.
pub fn phase(name: &str) {
    if emit(UiEvent::Phase(name.to_string())) {
        return;
    }
    println!("\n{} {}", "▶".cyan().bold(), name.bold());
}

/// Print a bordered panel with a title and body lines.
pub fn panel(title: &str, lines: &[String]) {
    if emit(UiEvent::Header(title.to_string())) {
        for line in lines {
            let _ = emit(UiEvent::Info(line.clone()));
        }
        return;
    }
    let width = lines
        .iter()
        .map(|l| visible_width(l))
        .max()
        .unwrap_or(0)
        .max(visible_width(title) + 4);
    println!(
        "{} {} {}",
        "╭".bright_black(),
        title.bold().cyan(),
        format!(
            "{}╮",
            "─".repeat(width.saturating_sub(visible_width(title)))
        )
        .bright_black()
    );
    for line in lines {
        let pad = " ".repeat(width.saturating_sub(visible_width(line)));
        println!(
            "{} {}{} {}",
            "│".bright_black(),
            line,
            pad,
            "│".bright_black()
        );
    }
    println!(
        "{}{}{}",
        "╰".bright_black(),
        "─".repeat(width + 2).bright_black(),
        "╯".bright_black()
    );
}

pub(crate) fn visible_width(s: &str) -> usize {
    let mut plain = String::with_capacity(s.len());
    let mut in_esc = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_esc = true;
            continue;
        }
        if in_esc {
            if c.is_alphabetic() {
                in_esc = false;
            }
            continue;
        }
        plain.push(c);
    }
    UnicodeWidthStr::width(plain.as_str())
}

pub fn header(title: &str) {
    if emit(UiEvent::Header(title.to_string())) {
        return;
    }
    println!("\n{} {}\n", "▌".cyan().bold(), title.bold());
}

pub fn ok(msg: &str) {
    if emit(UiEvent::Ok(msg.to_string())) {
        return;
    }
    println!("  {} {}", "✓".green().bold(), msg);
}

pub fn warn(msg: &str) {
    if emit(UiEvent::Warn(msg.to_string())) {
        return;
    }
    println!("  {} {}", "!".yellow().bold(), msg.yellow());
}

pub fn error(msg: &str) {
    if emit(UiEvent::Error(msg.to_string())) {
        return;
    }
    eprintln!("  {} {}", "✗".red().bold(), msg.red());
}

pub fn info(msg: &str) {
    if emit(UiEvent::Info(msg.to_string())) {
        return;
    }
    println!("  {} {}", "·".bright_black(), msg);
}

pub fn step(n: usize, total: usize, msg: &str) {
    if emit(UiEvent::Step {
        n,
        total,
        msg: msg.to_string(),
    }) {
        return;
    }
    println!("{} {}", format!("[{n}/{total}]").bright_black(), msg.bold());
}

/// Notify event-driven hosts that an artifact is ready to be read from disk.
pub fn artifact_written(session: &str, artifact: &str, path: &Path) {
    let _ = emit(UiEvent::ArtifactWritten {
        session: session.into(),
        artifact: artifact.into(),
        path: path.to_path_buf(),
    });
}

pub fn spinner(msg: &str) -> ProgressBar {
    if sink_active() {
        emit(UiEvent::Info(msg.to_string()));
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_message(msg.to_string());
    pb
}

pub fn progress_bar(total: u64, msg: &str) -> ProgressBar {
    if sink_active() {
        emit(UiEvent::Info(msg.to_string()));
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template(
            "  {spinner:.cyan} {msg} [{bar:30.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("=>-"),
    );
    pb.set_message(msg.to_string());
    pb
}

pub fn duration_bar(total_secs: u64, msg: &str) -> ProgressBar {
    if sink_active() {
        emit(UiEvent::Info(msg.to_string()));
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::new(total_secs);
    pb.set_style(
        ProgressStyle::with_template(
            "  {spinner:.cyan} {msg} [{bar:30.cyan/blue}] {pos}s/{len}s ({eta})",
        )
        .unwrap()
        .progress_chars("=>-"),
    );
    pb.set_message(msg.to_string());
    pb
}

pub fn new_table(headers: &[&str]) -> Table {
    let mut t = Table::new();
    t.load_preset(presets::UTF8_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic);
    t.set_header(headers.iter().map(|h| Cell::new(h).fg(TableColor::Cyan)));
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::report::ChannelReporter;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn visible_width_ignores_ansi_and_counts_wide_characters() {
        assert_eq!(visible_width("\x1b[31mred\x1b[0m"), 3);
        assert_eq!(visible_width("表"), 2);
        assert_eq!(visible_width("café"), 4);
    }

    #[tokio::test]
    async fn task_reporter_receives_events_from_ui_helpers_and_spawned_work() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let reporter = Arc::new(ChannelReporter::new(sender, CancellationToken::new()));

        with_reporter(reporter, async {
            phase("Notes");
            tokio::spawn(inherit_reporter(async {
                ok("artifact ready");
            }))
            .await
            .expect("child task should complete");
        })
        .await;

        assert_eq!(receiver.recv().await, Some(UiEvent::Phase("Notes".into())));
        assert_eq!(
            receiver.recv().await,
            Some(UiEvent::Ok("artifact ready".into()))
        );
    }

    #[tokio::test]
    async fn task_reporter_receives_events_from_blocking_work() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let reporter = Arc::new(ChannelReporter::new(sender, CancellationToken::new()));

        with_reporter(reporter, async {
            spawn_blocking_with_reporter(|| {
                ok("blocking artifact ready");
            })
            .await
            .expect("blocking task should complete");
        })
        .await;

        assert_eq!(
            receiver.recv().await,
            Some(UiEvent::Ok("blocking artifact ready".into()))
        );
    }

    #[tokio::test]
    async fn task_reporter_exposes_cancellation_state() {
        let (sender, _receiver) = mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();
        let reporter = Arc::new(ChannelReporter::new(sender, cancellation.clone()));

        with_reporter(reporter, async {
            assert!(!cancellation_requested());
            cancellation.cancel();
            assert!(cancellation_requested());
        })
        .await;
    }
}
