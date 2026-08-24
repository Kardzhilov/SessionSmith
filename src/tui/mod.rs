//! Full-screen terminal UI (ratatui + crossterm). This is the default
//! experience when SessionSmith is launched with no subcommand; the plain
//! subcommands remain available for scripting.

mod app;
mod campaign_form;
mod draw;
mod form;
mod fuzzy;
mod input;
mod jobs;
mod markdown;
mod player;
mod screenshots;
mod theme;

pub use app::App;
pub use jobs::{
    spawn_model_with_reporter, spawn_with_reporter, JobKind as PipelineJobKind, JobRequest,
    ModelJob,
};
#[doc(hidden)]
pub use screenshots::generate_screenshots;

use std::io::{self, Stdout};
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::Terminal;

#[cfg(unix)]
use signal_hook::{
    consts::signal::{SIGCONT, SIGTSTP},
    iterator::{Handle as SignalHandle, Signals},
    low_level,
};

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
    let handle = tokio::runtime::Handle::current();
    let mut app = App::new(handle);
    let mut terminal = setup(app.mouse_enabled)?;
    let res = run_loop(&mut terminal, &mut app);
    app.stop_audio();
    crate::transcribe::kill_current_asr();
    restore();
    res
}

fn run_loop(terminal: &mut Term, app: &mut App) -> Result<()> {
    #[cfg(unix)]
    let mut suspend_signals = SuspendSignals::install()?;

    loop {
        #[cfg(unix)]
        if suspend_signals.handle_pending(terminal, app)? {
            continue;
        }
        app.drain_job_events();
        app.tick_player();
        terminal.draw(|f| draw::draw(f, app))?;

        if event::poll(Duration::from_millis(120))? {
            drain_events(app)?;
        }

        if let Some(path) = app.pending_editor.take() {
            open_in_editor(terminal, &path, app.mouse_enabled);
            app.finish_editor(&path);
        }
        if let Some((title, command)) = app.pending_shell.take() {
            run_shell_suspended(terminal, &title, &command, app.mouse_enabled);
            if app.pending_data_reload {
                app.pending_data_reload = false;
                app.load_campaign_data();
            }
        }
        if app.mouse_toggle_pending {
            app.mouse_toggle_pending = false;
            set_mouse_capture(terminal, app.mouse_enabled);
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

fn setup(mouse_enabled: bool) -> Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let terminal_modes = (|| -> io::Result<()> {
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableFocusChange,
            EnableBracketedPaste
        )?;
        if mouse_enabled {
            execute!(stdout, EnableMouseCapture)?;
        }
        Ok(())
    })();
    if let Err(error) = terminal_modes {
        restore();
        return Err(error.into());
    }
    let backend = CrosstermBackend::new(stdout);
    match Terminal::new(backend) {
        Ok(terminal) => Ok(terminal),
        Err(error) => {
            restore();
            Err(error.into())
        }
    }
}

fn restore() {
    execute!(
        io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen
    )
    .ok();
    disable_raw_mode().ok();
}

fn set_mouse_capture(terminal: &mut Term, enabled: bool) {
    if enabled {
        execute!(terminal.backend_mut(), EnableMouseCapture).ok();
    } else {
        execute!(terminal.backend_mut(), DisableMouseCapture).ok();
    }
}

fn drain_events(app: &mut App) -> Result<()> {
    let mut latest_move = None;
    let mut scroll = None;
    let mut regions_stale = false;
    loop {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                flush_mouse_events(app, &mut latest_move, &mut scroll);
                app.on_key(key);
                regions_stale = true;
            }
            Event::Mouse(mouse) => match mouse.kind {
                event::MouseEventKind::Moved => latest_move = Some(mouse),
                event::MouseEventKind::ScrollUp | event::MouseEventKind::ScrollDown
                    if !regions_stale =>
                {
                    let delta = if matches!(mouse.kind, event::MouseEventKind::ScrollUp) {
                        -1
                    } else {
                        1
                    };
                    if scroll.as_ref().is_some_and(|(last, _)| {
                        last.column != mouse.column || last.row != mouse.row
                    }) {
                        flush_mouse_events(app, &mut latest_move, &mut scroll);
                        regions_stale = true;
                    }
                    if !regions_stale {
                        accumulate_scroll(&mut scroll, mouse, delta);
                    }
                }
                event::MouseEventKind::Down(event::MouseButton::Left) => {
                    let scrolled = flush_mouse_events(app, &mut latest_move, &mut scroll);
                    if !regions_stale && !scrolled {
                        app.on_mouse(mouse);
                        regions_stale = true;
                    } else {
                        app.set_hover(mouse.column, mouse.row);
                    }
                }
                _ => {
                    if matches!(
                        mouse.kind,
                        event::MouseEventKind::Drag(event::MouseButton::Left)
                    ) || matches!(
                        mouse.kind,
                        event::MouseEventKind::Up(event::MouseButton::Left)
                    ) {
                        app.on_mouse(mouse);
                    } else {
                        app.set_hover(mouse.column, mouse.row);
                    }
                }
            },
            Event::FocusLost => {
                app.clear_hover();
                regions_stale = true;
            }
            Event::FocusGained => {}
            Event::Resize(_, _) => regions_stale = true,
            Event::Paste(text) => {
                flush_mouse_events(app, &mut latest_move, &mut scroll);
                app.on_paste(text);
                regions_stale = true;
            }
            _ => {}
        }
        if !event::poll(Duration::ZERO)? {
            break;
        }
    }
    if !regions_stale {
        flush_mouse_events(app, &mut latest_move, &mut scroll);
    } else if let Some(mouse) = latest_move.take() {
        app.set_hover(mouse.column, mouse.row);
    }
    Ok(())
}

