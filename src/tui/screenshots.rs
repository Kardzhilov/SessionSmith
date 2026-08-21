//! Off-screen renderer that turns real TUI frames into SVG image files, used to
//! generate the screenshots in the README. It renders the *actual* widgets and
//! theme colours — not a mock-up — but against a **throwaway dummy workspace**
//! so no real campaign data is ever exposed. Invoked via
//! `cargo run --example gen_screenshots`.

use std::io::Write;
use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::{Color, Modifier};

use super::app::{App, LogLevel, Pane};

// Monospace cell metrics (px) and window chrome.
const CW: f32 = 8.6;
const CH: f32 = 17.4;
const FS: f32 = 14.0;
const PAD: f32 = 14.0;
const BAR: f32 = 30.0;

/// Render every README screenshot into `out_dir` as an SVG.
///
/// A self-contained dummy workspace (fake campaign, sessions, notes, audio and
/// global config) is created in a temp directory and used for rendering, so the
/// screenshots never contain the user's real campaigns or logs.
pub fn generate_screenshots(out_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(out_dir)?;
    // Resolve the output dir to an absolute path *before* we change cwd.
    let out_abs = std::fs::canonicalize(out_dir)?;

    let original_cwd = std::env::current_dir()?;
    let dummy = std::env::temp_dir().join("sessionsmith-screenshots");
    build_dummy_env(&dummy)?;

    // Isolate config + working directory so App::new loads only dummy data.
    std::env::set_var("XDG_CONFIG_HOME", dummy.join("config"));
    std::env::set_current_dir(&dummy)?;

    let result = render_all(&out_abs);

    // Always restore the original working directory.
    std::env::set_current_dir(&original_cwd).ok();
    result
}

fn render_all(out_dir: &Path) -> std::io::Result<()> {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let handle = rt.handle().clone();

    let (w, h) = (108u16, 30u16);

    // 1) Dashboard with a session open.
    let mut app = App::new(handle.clone());
    set_theme(&mut app, "midnight");
    select_campaign(&mut app, "Emberfall");
    app.pane = Pane::Sessions;
    app.on_key(KeyEvent::from(KeyCode::Enter)); // open newest session
    let buf = super::render_to_buffer(&mut app, w, h);
    write_svg(&out_dir.join("dashboard.svg"), &buf, &app, "SessionSmith")?;

    // 2) Command palette.
    let mut app = App::new(handle.clone());
    set_theme(&mut app, "midnight");
    select_campaign(&mut app, "Emberfall");
    app.on_key(KeyEvent::from(KeyCode::Char(':')));
    let buf = super::render_to_buffer(&mut app, w, h);
    write_svg(
        &out_dir.join("palette.svg"),
        &buf,
        &app,
        "SessionSmith — command palette",
    )?;

    // 3) Theme picker, previewing gruvbox.
    let mut app = App::new(handle.clone());
    set_theme(&mut app, "midnight");
    select_campaign(&mut app, "Emberfall");
    app.on_key(KeyEvent::from(KeyCode::Char('T')));
    app.on_key(KeyEvent::from(KeyCode::Down));
    app.on_key(KeyEvent::from(KeyCode::Down)); // → gruvbox
    let buf = super::render_to_buffer(&mut app, w, h);
    write_svg(
        &out_dir.join("themes.svg"),
        &buf,
        &app,
        "SessionSmith — themes",
    )?;

    // 4) Live pipeline / doctor pane.
    let mut app = App::new(handle.clone());
    set_theme(&mut app, "midnight");
    select_campaign(&mut app, "Emberfall");
    app.job_title = "Run pipeline".into();
    app.job_running = true;
    for (lvl, msg) in [
        (LogLevel::Step, "[1/1] the-drowned-bell"),
        (
            LogLevel::Info,
            "transcribing the-drowned-bell with whisper-large-v3",
        ),
        (LogLevel::Info, "ASR device: cuda (float16)"),
        (
            LogLevel::Ok,
            "wrote output/emberfall/transcripts/the-drowned-bell.txt",
        ),
        (
            LogLevel::Ok,
            "wrote output/emberfall/notes/the-drowned-bell/bullets.md",
        ),
        (
            LogLevel::Ok,
            "wrote output/emberfall/notes/the-drowned-bell/recap.md",
        ),
        (LogLevel::Info, "summary … ~1.2k tokens"),
    ] {
        app.job_log.push((lvl, msg.to_string()));
    }
    let buf = super::render_to_buffer(&mut app, w, h);
    write_svg(
        &out_dir.join("pipeline.svg"),
        &buf,
        &app,
        "SessionSmith — live pipeline",
    )?;

    Ok(())
}

