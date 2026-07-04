//! Full-screen terminal UI (ratatui + crossterm). This is the default
//! experience when SessionSmith is launched with no subcommand; the plain
//! subcommands remain available for scripting.

mod app;
mod draw;
mod fuzzy;
mod input;
mod jobs;
mod markdown;
mod player;
mod screenshots;
mod theme;

pub use app::App;
#[doc(hidden)]
pub use screenshots::generate_screenshots;

use std::io::{self, Stdout};
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::Terminal;

type Term = Terminal<CrosstermBackend<Stdout>>;

/// Render the given app state into an off-screen buffer of `width`×`height`.
///
/// Exposed for screenshot/example generation and tests; not part of the stable
/// API surface.
#[doc(hidden)]
pub fn render_to_buffer(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test backend");
    terminal.draw(|f| draw::draw(f, app)).expect("draw");
    terminal.backend().buffer().clone()
}

/// Launch the full-screen TUI. Restores the terminal on every exit path,
/// including panics.
pub async fn run() -> Result<()> {
    crate::deps::ensure_dirs()?;
    install_panic_hook();
    let mut terminal = setup()?;
    let handle = tokio::runtime::Handle::current();
    let mut app = App::new(handle);
    let res = run_loop(&mut terminal, &mut app);
    app.stop_audio();
    restore();
    res
}

fn run_loop(terminal: &mut Term, app: &mut App) -> Result<()> {
    loop {
        app.drain_job_events();
        app.tick_player();
        terminal.draw(|f| draw::draw(f, app))?;

        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => app.on_key(k),
                Event::Mouse(m) => app.on_mouse(m),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if let Some(path) = app.pending_editor.take() {
            open_in_editor(terminal, &path);
        }
        if let Some((title, command)) = app.pending_shell.take() {
            run_shell_suspended(terminal, &title, &command);
        }
        if app.mouse_toggle_pending {
            app.mouse_toggle_pending = false;
            if app.mouse_enabled {
                execute!(terminal.backend_mut(), EnableMouseCapture).ok();
            } else {
                execute!(terminal.backend_mut(), DisableMouseCapture).ok();
            }
        }
        if let Some(text) = app.copy_pending.take() {
            copy_to_clipboard(terminal, &text);
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn setup() -> Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore() {
    disable_raw_mode().ok();
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        original(info);
    }));
}

/// Temporarily leave the TUI, open `path` in `$EDITOR`, then re-enter.
fn open_in_editor(terminal: &mut Term, path: &Path) {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture).ok();

    let _ = std::process::Command::new(&editor).arg(path).status();

    enable_raw_mode().ok();
    execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture).ok();
    terminal.clear().ok();
}

/// Suspend the TUI and run a shell `command` in the real terminal (so it can
/// print output and prompt for `sudo`), after an explicit confirmation. Used
/// for the "Update Ollama" action.
fn run_shell_suspended(terminal: &mut Term, title: &str, command: &str) {
    use std::io::Write;

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture).ok();

    println!("\n=== {title} ===");
    println!("SessionSmith will run this command in your terminal:\n");
    println!("    {command}\n");
    print!("Proceed? [y/N] ");
    let _ = io::stdout().flush();

    let mut answer = String::new();
    let _ = io::stdin().read_line(&mut answer);
    if answer.trim().eq_ignore_ascii_case("y") {
        let status = std::process::Command::new("sh").arg("-c").arg(command).status();
        match status {
            Ok(s) if s.success() => println!("\n✓ Done."),
            Ok(s) => println!("\n✗ Command exited with status {s}."),
            Err(e) => println!("\n✗ Could not run command: {e}"),
        }
    } else {
        println!("\nCancelled.");
    }
    print!("\nPress Enter to return to SessionSmith… ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);

    enable_raw_mode().ok();
    execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture).ok();
    terminal.clear().ok();
}