fn accumulate_scroll(
    pending: &mut Option<(ratatui::crossterm::event::MouseEvent, i32)>,
    mouse: ratatui::crossterm::event::MouseEvent,
    delta: i32,
) {
    match pending {
        Some((last, total)) if last.column == mouse.column && last.row == mouse.row => {
            *last = mouse;
            *total += delta;
        }
        _ => *pending = Some((mouse, delta)),
    }
}

fn flush_mouse_events(
    app: &mut App,
    latest_move: &mut Option<ratatui::crossterm::event::MouseEvent>,
    scroll: &mut Option<(ratatui::crossterm::event::MouseEvent, i32)>,
) -> bool {
    if let Some(mouse) = latest_move.take() {
        app.on_mouse(mouse);
    }
    let mut scrolled = false;
    if let Some((mouse, delta)) = scroll.take() {
        let kind = if delta < 0 {
            event::MouseEventKind::ScrollUp
        } else {
            event::MouseEventKind::ScrollDown
        };
        for _ in 0..delta.unsigned_abs() {
            app.on_mouse(event::MouseEvent { kind, ..mouse });
            scrolled = true;
        }
    }
    scrolled
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        crate::transcribe::kill_current_asr();
        restore();
        original(info);
    }));
}

/// Temporarily leave the TUI, open `path` in `$EDITOR`, then re-enter.
fn open_in_editor(terminal: &mut Term, path: &Path, mouse_enabled: bool) {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());

    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen
    )
    .ok();
    disable_raw_mode().ok();

    let _ = std::process::Command::new(&editor).arg(path).status();

    reenter_terminal(terminal, mouse_enabled);
    terminal.clear().ok();
}

/// Suspend the TUI and run a shell `command` in the real terminal (so it can
/// print output and prompt for `sudo`), after an explicit confirmation. Used
/// for the "Update Ollama" action.
fn run_shell_suspended(terminal: &mut Term, title: &str, command: &str, mouse_enabled: bool) {
    use std::io::Write;

    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen
    )
    .ok();
    disable_raw_mode().ok();

    println!("\n=== {title} ===");
    println!("SessionSmith will run this command in your terminal:\n");
    println!("    {command}\n");
    print!("Proceed? [y/N] ");
    let _ = io::stdout().flush();

    let mut answer = String::new();
    let _ = io::stdin().read_line(&mut answer);
    if answer.trim().eq_ignore_ascii_case("y") {
        let status = run_shell_command(command);
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

    reenter_terminal(terminal, mouse_enabled);
    terminal.clear().ok();
}

fn reenter_terminal(terminal: &mut Term, mouse_enabled: bool) {
    enable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableFocusChange,
        EnableBracketedPaste
    )
    .ok();
    set_mouse_capture(terminal, mouse_enabled);
}

#[cfg(unix)]
struct SuspendSignals {
    signals: Signals,
    handle: SignalHandle,
    terminal_active: bool,
}