/// Write a self-contained fake workspace (config + campaign + notes + audio).
fn build_dummy_env(root: &Path) -> std::io::Result<()> {
    // Start clean so stale files never leak between runs.
    if root.exists() {
        std::fs::remove_dir_all(root)?;
    }

    let cfg_dir = root.join("config/sessionsmith");
    std::fs::create_dir_all(&cfg_dir)?;
    std::fs::write(
        cfg_dir.join("config.toml"),
        "[backend]\nkind = \"ollama\"\nmodel = \"qwen2.5:32b\"\n\n\
         [asr]\nmodel = \"large-v3\"\n\n\
         [ui]\ntheme = \"midnight\"\n",
    )?;

    std::fs::create_dir_all(root.join("campaigns"))?;
    std::fs::write(
        root.join("campaigns/Emberfall.toml"),
        "[campaign]\nname = \"Emberfall\"\ngm = \"Ava\"\nsetting = \"the Sundered Coast\"\n\n\
         [[players]]\nplayer = \"Ren\"\ncharacter = \"Vael\"\nancestry = \"Elf\"\nclass = \"Rogue\"\n\n\
         [[players]]\nplayer = \"Jo\"\ncharacter = \"Bram\"\nancestry = \"Dwarf\"\nclass = \"Cleric\"\n\n\
         [system]\npreset = \"dnd5e\"\n",
    )?;

    let notes = root.join("output/emberfall/notes");
    let tx = root.join("output/emberfall/transcripts");
    std::fs::create_dir_all(&tx)?;
    std::fs::create_dir_all(&notes)?;

    // Two sessions; the-drowned-bell is written last so it sorts newest.
    write_session(&tx, &notes, "ashford-gate", ASHFORD_BULLETS, ASHFORD_RECAP)?;
    write_session(&tx, &notes, "the-drowned-bell", BELL_BULLETS, BELL_RECAP)?;

    std::fs::write(notes.join("_campaign-log.md"), CAMPAIGN_LOG)?;

    // Audio drop-zone: two transcribed (✓) and one pending (○).
    let audio = root.join("audio");
    std::fs::create_dir_all(&audio)?;
    for f in [
        "ashford-gate.flac",
        "the-drowned-bell.flac",
        "the-tidewatch.flac",
    ] {
        std::fs::write(audio.join(f), b"")?;
    }
    Ok(())
}

fn write_session(
    tx: &Path,
    notes: &Path,
    stem: &str,
    bullets: &str,
    recap: &str,
) -> std::io::Result<()> {
    std::fs::write(tx.join(format!("{stem}.txt")), "dummy transcript\n")?;
    let dir = notes.join(stem);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("bullets.md"), bullets)?;
    std::fs::write(dir.join("recap.md"), recap)?;
    // The remaining artifacts just need to exist for the ✓ markers/6-of-6 count.
    for f in ["dm-notes.md", "summary.md", "story.md", "quotes.md"] {
        std::fs::write(dir.join(f), "# Dummy artifact\n\nExample content.\n")?;
    }
    Ok(())
}

const BELL_BULLETS: &str = "\
# Session Outline

## The Drowned Bell

