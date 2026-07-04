//! All TUI rendering. Immediate-mode: the frame is laid out from the live
//! terminal size every tick, so resizing reflows automatically.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::Frame;

use crate::prompts::ALL_ARTIFACTS;

use super::app::{App, LogLevel, Overlay, Pane};
use super::markdown;
use super::theme::Theme;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let th = app.theme().clone();

    // Minimum-size guard.
    if area.width < 70 || area.height < 18 {
        let msg = Paragraph::new(format!(
            "Terminal too small\n\n{}×{} — please resize to at least 70×18.",
            area.width, area.height
        ))
        .style(th.base())
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
        frame.render_widget(msg, area);
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    draw_header(frame, app, &th, rows[0]);
    draw_footer(frame, app, &th, rows[2]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Min(0)])
        .split(rows[1]);
    draw_sidebar(frame, app, &th, body[0]);
    draw_content(frame, app, &th, body[1]);

    // Overlays on top.
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => draw_help(frame, &th, area),
        Overlay::Message { .. } => draw_message(frame, app, &th, area),
        Overlay::Palette(_) => draw_palette(frame, app, &th, area),
        Overlay::Search(_) => draw_search(frame, app, &th, area),
        Overlay::Picker(_) => draw_picker(frame, app, &th, area),
        Overlay::ThemePicker { .. } => draw_theme_picker(frame, app, area),
    }
}

fn draw_header(frame: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let campaign = app
        .campaign
        .as_ref()
        .map(|c| c.campaign.name.clone())
        .unwrap_or_else(|| "no campaign".into());
    let left = Line::from(vec![
        Span::styled(" SessionSmith ", th.accent_style()),
        Span::styled(format!("· {campaign}"), th.muted_style()),
    ]);
    let mut rspans: Vec<Span> = Vec::new();
    if !app.mouse_enabled {
        rspans.push(Span::styled(
            " SELECT MODE — press s to resume ",
            Style::default()
                .fg(th.selection_fg)
                .bg(th.warn)
                .add_modifier(Modifier::BOLD),
        ));
        rspans.push(Span::raw(" "));
    }
    rspans.push(Span::styled(
        format!("{} · asr {} ", app.backend_summary(), app.asr_model_label()),
        th.muted_style(),
    ));
    let right = Line::from(rspans);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    frame.render_widget(Paragraph::new(left).style(th.base()), cols[0]);
    frame.render_widget(
        Paragraph::new(right).style(th.base()).alignment(Alignment::Right),
        cols[1],
    );
}

fn draw_footer(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    use super::app::FooterCmd;
    // (key, label, optional clickable command)
    let hints: &[(&str, &str, Option<FooterCmd>)] = match app.pane {
        Pane::Content => &[
            ("↑↓", "scroll", None),
            ("←→", "artifact", None),
            ("e", "editor", Some(FooterCmd::Editor)),
            (":", "palette", Some(FooterCmd::Palette)),
            ("/", "search", Some(FooterCmd::Search)),
            ("?", "help", Some(FooterCmd::Help)),
            ("q", "quit", Some(FooterCmd::Quit)),
        ],
        _ => &[
            ("↹", "pane", None),
            ("↑↓", "move", None),
            ("⏎", "open", None),
            ("r", "run", Some(FooterCmd::Run)),
            ("t", "transcribe", Some(FooterCmd::Transcribe)),
            ("n", "notes", Some(FooterCmd::Notes)),
            ("y", "copy", Some(FooterCmd::Copy)),
            ("s", "select", Some(FooterCmd::Select)),
            ("/", "search", Some(FooterCmd::Search)),
            (":", "palette", Some(FooterCmd::Palette)),
            ("?", "help", Some(FooterCmd::Help)),
        ],
    };
    let mut spans: Vec<Span> = Vec::new();
    let mut hits: Vec<(u16, u16, FooterCmd)> = Vec::new();
    let mut x = area.x;
    let hover_here = app.hover_row == area.y;
    for (k, label, cmd) in hints {
        let key = format!(" {k} ");
        let lbl = format!("{label}  ");
        let start = x;
        let w = (key.chars().count() + lbl.chars().count()) as u16;
        let clickable = cmd.is_some();
        let hovered = clickable && hover_here && app.hover_col >= start && app.hover_col < x + w;
        let mut key_style = if clickable { th.accent_style() } else { th.muted_style() };
        let mut lbl_style = th.muted_style();
        if hovered {
            key_style = key_style.add_modifier(Modifier::UNDERLINED | Modifier::REVERSED);
            lbl_style = lbl_style.add_modifier(Modifier::UNDERLINED);
        }
        spans.push(Span::styled(key, key_style));
        spans.push(Span::styled(lbl, lbl_style));
        if let Some(c) = cmd {
            hits.push((start, x + w, *c));
        }
        x += w;
    }
    app.rects.footer = area;
    app.rects.footer_hits = hits;
    frame.render_widget(Paragraph::new(Line::from(spans)).style(th.base()), area);
}