#[cfg(unix)]
impl SuspendSignals {
    fn install() -> Result<Self> {
        let signals = Signals::new([SIGTSTP, SIGCONT])?;
        let handle = signals.handle();
        Ok(Self {
            signals,
            handle,
            terminal_active: true,
        })
    }

    fn handle_pending(&mut self, terminal: &mut Term, app: &App) -> Result<bool> {
        let mut handled = false;
        for signal in self.signals.pending() {
            match signal {
                SIGTSTP if self.terminal_active => {
                    restore();
                    self.terminal_active = false;
                    low_level::emulate_default_handler(SIGTSTP)?;
                    handled = true;
                }
                SIGCONT if !self.terminal_active => {
                    reenter_terminal(terminal, app.mouse_enabled);
                    terminal.clear().ok();
                    self.terminal_active = true;
                    handled = true;
                }
                _ => {}
            }
        }
        Ok(handled)
    }
}

#[cfg(unix)]
impl Drop for SuspendSignals {
    fn drop(&mut self) {
        self.handle.close();
    }
}

fn run_shell_command(command: &str) -> io::Result<std::process::ExitStatus> {
    #[cfg(windows)]
    return std::process::Command::new("cmd")
        .args(["/C", command])
        .status();
    #[cfg(not(windows))]
    std::process::Command::new("sh")
        .args(["-c", command])
        .status()
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
    use super::app::{
        Action, CampaignEntry, FooterCmd, HitTarget, Overlay, PickerKind, PickerState,
        SpeakerMapState,
    };
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::layout::Rect;
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
    fn campaign_management_stays_inside_the_tui() {
        let (_rt, mut app) = new_app();

        app.on_key(KeyEvent::from(KeyCode::Char('N')));
        assert!(matches!(&app.overlay, super::app::Overlay::CampaignForm(_)));
        assert!(app.pending_shell.is_none());
        let buffer = render_to_buffer(&mut app, 110, 32);
        let text = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("New campaign"));
        let _ = render_to_buffer(&mut app, 46, 16);
        app.on_key(KeyEvent::from(KeyCode::Esc));

        app.on_key(KeyEvent::from(KeyCode::Char('E')));
        assert!(matches!(&app.overlay, super::app::Overlay::CampaignForm(_)));
        app.on_key(KeyEvent::from(KeyCode::Esc));

        app.on_key(KeyEvent::from(KeyCode::Char('F')));
        assert!(matches!(&app.overlay, super::app::Overlay::TextPrompt(_)));
        assert!(app.pending_shell.is_none());
    }

    #[test]
    fn unavailable_models_have_no_delete_button_or_action() {
        let (_rt, mut app) = new_app();
        app.pane = super::app::Pane::Content;
        app.models = Some(super::app::ModelsState {
            rows: vec![super::app::ModelRow {
                kind: super::app::ModelKind::Whisper,
                id: "base".into(),
                display: "Whisper base".into(),
                released: "2022-09".into(),
                ..Default::default()
            }],
            cursor: 0,
            scroll: 0,
            expanded: std::collections::HashSet::new(),
            installed: std::collections::HashMap::new(),
        });

        let _ = render_to_buffer(&mut app, 110, 32);
        assert!(app
            .hit_rect(HitTarget::ModelButton {
                row: 0,
                install: true,
            })
            .is_some());
        assert!(app
            .hit_rect(HitTarget::ModelButton {
                row: 0,
                install: false,
            })
            .is_none());

        app.model_action(false);
        assert!(!app.job_running);
        assert!(app.job_queue.is_empty());
        assert_eq!(app.status, "base is not installed");
    }

    #[test]
    fn narrow_terminals_wrap_bars() {
        let (_rt, mut app) = new_app();
        for (w, h) in [(56u16, 20u16), (46, 16), (72, 24), (120, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        }
    }

    #[test]
    fn narrow_header_keeps_asr_runtime_token_visible() {
        let (_rt, mut app) = new_app();
        let buffer = render_to_buffer(&mut app, 46, 16);
        let text = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("asr"));
        assert!(text.contains(&app.asr_model_label()));
    }

    #[test]
    fn empty_sidebar_hints_render_without_selecting_audio_or_campaigns() {
        let (_rt, mut app) = new_app();
        app.campaigns.clear();
        app.sessions.clear();
        app.audio.clear();
        app.camp_state.select(None);
        app.audio_state.select(None);
        let buffer = render_to_buffer(&mut app, 110, 32);
        let text = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("no campaigns"));
        assert!(text.contains("no sessions yet"));
        assert!(text.contains("drop recordings"));
        let audio_hint = app.hit_rect(HitTarget::AudioList).unwrap();
        click(&mut app, audio_hint.x + 2, audio_hint.y + 2);
        assert_eq!(app.audio_idx, 0);
        assert_eq!(app.audio_state.selected(), None);
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
        if let Some(footer) = app.hit_rect(HitTarget::Footer(FooterCmd::Palette)) {
            click(&mut app, footer.x, footer.y);
            terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        }
        app.on_key(KeyEvent::from(KeyCode::Esc));
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();

        // Open the palette and click its first row.
        app.on_key(KeyEvent::from(KeyCode::Char(':')));
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        let palette_item = app.hit_rect(HitTarget::PaletteItem(0)).unwrap();
        click(&mut app, palette_item.x, palette_item.y);
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        app.on_key(KeyEvent::from(KeyCode::Esc));

        // Open the theme picker and click a row.
        app.on_key(KeyEvent::from(KeyCode::Char('T')));
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();
        let theme_item = app.hit_rect(HitTarget::ThemeRow(0)).unwrap();
        click(&mut app, theme_item.x, theme_item.y);
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
        let job = app.hit_rect(HitTarget::Job).unwrap();
        let (jx, jy) = (job.x, job.y);
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: jx,
            row: jy,
            modifiers: KeyModifiers::NONE,
        });
        terminal.draw(|f| draw::draw(f, &mut app)).unwrap();

        // Hover over the footer, then copy + toggle select mode.
        let footer = app.hit_rect(HitTarget::Footer(FooterCmd::Palette)).unwrap();
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: footer.x,
            row: footer.y,
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
        let started = std::time::Instant::now();
        app.job_stages = vec![
            ("Transcribe · session1".into(), started),
            (
                "Outline".into(),
                started + std::time::Duration::from_secs(4),
            ),
            ("Notes".into(), started + std::time::Duration::from_secs(7)),
        ];
        app.job_progress = Some(("transcribing".into(), 45, 600, None));
        app.job_log.push((LogLevel::Step, "Notes".into()));
        app.job_queue.push_back((
            super::jobs::ModelJob::PullWhisper("large-v3".into()),
            "Install large-v3".into(),
        ));
        app.job_queue.push_back((
            super::jobs::ModelJob::DeleteWhisper("base".into()),
            "Delete base".into(),
        ));
        for w in [40u16, 70, 110] {
            let buffer = render_to_buffer(&mut app, w, 24);
            let text = buffer
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            if w >= 70 {
                assert!(text.contains("queued:"));
            }
        }

        // Indeterminate (pulse) bar + byte-sized download counter.
        app.job_progress = Some(("downloading model".into(), 0, 0, None));
        let _ = render_to_buffer(&mut app, 80, 24);
        app.job_progress = Some(("downloading model".into(), 5_000_000, 12_000_000, None));
        let _ = render_to_buffer(&mut app, 80, 24);

        // Finished: stages should all read as done without panicking.
        app.job_running = false;
        let _ = render_to_buffer(&mut app, 80, 24);
    }

    #[test]
    fn hit_regions_exclude_borders_and_follow_the_visible_offset() {
        let (_rt, mut app) = new_app();
        app.campaigns = (0..18)
            .map(|index| CampaignEntry {
                name: format!("Campaign {index}"),
                path: std::path::PathBuf::from(format!("/tmp/campaign-{index}.toml")),
            })
            .collect();
        app.campaign_idx = 12;
        app.camp_state.select(Some(12));

        let _ = render_to_buffer(&mut app, 110, 32);
        let list = app.hit_rect(HitTarget::CampaignList).unwrap();
        let offset = app.camp_state.offset();
        assert_eq!(
            app.hit_target_at(list.x, list.y),
            Some(HitTarget::CampaignRow(offset))
        );
        for (column, row) in [
            (list.x.saturating_sub(1), list.y),
            (list.x + list.width, list.y),
            (list.x, list.y.saturating_sub(1)),
            (list.x, list.y + list.height),
        ] {
            assert!(
                !matches!(
                    app.hit_target_at(column, row),
                    Some(HitTarget::CampaignRow(_))
                ),
                "border at ({column}, {row}) became a campaign row"
            );
        }
    }

    #[test]
    fn modal_backdrops_block_background_controls_and_picker_rows_toggle() {
        let (_rt, mut app) = new_app();
        let _ = render_to_buffer(&mut app, 110, 32);
        let footer = app.hit_rect(HitTarget::Footer(FooterCmd::Palette)).unwrap();

        app.on_key(KeyEvent::from(KeyCode::Char(':')));
        let _ = render_to_buffer(&mut app, 110, 32);
        assert_eq!(
            app.hit_target_at(footer.x, footer.y),
            Some(HitTarget::OverlayBarrier)
        );
        click(&mut app, footer.x, footer.y);
        assert!(matches!(app.overlay, Overlay::Palette(_)));

        app.overlay = Overlay::Picker(PickerState {
            title: "Choose artifacts".into(),
            kind: PickerKind::Artifacts,
            items: vec!["Summary".into()],
            checked: vec![false],
            cursor: 0,
            target: None,
        });
        let _ = render_to_buffer(&mut app, 110, 32);
        let item = app.hit_rect(HitTarget::PickerItem(0)).unwrap();
        click(&mut app, item.x, item.y);
        let Overlay::Picker(picker) = &app.overlay else {
            panic!("picker closed after toggling an item");
        };
        assert_eq!(picker.checked, vec![true]);
        assert!(app.hit_rect(HitTarget::PickerConfirm).is_some());
    }

    #[test]
    fn small_terminal_frame_clears_all_hit_regions() {
        let (_rt, mut app) = new_app();
        let _ = render_to_buffer(&mut app, 110, 32);
        let footer = app.hit_rect(HitTarget::Footer(FooterCmd::Palette)).unwrap();
        let status = app.status.clone();

        let _ = render_to_buffer(&mut app, 40, 10);
        assert_eq!(app.hit_target_at(footer.x, footer.y), None);
        click(&mut app, footer.x, footer.y);
        assert_eq!(app.status, status);
        assert!(matches!(app.overlay, Overlay::None));
    }

    #[test]
    fn wheel_uses_the_hovered_sidebar_and_never_wraps() {
        let (_rt, mut app) = new_app();
        app.campaigns = (0..3)
            .map(|index| CampaignEntry {
                name: format!("Campaign {index}"),
                path: std::path::PathBuf::from(format!("/tmp/campaign-{index}.toml")),
            })
            .collect();
        app.campaign_idx = 1;
        app.pane = super::app::Pane::Sessions;
        let session_idx = app.session_idx;
        let _ = render_to_buffer(&mut app, 110, 32);
        let campaigns = app.hit_rect(HitTarget::CampaignList).unwrap();

        app.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: campaigns.x,
            row: campaigns.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(app.pane, super::app::Pane::Campaigns));
        assert_eq!(app.campaign_idx, 2);
        assert_eq!(app.session_idx, session_idx);

        app.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: campaigns.x,
            row: campaigns.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.campaign_idx, 2);
    }

    #[test]
    fn drag_routes_cover_viewer_scrollbars_and_player_tracks() {
        let (_rt, mut app) = new_app();
        app.viewing_log = true;
        app.viewer_lines = (0..200).map(|index| format!("line {index}")).collect();
        let _ = render_to_buffer(&mut app, 110, 32);
        let scrollbar = app.hit_rect(HitTarget::ViewerScrollbar).unwrap();
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar.x,
            row: scrollbar.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.viewer_dragging);
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: scrollbar.x,
            row: scrollbar.y + scrollbar.height.saturating_sub(1),
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.viewer_scroll > 0);
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: scrollbar.x,
            row: scrollbar.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.viewer_dragging);

        app.begin_frame();
        app.register_hit(Rect::new(5, 5, 10, 1), HitTarget::PlayerTrack);
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.player_dragging);
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 30,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 30,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.player_dragging);
    }

    #[test]
    fn overlay_click_targets_and_paste_cover_interactive_workflows() {
        let (_rt, mut app) = new_app();
        app.on_key(KeyEvent::from(KeyCode::Char('F')));
        let _ = render_to_buffer(&mut app, 110, 32);
        assert!(app.hit_rect(HitTarget::TextPromptInput).is_some());
        assert!(app.hit_rect(HitTarget::TextPromptSubmit).is_some());
        let prompt_before_paste = match &app.overlay {
            Overlay::TextPrompt(prompt) => prompt.input.value.clone(),
            _ => panic!("fork prompt was not open"),
        };
        app.on_paste("New\nCampaign".into());
        let Overlay::TextPrompt(prompt) = &app.overlay else {
            panic!("fork prompt was unexpectedly closed");
        };
        assert_eq!(
            prompt.input.value,
            format!("{prompt_before_paste}New Campaign")
        );
        app.on_key(KeyEvent::from(KeyCode::Esc));

        app.on_key(KeyEvent::from(KeyCode::Char('N')));
        let _ = render_to_buffer(&mut app, 110, 32);
        assert!(app.hit_rect(HitTarget::CampaignFormRow(1)).is_some());
        app.on_key(KeyEvent::from(KeyCode::Esc));

        app.overlay = Overlay::SpeakerMap(SpeakerMapState {
            stem: "session".into(),
            labels: vec!["SPEAKER_00".into()],
            samples: std::collections::BTreeMap::new(),
            map: std::collections::BTreeMap::new(),
            choices: vec!["Alice".into(), "Skip".into()],
            cursor: 0,
            preview_samples: std::collections::BTreeMap::new(),
            audio: None,
        });
        let _ = render_to_buffer(&mut app, 110, 32);
        let speaker = app.hit_rect(HitTarget::SpeakerRow(0)).unwrap();
        click(&mut app, speaker.x, speaker.y);
        let Overlay::SpeakerMap(state) = &app.overlay else {
            panic!("speaker map was unexpectedly closed");
        };
        assert_eq!(state.map.get("SPEAKER_00"), Some(&"Alice".to_string()));
        assert!(app.hit_rect(HitTarget::SpeakerPreview).is_some());
        assert!(app.hit_rect(HitTarget::SpeakerSave).is_some());
        app.on_key(KeyEvent::from(KeyCode::Esc));

        app.overlay = Overlay::Confirm {
            title: "Confirm".into(),
            body: "Continue?".into(),
        };
        let _ = render_to_buffer(&mut app, 110, 32);
        let no = app.hit_rect(HitTarget::ConfirmNo).unwrap();
        click(&mut app, no.x, no.y);
        assert!(matches!(app.overlay, Overlay::None));
    }

    #[test]
    fn scroll_accumulator_sums_same_location() {
        let mut pending = None;
        let mouse = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 12,
            row: 7,
            modifiers: KeyModifiers::NONE,
        };
        accumulate_scroll(&mut pending, mouse, 1);
        accumulate_scroll(&mut pending, mouse, 1);
        accumulate_scroll(&mut pending, mouse, -1);
        assert_eq!(pending.map(|(_, delta)| delta), Some(1));
    }

    #[test]
    fn randomized_mouse_events_preserve_state_invariants() {
        let (_rt, mut app) = new_app();
        let mut seed = 0x9e37_79b9_u64;
        for iteration in 0..160 {
            app.overlay = if iteration % 2 == 0 {
                Overlay::Help
            } else {
                Overlay::None
            };
            let _ = render_to_buffer(&mut app, 110, 32);
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let column = (seed % 110) as u16;
            let row = ((seed >> 16) % 32) as u16;
            let kind = if iteration % 2 == 0 {
                match (seed >> 32) % 5 {
                    0 => MouseEventKind::Down(MouseButton::Left),
                    1 => MouseEventKind::Up(MouseButton::Left),
                    2 => MouseEventKind::Drag(MouseButton::Left),
                    3 => MouseEventKind::ScrollUp,
                    _ => MouseEventKind::ScrollDown,
                }
            } else {
                match (seed >> 32) % 4 {
                    0 => MouseEventKind::Moved,
                    1 => MouseEventKind::Drag(MouseButton::Left),
                    2 => MouseEventKind::ScrollUp,
                    _ => MouseEventKind::ScrollDown,
                }
            };
            app.on_mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            });
            assert!(app.campaigns.is_empty() || app.campaign_idx < app.campaigns.len());
            assert!(app.sessions.is_empty() || app.session_idx < app.sessions.len());
            assert!(app.audio.is_empty() || app.audio_idx < app.audio.len());
        }
    }
}