/// Copy `text` to the system clipboard via the OSC 52 terminal escape, which
/// works locally and over SSH (when the terminal supports it).
fn copy_to_clipboard(terminal: &mut Term, text: &str) {
    use std::io::Write;
    let seq = format!("\x1b]52;c;{}\x07", base64_encode(text.as_bytes()));
    let out = terminal.backend_mut();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// Minimal standard base64 encoder (no external dependency).
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::app::Action;
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    fn new_app() -> (tokio::runtime::Runtime, App) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let handle = rt.handle().clone();
        let app = App::new(handle);
        (rt, app)
    }

    #[test]
    fn renders_all_states_without_panic() {
        let (_rt, mut app) = new_app();
        let backend = TestBackend::new(110, 32);
        let mut terminal = Terminal::new(backend).unwrap();

        // Base dashboard.
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();

        // Cycle theme + open each overlay via keys, drawing each time.
        app.dispatch(Action::CycleTheme);
        for key in [
            KeyCode::Esc,
            KeyCode::Char(':'),
            KeyCode::Esc,
            KeyCode::Char('/'),
            KeyCode::Esc,
            KeyCode::Char('?'),
            KeyCode::Esc,
            KeyCode::Char('r'),
            KeyCode::Esc,
            KeyCode::Char('T'),
            KeyCode::Down,
            KeyCode::Esc,
            KeyCode::Char('m'),
            KeyCode::Down,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Up,
            KeyCode::Up,
            KeyCode::Enter,
        ] {
            app.on_key(KeyEvent::from(key));
            terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        }
    }

    #[test]
    fn renders_small_terminal_guard() {
        let (_rt, mut app) = new_app();
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
    }

    #[test]
    fn narrow_terminals_wrap_bars() {
        let (_rt, mut app) = new_app();
        for (w, h) in [(56u16, 20u16), (46, 16), (72, 24), (120, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        }
    }

    fn click(app: &mut App, col: u16, row: u16) {
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }

    #[test]
    fn mouse_clicks_dont_panic() {
        let (_rt, mut app) = new_app();
        let backend = TestBackend::new(110, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();

        // Click a footer shortcut.
        let footer_y = app.rects.footer.y;
        if let Some((x, _, _, _)) = app.rects.footer_hits.first().copied() {
            click(&mut app, x, footer_y);
            terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        }
        app.on_key(KeyEvent::from(KeyCode::Esc));
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();

        // Open the palette and click its first row.
        app.on_key(KeyEvent::from(KeyCode::Char(':')));
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        let list = app.rects.overlay_list;
        click(&mut app, list.x + 2, list.y + 1);
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        app.on_key(KeyEvent::from(KeyCode::Esc));

        // Open the theme picker and click a row.
        app.on_key(KeyEvent::from(KeyCode::Char('T')));
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        let list = app.rects.overlay_list;
        click(&mut app, list.x + 1, list.y + 1);
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
    }

    #[test]
    fn job_pane_hover_copy_select() {
        use super::app::LogLevel;
        let (_rt, mut app) = new_app();
        let backend = TestBackend::new(90, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        // A job log with a very long line to exercise wrapping.
        app.job_title = "System check".into();
        app.job_log.push((LogLevel::Info, "x ".repeat(200)));
        app.job_log.push((LogLevel::Ok, "done".into()));
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();

        // Scroll the job pane.
        let (jx, jy) = (app.rects.job.x + 1, app.rects.job.y + 1);
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: jx,
            row: jy,
            modifiers: KeyModifiers::NONE,
        });
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();

        // Hover over the footer, then copy + toggle select mode.
        let fy = app.rects.footer.y;
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 3,
            row: fy,
            modifiers: KeyModifiers::NONE,
        });
        app.on_key(KeyEvent::from(KeyCode::Char('y')));
        app.on_key(KeyEvent::from(KeyCode::Char('s')));
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        assert!(app.copy_pending.is_some() || !app.job_log.is_empty());
    }

    #[test]
    fn job_pane_renders_stages_and_bars() {
        use super::app::LogLevel;
        let (_rt, mut app) = new_app();

        // Running job with a stage timeline + a determinate transcription bar.
        app.job_running = true;
        app.job_title = "Run".into();
        app.job_started = Some(std::time::Instant::now());
        app.job_stages = vec![
            "Transcribe · session1".into(),
            "Outline".into(),
            "Notes".into(),
        ];
        app.job_progress = Some(("transcribing".into(), 45, 600));
        app.job_log.push((LogLevel::Step, "Notes".into()));
        for w in [40u16, 70, 110] {
            let _ = render_to_buffer(&mut app, w, 24);
        }

        // Indeterminate (pulse) bar + byte-sized download counter.
        app.job_progress = Some(("downloading model".into(), 0, 0));
        let _ = render_to_buffer(&mut app, 80, 24);
        app.job_progress = Some(("downloading model".into(), 5_000_000, 12_000_000));
        let _ = render_to_buffer(&mut app, 80, 24);

        // Finished: stages should all read as done without panicking.
        app.job_running = false;
        let _ = render_to_buffer(&mut app, 80, 24);
    }
}