fn draw_sidebar(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(35),
            Constraint::Percentage(35),
        ])
        .split(area);
    let hover = (app.hover_col, app.hover_row);

    // Campaigns.
    let camp_hov = hovered_index(parts[0], hover, app.camp_state.offset(), app.campaigns.len());
    let camp_items: Vec<ListItem> = app
        .campaigns
        .iter()
        .enumerate()
        .map(|(i, c)| ListItem::new(c.name.clone()).style(hover_style(th, Some(i) == camp_hov)))
        .collect();
    let camp_list = List::new(camp_items)
        .block(section_block("Campaigns", th, app.pane == Pane::Campaigns))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    app.rects.campaigns = parts[0];
    frame.render_stateful_widget(camp_list, parts[0], &mut app.camp_state);

    // Sessions (row 0 is the synthetic Campaign Log).
    let sess_hov = hovered_index(parts[1], hover, app.sess_state.offset(), app.sessions.len() + 1);
    let mut sess_items: Vec<ListItem> = Vec::new();
    sess_items.push(
        ListItem::new(Line::from(vec![
            Span::styled("≡ ", th.accent_style()),
            Span::styled("Campaign Log", th.title_style(false)),
        ]))
        .style(hover_style(th, sess_hov == Some(0))),
    );
    for (i, s) in app.sessions.iter().enumerate() {
        let done = s.artifacts.iter().filter(|b| **b).count();
        let total = s.artifacts.len();
        let mark = if done == total { "✓" } else if done == 0 { "·" } else { "◐" };
        let age = crate::audio::human_age(s.modified);
        sess_items.push(
            ListItem::new(Line::from(vec![
                Span::styled(format!("{mark} "), th.success_style()),
                Span::raw(s.stem.clone()),
                Span::styled(format!("  {done}/{total} · {age}"), th.muted_style()),
            ]))
            .style(hover_style(th, sess_hov == Some(i + 1))),
        );
    }
    let sess_list = List::new(sess_items)
        .block(section_block("Sessions", th, app.pane == Pane::Sessions))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    app.rects.sessions = parts[1];
    frame.render_stateful_widget(sess_list, parts[1], &mut app.sess_state);

    // Audio.
    let audio_hov = hovered_index(parts[2], hover, app.audio_state.offset(), app.audio.len());
    let audio_items: Vec<ListItem> = app
        .audio
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mark = if f.already_transcribed { "✓" } else { "○" };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{mark} "), th.muted_style()),
                Span::raw(f.stem()),
            ]))
            .style(hover_style(th, Some(i) == audio_hov))
        })
        .collect();
    let audio_list = List::new(audio_items)
        .block(section_block("Audio", th, app.pane == Pane::Audio))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    app.rects.audio = parts[2];
    frame.render_stateful_widget(audio_list, parts[2], &mut app.audio_state);
}

fn draw_content(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    // If a job is running or has produced output, reserve the lower part.
    let show_job = app.job_running || !app.job_log.is_empty();
    let (main_area, job_area) = if show_job {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Percentage(40)])
            .split(area);
        (parts[0], Some(parts[1]))
    } else {
        (area, None)
    };

    if app.viewing_log {
        draw_log_viewer(frame, app, th, main_area);
        app.rects.tabs = Rect::default();
        app.rects.tab_ranges.clear();
    } else if app.open_session.is_some() {
        draw_viewer(frame, app, th, main_area);
    } else {
        draw_welcome(frame, app, th, main_area);
        app.rects.tabs = Rect::default();
        app.rects.viewer = Rect::default();
        app.rects.tab_ranges.clear();
    }

    if let Some(job_area) = job_area {
        draw_job(frame, app, th, job_area);
    }
}