- The party rows into the flooded undercroft beneath Ashford Chapel at low tide.
- **Vael** spots a bell-rope trailing into the black water and hauls up a barnacled hand-bell.
- Bram blesses the water; the *tide-cult* wards flicker and briefly go dark.
- A drowned cantor rises and demands the bell be rung \"three times, and never a fourth.\"
- Vael rings it twice; the cantor bows and points to a sealed vault door.
- The sigil on the vault matches the tattoo on the ferryman from Session 2.
- Loot: a coral key, 40 gp in sodden coin, and a waterlogged ledger naming three debtors.
- The party retreats as the tide returns, vowing to come back with the ferryman.
";

const BELL_RECAP: &str = "\
# Recap — The Drowned Bell

The crew slipped into the undercroft beneath Ashford Chapel while the tide was
out. Vael fished a strange hand-bell from the flooded nave, and Bram's blessing
briefly quieted the tide-cult's wards. A drowned cantor gave a single warning —
ring the bell three times, *never* a fourth — and revealed a sealed vault whose
sigil ties back to the ferryman. With a coral key and a debtors' ledger in hand,
the party fell back before the water rose.
";

const ASHFORD_BULLETS: &str = "\
# Session Outline

## Ashford Gate

- The party argues past the gate wardens with a forged writ.
- Bram recognises the captain from a past pilgrimage and smooths things over.
- A pickpocket lifts Vael's coin purse; a short chase ends on the chapel steps.
";

const ASHFORD_RECAP: &str = "\
# Recap — Ashford Gate

A forged writ and a lucky reunion got the party through Ashford Gate, though
Vael lost a purse to a nimble thief before the night was done.
";

const CAMPAIGN_LOG: &str = "\
# Emberfall — Campaign Log

## Session 1 — Ashford Gate
The party talked their way into the walled town of Ashford and made a first,
uneasy contact with the harbour ferryman.

## Session 2 — The Drowned Bell
Beneath the chapel the crew recovered a warded hand-bell and a coral key, and
uncovered a debtors' ledger linking the ferryman to a sealed vault.

## Ongoing Threads
- What lies behind the sealed vault door?
- Who are the three debtors named in the ledger?
- The tide-cult knows the party rang the bell.
";

fn select_campaign(app: &mut App, name: &str) {
    if let Some(idx) = app.campaigns.iter().position(|c| c.name == name) {
        app.campaign_idx = idx;
        app.load_campaign_data();
    }
}

fn set_theme(app: &mut App, name: &str) {
    if let Some(idx) = app.themes.iter().position(|t| t.name == name) {
        app.theme_idx = idx;
    }
}

/// Per-theme page background/foreground for the SVG canvas (cells with a
/// `Reset` colour inherit these, matching how the theme looks in a terminal).
fn page_colors(theme_name: &str) -> (&'static str, &'static str) {
    match theme_name {
        "solar" => ("#fdf6e3", "#586e75"),
        "gruvbox" => ("#282828", "#ebdbb2"),
        "mono" => ("#1c1c1c", "#d0d0d0"),
        _ => ("#16161e", "#c0caf5"), // midnight
    }
}

