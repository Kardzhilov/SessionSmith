//! Rich-style UI helpers: panels, status lines, progress bars, tables.

use comfy_table::{presets, Cell, Color as TableColor, ContentArrangement, Table};
use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use std::time::Duration;

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
    println!("\n{} {}\n", "▌".cyan().bold(), title.bold());
}

pub fn ok(msg: &str) {
    println!("  {} {}", "✓".green().bold(), msg);
}

pub fn warn(msg: &str) {
    println!("  {} {}", "!".yellow().bold(), msg.yellow());
}

pub fn error(msg: &str) {
    eprintln!("  {} {}", "✗".red().bold(), msg.red());
}

pub fn info(msg: &str) {
    println!("  {} {}", "·".bright_black(), msg);
}

pub fn step(n: usize, total: usize, msg: &str) {
    println!(
        "{} {}",
        format!("[{n}/{total}]").bright_black(),
        msg.bold()
    );
}

pub fn spinner(msg: &str) -> ProgressBar {
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