fn draw_viewer(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // Artifact tabs — rendered manually so we can record exact hit-test ranges.
    let block = section_block("Artifacts", th, app.pane == Pane::Content);
    let inner = block.inner(parts[0]);
    frame.render_widget(block, parts[0]);

    let sess_arts = app
        .open_session
        .and_then(|si| app.sessions.get(si))
        .map(|s| s.artifacts.clone());
    let mut spans: Vec<Span> = Vec::new();
    let mut ranges: Vec<(u16, u16)> = Vec::new();
    let mut x = inner.x;
    let tab_row_hover = app.hover_row >= parts[0].y && app.hover_row < parts[0].y + parts[0].height;
    for (i, a) in ALL_ARTIFACTS.iter().enumerate() {
        let exists = sess_arts.as_ref().and_then(|v| v.get(i).copied()).unwrap_or(false);
        let mark = if exists { " ✓" } else { "" };
        let label = format!(" {}{} ", a.label(), mark);
        let w = label.chars().count() as u16;
        let selected = i == app.artifact_tab;
        let hovered = tab_row_hover && app.hover_col >= x && app.hover_col < x + w;
        let mut style = if selected {
            Style::default()
                .fg(th.selection_fg)
                .bg(th.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else if exists {
            th.base()
        } else {
            th.muted_style()
        };
        if hovered && !selected {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        ranges.push((x, x + w));
        spans.push(Span::styled(label, style));
        x += w;
        if i + 1 < ALL_ARTIFACTS.len() {
            spans.push(Span::styled("│", th.muted_style()));
            x += 1;
        }
    }
    app.rects.tabs = parts[0];
    app.rects.tab_ranges = ranges;
    frame.render_widget(Paragraph::new(Line::from(spans)).style(th.base()), inner);

    // Preview with markdown rendering + scrollbar.
    let stem = app
        .open_session
        .and_then(|si| app.sessions.get(si))
        .map(|s| s.stem.clone())
        .unwrap_or_default();
    let title = format!("{} · {}", stem, ALL_ARTIFACTS[app.artifact_tab].filename());
    draw_markdown_pane(frame, app, th, parts[1], &title);
}

fn draw_log_viewer(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    draw_markdown_pane(frame, app, th, area, "Campaign Log · _campaign-log.md");
}

/// Render `app.viewer_lines` as markdown into a bordered, scrollable pane.
fn draw_markdown_pane(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect, title: &str) {
    let lines = markdown::render_lines(&app.viewer_lines, th);
    let para = Paragraph::new(Text::from(lines))
        .block(section_block(title, th, app.pane == Pane::Content))
        .style(th.base())
        .wrap(Wrap { trim: false })
        .scroll((app.viewer_scroll, 0));
    app.rects.viewer = area;
    frame.render_widget(para, area);

    let mut sb_state = ScrollbarState::new(app.viewer_lines.len().max(1))
        .position(app.viewer_scroll as usize);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight).style(th.muted_style()),
        area,
        &mut sb_state,
    );
}

fn draw_welcome(frame: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let lines = vec![
        Line::from(Span::styled("SessionSmith", th.accent_style())),
        Line::from(""),
        Line::from("Turn session recordings into GM-ready notes."),
        Line::from(""),
        Line::from(vec![
            Span::styled("Sessions", th.title_style(false)),
            Span::raw("   pick a transcript on the left and press "),
            Span::styled("⏎", th.accent_style()),
            Span::raw(" to read its artifacts."),
        ]),
        Line::from(vec![
            Span::styled("Audio", th.title_style(false)),
            Span::raw("      press "),
            Span::styled("r", th.accent_style()),
            Span::raw(" to run the full pipeline or "),
            Span::styled("t", th.accent_style()),
            Span::raw(" to transcribe only."),
        ]),
        Line::from(vec![
            Span::styled("Palette", th.title_style(false)),
            Span::raw("    press "),
            Span::styled(":", th.accent_style()),
            Span::raw(" (or Ctrl-P) to reach any action; "),
            Span::styled("?", th.accent_style()),
            Span::raw(" for help."),
        ]),
        Line::from(""),
        Line::from(Span::styled(app.status.clone(), th.muted_style())),
    ];
    let para = Paragraph::new(lines)
        .block(section_block("Home", th, app.pane == Pane::Content))
        .style(th.base())
        .wrap(Wrap { trim: true });
    frame.render_widget(para, area);
}

fn draw_job(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let title = if app.job_running {
        format!("Working · {} ⠿", app.job_title)
    } else {
        format!("Job · {}", app.job_title)
    };
    let focused = matches!(app.pane, Pane::Content);
    let block = section_block(&title, th, focused);
    let inner = block.inner(area);
    app.rects.job = area;
    frame.render_widget(block, area);

    // Wrap every log entry to the inner width so long strings roll over.
    let width = (inner.width as usize).max(1);
    let mut rows: Vec<Line> = Vec::new();
    for (lvl, msg) in &app.job_log {
        let (sym, style) = match lvl {
            LogLevel::Ok => ("✓ ", th.success_style()),
            LogLevel::Warn => ("! ", th.warn_style()),
            LogLevel::Error => ("✗ ", th.error_style()),
            LogLevel::Step => ("» ", th.accent_style()),
            LogLevel::Info => ("· ", th.muted_style()),
        };
        let full = format!("{sym}{msg}");
        for chunk in wrap_text(&full, width) {
            rows.push(Line::from(Span::styled(chunk, style)));
        }
    }

    let visible = inner.height as usize;
    let max_off = rows.len().saturating_sub(visible);
    // Re-pin to the bottom when scrolled to (or past) the end.
    if app.job_scroll as usize >= max_off {
        app.job_follow = true;
    }
    let off = if app.job_follow {
        max_off
    } else {
        (app.job_scroll as usize).min(max_off)
    };
    app.job_scroll = off as u16;

    let end = (off + visible).min(rows.len());
    let slice: Vec<Line> = rows[off..end].to_vec();
    frame.render_widget(Paragraph::new(slice).style(th.base()), inner);

    if rows.len() > visible {
        let mut sb = ScrollbarState::new(rows.len()).position(off);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).style(th.muted_style()),
            area,
            &mut sb,
        );
    }
}