fn write_svg(path: &Path, buf: &Buffer, app: &App, title: &str) -> std::io::Result<()> {
    let (page_bg, page_fg) = page_colors(&app.theme().name);
    let area = buf.area;
    let cols = area.width;
    let rows = area.height;

    let content_w = PAD * 2.0 + cols as f32 * CW;
    let content_h = BAR + PAD + rows as f32 * CH + PAD;

    let mut s = String::new();
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" \
         viewBox=\"0 0 {w:.0} {h:.0}\" font-family=\"'SFMono-Regular',Menlo,Consolas,'DejaVu Sans Mono',monospace\" \
         font-size=\"{fs}\">\n",
        w = content_w,
        h = content_h,
        fs = FS,
    ));
    // Window background + title bar.
    s.push_str(&format!(
        "<rect width=\"{w:.0}\" height=\"{h:.0}\" rx=\"10\" fill=\"{bg}\"/>\n",
        w = content_w,
        h = content_h,
        bg = page_bg
    ));
    for (i, c) in ["#ff5f56", "#ffbd2e", "#27c93f"].iter().enumerate() {
        s.push_str(&format!(
            "<circle cx=\"{cx}\" cy=\"15\" r=\"6\" fill=\"{c}\"/>\n",
            cx = 18.0 + i as f32 * 20.0
        ));
    }
    s.push_str(&format!(
        "<text x=\"{x:.0}\" y=\"20\" fill=\"{fg}\" text-anchor=\"middle\" \
         font-size=\"12\" opacity=\"0.6\">{title}</text>\n",
        x = content_w / 2.0,
        fg = page_fg,
        title = xml_escape(title),
    ));

    let ox = PAD;
    let oy = BAR + PAD;

    // First pass: background rects. Second pass: glyphs.
    for y in 0..rows {
        for x in 0..cols {
            let cell = &buf[(x, y)];
            let (_, bgc) = resolve(cell, page_fg, page_bg);
            if bgc != page_bg {
                s.push_str(&format!(
                    "<rect x=\"{px:.2}\" y=\"{py:.2}\" width=\"{cw:.2}\" height=\"{ch:.2}\" fill=\"{bg}\"/>\n",
                    px = ox + x as f32 * CW,
                    py = oy + y as f32 * CH,
                    cw = CW + 0.5,
                    ch = CH,
                    bg = bgc,
                ));
            }
        }
    }
    for y in 0..rows {
        for x in 0..cols {
            let cell = &buf[(x, y)];
            let sym = cell.symbol();
            if sym.trim().is_empty() {
                continue;
            }
            let (fgc, _) = resolve(cell, page_fg, page_bg);
            let m = cell.modifier;
            let mut extra = String::new();
            if m.contains(Modifier::BOLD) {
                extra.push_str(" font-weight=\"bold\"");
            }
            if m.contains(Modifier::ITALIC) {
                extra.push_str(" font-style=\"italic\"");
            }
            if m.contains(Modifier::UNDERLINED) {
                extra.push_str(" text-decoration=\"underline\"");
            }
            if m.contains(Modifier::DIM) {
                extra.push_str(" opacity=\"0.6\"");
            }
            s.push_str(&format!(
                "<text x=\"{px:.2}\" y=\"{py:.2}\" fill=\"{fg}\"{extra} xml:space=\"preserve\">{t}</text>\n",
                px = ox + x as f32 * CW,
                py = oy + y as f32 * CH + CH * 0.76,
                fg = fgc,
                extra = extra,
                t = xml_escape(sym),
            ));
        }
    }

    s.push_str("</svg>\n");

    let mut f = std::fs::File::create(path)?;
    f.write_all(s.as_bytes())
}

/// Resolve a cell's (fg, bg) to concrete CSS hex, applying REVERSED.
fn resolve(cell: &ratatui::buffer::Cell, page_fg: &str, page_bg: &str) -> (String, String) {
    let mut fg = color_hex(cell.fg).unwrap_or_else(|| page_fg.to_string());
    let mut bg = color_hex(cell.bg).unwrap_or_else(|| page_bg.to_string());
    if cell.modifier.contains(Modifier::REVERSED) {
        std::mem::swap(&mut fg, &mut bg);
    }
    (fg, bg)
}

fn color_hex(c: Color) -> Option<String> {
    Some(match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Reset => return None,
        Color::Black => "#1c1c1c".into(),
        Color::Red => "#f7768e".into(),
        Color::Green => "#9ece6a".into(),
        Color::Yellow => "#e0af68".into(),
        Color::Blue => "#7aa2f7".into(),
        Color::Magenta => "#bb9af7".into(),
        Color::Cyan => "#7dcfff".into(),
        Color::Gray => "#a9b1d6".into(),
        Color::DarkGray => "#565f89".into(),
        Color::LightRed => "#ff7a93".into(),
        Color::LightGreen => "#b9f27c".into(),
        Color::LightYellow => "#ff9e64".into(),
        Color::LightBlue => "#7dcfff".into(),
        Color::LightMagenta => "#c0a4ff".into(),
        Color::LightCyan => "#a4daff".into(),
        Color::White => "#ffffff".into(),
        Color::Indexed(_) => return None,
    })
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
