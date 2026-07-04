//! Rich-style UI helpers: panels, status lines, progress bars, tables.

use comfy_table::{presets, Cell, Color as TableColor, ContentArrangement, Table};
use indicatif::{ProgressBar, ProgressStyle};
use once_cell::sync::Lazy;
use owo_colors::OwoColorize;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// Structured progress events emitted by the pipeline/transcription code.
///
/// When a full-screen TUI installs an event sink via [`set_event_sink`], the
/// `header`/`step`/`ok`/`warn`/`error`/`info` helpers below redirect their
/// output into these events instead of printing to stdout (which would corrupt
/// the alternate screen). The plain CLI leaves the sink unset and keeps its
/// line-oriented output.
#[derive(Clone, Debug)]
pub enum UiEvent {
    Header(String),
    Step { n: usize, total: usize, msg: String },
    Ok(String),
    Warn(String),
    Error(String),
    Info(String),
    /// A determinate/indeterminate progress update. `total == 0` means the
    /// length is unknown (render as an animated/indeterminate indicator).
    Progress { label: String, pos: u64, total: u64 },
    /// A background job finished: `Ok(summary)` or `Err(message)`.
    JobDone(std::result::Result<String, String>),
}

static SINK: Lazy<Mutex<Option<UnboundedSender<UiEvent>>>> = Lazy::new(|| Mutex::new(None));

/// Install (or clear with `None`) the process-wide UI event sink. While a sink
/// is active, the status helpers emit [`UiEvent`]s instead of printing.
pub fn set_event_sink(tx: Option<UnboundedSender<UiEvent>>) {
    if let Ok(mut g) = SINK.lock() {
        *g = tx;
    }
}

/// Whether an event sink is currently installed (i.e. running inside the TUI).
pub fn sink_active() -> bool {
    SINK.lock().map(|g| g.is_some()).unwrap_or(false)
}

fn emit(ev: UiEvent) -> bool {
    if let Ok(g) = SINK.lock() {
        if let Some(tx) = g.as_ref() {
            let _ = tx.send(ev);
            return true;
        }
    }
    false
}

/// Emit a progress update to the TUI (no-op outside the TUI). `total == 0`
/// marks the length as unknown.
pub fn progress(label: &str, pos: u64, total: u64) {
    emit(UiEvent::Progress { label: label.to_string(), pos, total });
}

/// Print a bordered panel with a title and body lines.
pub fn panel(title: &str, lines: &[String]) {
    let width = lines.iter().map(|l| visible_width(l)).max().unwrap_or(0).max(title.len() + 4);
    let bar = "─".repeat(width + 2);
    println!("{} {} {}", "╭".bright_black(), title.bold().cyan(), format!("{}╮", "─".repeat(width.saturating_sub(title.len()))).bright_black());
    let _ = bar; // future use
    for line in lines {
        let pad = " ".repeat(width.saturating_sub(visible_width(line)));
        println!("{} {}{} {}", "│".bright_black(), line, pad, "│".bright_black());
    }
    println!("{}{}{}", "╰".bright_black(), "─".repeat(width + 2).bright_black(), "╯".bright_black());
}

fn visible_width(s: &str) -> usize {
    // Naive: strip ANSI escapes.
    let mut count = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if c == '\x1b' { in_esc = true; continue; }
        if in_esc {
            if c.is_alphabetic() { in_esc = false; }
            continue;
        }
        count += 1;
    }
    count
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
    if emit(UiEvent::Step { n, total, msg: msg.to_string() }) {
        return;
    }
    println!(
        "{} {}",
        format!("[{n}/{total}]").bright_black(),
        msg.bold()
    );
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