/// Word-wrap `text` to `width` columns (falls back to hard splits for long words).
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut line = String::new();
    let mut len = 0usize;
    for word in text.split(' ') {
        let wlen = word.chars().count();
        if wlen > width {
            // Flush current line, then hard-split the long word.
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
                len = 0;
            }
            let mut chunk = String::new();
            for c in word.chars() {
                if chunk.chars().count() == width {
                    out.push(std::mem::take(&mut chunk));
                }
                chunk.push(c);
            }
            if !chunk.is_empty() {
                line = chunk;
                len = line.chars().count();
            }
            continue;
        }
        let extra = if line.is_empty() { wlen } else { wlen + 1 };
        if len + extra > width {
            out.push(std::mem::take(&mut line));
            line.push_str(word);
            len = wlen;
        } else {
            if !line.is_empty() {
                line.push(' ');
                len += 1;
            }
            line.push_str(word);
            len += wlen;
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
    out
}

// ---- overlays ------------------------------------------------------------

fn draw_palette(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let Overlay::Palette(p) = &app.overlay else { return };
    let rect = centered(area, 60, 60);
    frame.render_widget(Clear, rect);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(rect);

    let input = Paragraph::new(Line::from(vec![
        Span::styled("› ", th.accent_style()),
        Span::raw(p.query.clone()),
        Span::styled("▏", th.accent_style()),
    ]))
    .block(popup_block("Command palette", th))
    .style(th.base());
    frame.render_widget(input, parts[0]);

    let labels: Vec<&str> = super::app::Action::all().iter().map(|a| a.label()).collect();
    let hov = hovered_index(
        parts[1],
        (app.hover_col, app.hover_row),
        app.overlay_state.offset(),
        p.filtered.len(),
    );
    let items: Vec<ListItem> = p
        .filtered
        .iter()
        .enumerate()
        .map(|(row, &i)| ListItem::new(labels[i].to_string()).style(hover_style(th, Some(row) == hov)))
        .collect();
    let list = List::new(items)
        .block(popup_block("Actions  (click to run)", th))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    let empty = p.filtered.is_empty();
    let cursor = p.cursor;
    app.overlay_state.select(if empty { None } else { Some(cursor) });
    app.rects.overlay_list = parts[1];
    frame.render_stateful_widget(list, parts[1], &mut app.overlay_state);
}

fn draw_search(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let Overlay::Search(s) = &app.overlay else { return };
    let rect = centered(area, 72, 70);
    frame.render_widget(Clear, rect);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(rect);

    let input = Paragraph::new(Line::from(vec![
        Span::styled("🔎 ", th.accent_style()),
        Span::raw(s.query.clone()),
        Span::styled("▏", th.accent_style()),
    ]))
    .block(popup_block("Search notes", th))
    .style(th.base());
    frame.render_widget(input, parts[0]);

    let hov = hovered_index(
        parts[1],
        (app.hover_col, app.hover_row),
        app.overlay_state.offset(),
        s.hits.len(),
    );
    let items: Vec<ListItem> = s
        .hits
        .iter()
        .enumerate()
        .map(|(row, h)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", h.session), th.accent_style()),
                Span::styled(format!("[{}] ", h.kind), th.muted_style()),
                Span::raw(h.snippet.replace('\n', " ")),
            ]))
            .style(hover_style(th, Some(row) == hov))
        })
        .collect();
    let empty = items.is_empty();
    let list = List::new(items)
        .block(popup_block(
            if empty { "No matches" } else { "Results  (click to open)" },
            th,
        ))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    let cursor = s.cursor;
    let has = !s.hits.is_empty();
    app.overlay_state.select(if has { Some(cursor) } else { None });
    app.rects.overlay_list = parts[1];
    frame.render_stateful_widget(list, parts[1], &mut app.overlay_state);
}

fn draw_picker(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let Overlay::Picker(p) = &app.overlay else { return };
    let rect = centered(area, 64, 70);
    frame.render_widget(Clear, rect);
    let hov = hovered_index(
        rect,
        (app.hover_col, app.hover_row),
        app.overlay_state.offset(),
        p.items.len(),
    );
    let items: Vec<ListItem> = p
        .items
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let checked = p.checked.get(i).copied().unwrap_or(false);
            let box_ = if checked { "[x] " } else { "[ ] " };
            ListItem::new(Line::from(vec![
                Span::styled(box_, if checked { th.success_style() } else { th.muted_style() }),
                Span::raw(label.clone()),
            ]))
            .style(hover_style(th, Some(i) == hov))
        })
        .collect();
    let list = List::new(items)
        .block(popup_block(&p.title, th))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    let empty = p.items.is_empty();
    let cursor = p.cursor;
    app.overlay_state.select(if empty { None } else { Some(cursor) });
    app.rects.overlay_list = rect;
    frame.render_stateful_widget(list, rect, &mut app.overlay_state);
}

fn draw_message(frame: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let Overlay::Message { title, body, error } = &app.overlay else { return };
    let rect = centered(area, 50, 30);
    frame.render_widget(Clear, rect);
    let style = if *error { th.error_style() } else { th.base() };
    let para = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(body.clone(), style)),
        Line::from(""),
        Line::from(Span::styled("Press any key to dismiss.", th.muted_style())),
    ]))
    .block(popup_block(title, th))
    .style(th.base())
    .wrap(Wrap { trim: true });
    frame.render_widget(para, rect);
}

fn draw_theme_picker(frame: &mut Frame, app: &mut App, area: Rect) {
    let cursor = if let Overlay::ThemePicker { cursor, .. } = &app.overlay {
        *cursor
    } else {
        return;
    };
    // The hovered theme is applied live, so `app.theme()` is the preview.
    let th = app.theme().clone();
    let rect = centered(area, 62, 66);
    frame.render_widget(Clear, rect);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(rect);

    let hov = hovered_index(
        cols[0],
        (app.hover_col, app.hover_row),
        app.overlay_state.offset(),
        app.themes.len(),
    );
    let items: Vec<ListItem> = app
        .themes
        .iter()
        .enumerate()
        .map(|(i, t)| ListItem::new(t.name.clone()).style(hover_style(&th, Some(i) == hov)))
        .collect();
    let list = List::new(items)
        .block(popup_block("Themes  (click to preview · ⏎ apply)", &th))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    app.overlay_state.select(Some(cursor));
    app.rects.overlay_list = cols[0];
    frame.render_stateful_widget(list, cols[0], &mut app.overlay_state);

    let sample = vec![
        Line::from(Span::styled("The quick brown fox", th.base())),
        Line::from(""),
        Line::from(vec![
            Span::styled("# Heading ", th.accent_style()),
            Span::styled("primary", th.title_style(true)),
        ]),
        Line::from(vec![
            Span::styled("✓ success  ", th.success_style()),
            Span::styled("! warn  ", th.warn_style()),
            Span::styled("✗ error", th.error_style()),
        ]),
        Line::from(Span::styled("• bullet with muted note", th.muted_style())),
        Line::from(""),
        Line::from(Span::styled(" selected row ", th.selection())),
    ];
    frame.render_widget(
        Paragraph::new(sample)
            .block(popup_block(&th.name, &th))
            .style(th.base())
            .wrap(Wrap { trim: true }),
        cols[1],
    );
}

fn draw_help(frame: &mut Frame, th: &Theme, area: Rect) {
    let rect = centered(area, 64, 80);
    frame.render_widget(Clear, rect);
    let rows = [
        ("Tab / Shift-Tab", "cycle panes"),
        ("↑ ↓  /  j k", "move selection · scroll viewer"),
        ("← →  /  h l  /  1-6", "switch artifact tab"),
        ("Enter", "open session · switch campaign"),
        ("r / t / n", "run pipeline · transcribe · notes"),
        ("e", "open current artifact in $EDITOR"),
        ("y", "copy current view to clipboard"),
        ("s", "select mode (mouse off, drag to select)"),
        ("/", "search notes"),
        (": or Ctrl-P", "command palette"),
        ("T", "cycle theme"),
        ("PgUp / PgDn", "scroll viewer"),
        ("mouse", "click to select · wheel to scroll · hover to highlight"),
        ("q / Ctrl-C", "quit"),
        ("Esc", "close overlay"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!("  {k:<20}"), th.accent_style()),
                Span::styled((*v).to_string(), th.base()),
            ])
        })
        .collect();
    let para = Paragraph::new(lines)
        .block(popup_block("Help — keybindings", th))
        .style(th.base());
    frame.render_widget(para, rect);
}

// ---- helpers -------------------------------------------------------------

fn hover_style(_th: &Theme, hovered: bool) -> Style {
    if hovered {
        Style::default().add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default()
    }
}

/// Map a hover position inside a bordered list `Rect` to an item index.
fn hovered_index(rect: Rect, hover: (u16, u16), offset: usize, len: usize) -> Option<usize> {
    let (col, row) = hover;
    if col < rect.x || col >= rect.x + rect.width {
        return None;
    }
    let inner_top = rect.y + 1;
    if row < inner_top || row >= rect.y + rect.height {
        return None;
    }
    let idx = offset + (row - inner_top) as usize;
    if idx < len {
        Some(idx)
    } else {
        None
    }
}

fn section_block(title: &str, th: &Theme, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(th.border_style(focused))
        .title(Span::styled(format!(" {title} "), th.title_style(focused)))
}

fn popup_block(title: &str, th: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(th.border_focus).add_modifier(Modifier::BOLD))
        .title(Span::styled(format!(" {title} "), th.accent_style()))
}

fn centered(area: Rect, pct_w: u16, pct_h: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_h) / 2),
            Constraint::Percentage(pct_h),
            Constraint::Percentage((100 - pct_h) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_w) / 2),
            Constraint::Percentage(pct_w),
            Constraint::Percentage((100 - pct_w) / 2),
        ])
        .split(v[1])[1]
}
