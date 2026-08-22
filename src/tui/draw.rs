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

use super::app::{App, HitTarget, LogLevel, ModelKind, Overlay, Pane};
use super::markdown;
use super::theme::Theme;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let th = app.theme().clone();
    app.tick = app.tick.wrapping_add(1);
    app.begin_frame();

    // Minimum-size guard.
    if area.width < 44 || area.height < 12 {
        let msg = Paragraph::new(format!(
            "Terminal too small\n\n{}×{} — please resize to at least 44×12.",
            area.width, area.height
        ))
        .style(th.base())
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
        frame.render_widget(msg, area);
        return;
    }

    // Header and footer wrap onto multiple rows when the terminal is narrow, so
    // no content is ever clipped. Their heights are computed from the width.
    let header_h = header_lines(app, &th, area.width).len().max(1) as u16;
    let footer_hints = footer_hints(app.pane);
    let fwidths: Vec<u16> = footer_hints.iter().map(tok_width).collect();
    let footer_h = assign_rows(&fwidths, area.width).len().max(1) as u16;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),
            Constraint::Min(0),
            Constraint::Length(if app.player.is_some() { 1 } else { 0 }),
            Constraint::Length(if app.status.trim().is_empty() { 0 } else { 1 }),
            Constraint::Length(footer_h),
        ])
        .split(area);
    draw_header(frame, app, &th, rows[0]);
    if app.player.is_some() {
        draw_player_bar(frame, app, &th, rows[2]);
    }
    if !app.status.trim().is_empty() {
        let line = Line::from(vec![
            Span::styled(" › ", th.accent_style()),
            Span::styled(app.status.clone(), th.muted_style()),
        ]);
        frame.render_widget(Paragraph::new(line).style(th.base()), rows[3]);
    }
    draw_footer(frame, app, &th, rows[4]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Min(0)])
        .split(rows[1]);
    draw_sidebar(frame, app, &th, body[0]);
    draw_content(frame, app, &th, body[1]);

    // Overlays on top.
    if !matches!(app.overlay, Overlay::None) {
        let backdrop = if matches!(app.overlay, Overlay::Help | Overlay::Message { .. }) {
            HitTarget::OverlayDismiss
        } else {
            HitTarget::OverlayBarrier
        };
        app.register_hit(area, backdrop);
    }
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => draw_help(frame, app, &th, area),
        Overlay::Message { .. } => draw_message(frame, app, &th, area),
        Overlay::Confirm { .. } => draw_confirm(frame, app, &th, area),
        Overlay::Palette(_) => draw_palette(frame, app, &th, area),
        Overlay::Search(_) => draw_search(frame, app, &th, area),
        Overlay::Picker(_) => draw_picker(frame, app, &th, area),
        Overlay::CampaignForm(_) => draw_campaign_form(frame, app, &th, area),
        Overlay::TextPrompt(_) => draw_text_prompt(frame, app, &th, area),
        Overlay::SpeakerMap(_) => draw_speaker_map(frame, app, &th, area),
        Overlay::ThemePicker { .. } => draw_theme_picker(frame, app, area),
    }
}

/// A one-line transport bar for the active audio player: icon, label, elapsed /
/// total time, and a progress bar. Shown above the footer while playing.
fn draw_player_bar(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (pos, dur, paused, label, volume) = {
        let Some(player) = &app.player else { return };
        (
            player.position(),
            player.duration(),
            player.paused,
            player.label.chars().take(22).collect::<String>(),
            player.volume,
        )
    };
    let icon = if paused { "⏸" } else { "▶" };
    let time = if dur > 0.0 {
        let pct = ((pos / dur).clamp(0.0, 1.0) * 100.0).round() as u16;
        format!(
            "{} / {}  {pct}%",
            super::player::fmt_time(pos),
            super::player::fmt_time(dur)
        )
    } else {
        format!("{} / --:--", super::player::fmt_time(pos))
    };
    let prefix = format!(" {icon} {label}  {time}  ");
    let pw = prefix.chars().count();
    let hint = format!("  🔊 {}%  space ⏯ · , . seek · - + vol · S stop", volume);
    let hw = hint.chars().count();
    let total = area.width as usize;
    let bar_w = total.saturating_sub(pw + hw).max(6);
    let last = bar_w - 1;
    let player_track = Rect::new(area.x + pw as u16, area.y, bar_w as u16, 1);
    app.register_hit(player_track, HitTarget::PlayerTrack);

    // Progress track with a distinct ● knob at the current position. When the
    // duration is unknown the knob sweeps back and forth so there's still motion.
    let played_style = if paused {
        th.muted_style()
    } else {
        th.success_style()
    };
    let knob_style = if app.is_hovering(HitTarget::PlayerTrack) {
        th.accent_style()
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        th.accent_style().add_modifier(Modifier::BOLD)
    };
    let knob = if dur > 0.0 {
        ((pos / dur).clamp(0.0, 1.0) * last as f64).round() as usize
    } else {
        let cycle = last.max(1) * 2;
        let t = (app.tick as usize / 2) % cycle;
        if t <= last {
            t
        } else {
            cycle - t
        }
    }
    .min(last);
    let bar_spans = vec![
        Span::styled("━".repeat(knob), played_style),
        Span::styled("●".to_string(), knob_style),
        Span::styled("─".repeat(last - knob), th.muted_style()),
    ];

    let mut spans = vec![Span::styled(prefix, th.accent_style())];
    spans.extend(bar_spans);
    spans.push(Span::styled(hint, th.muted_style()));
    frame.render_widget(Paragraph::new(Line::from(spans)).style(th.base()), area);
}

fn draw_header(frame: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let lines = header_lines(app, th, area.width);
    frame.render_widget(Paragraph::new(lines).style(th.base()), area);
}

/// Build the header as atomic tokens wrapped to `width` rows.
fn header_lines(app: &App, th: &Theme, width: u16) -> Vec<Line<'static>> {
    let campaign = app
        .campaign
        .as_ref()
        .map(|c| c.campaign.name.clone())
        .unwrap_or_else(|| "no campaign".into());

    // Each token stays intact; whole tokens wrap onto new rows when narrow.
    let mut toks: Vec<(String, Style)> = vec![
        (" SessionSmith ".to_string(), th.accent_style()),
        (format!("· {campaign}  "), th.muted_style()),
        (format!("{}  ", app.backend_summary()), th.muted_style()),
        (
            format!(
                "· asr {} ({}) ",
                app.asr_model_label(),
                app.asr_runtime_label()
            ),
            th.muted_style(),
        ),
        (
            format!(
                "· diarize {} ",
                if app.asr_diarize() { "on" } else { "off" }
            ),
            if app.asr_diarize() {
                th.success_style()
            } else {
                th.muted_style()
            },
        ),
    ];
    if !app.mouse_enabled {
        toks.push((
            " SELECT MODE — press s to resume ".to_string(),
            Style::default()
                .fg(th.selection_fg)
                .bg(th.warn)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let widths: Vec<u16> = toks.iter().map(|(t, _)| t.chars().count() as u16).collect();
    assign_rows(&widths, width)
        .into_iter()
        .map(|row| {
            Line::from(
                row.into_iter()
                    .map(|i| Span::styled(toks[i].0.clone(), toks[i].1))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// A clickable footer token: `key` + `label`, optionally bound to a command.
struct FTok {
    key: &'static str,
    label: &'static str,
    cmd: Option<super::app::FooterCmd>,
}

fn tok_width(t: &FTok) -> u16 {
    // Rendered as " {key} " + "{label}  " → (key + 2) + (label + 2).
    (t.key.chars().count() + 2 + t.label.chars().count() + 2) as u16
}

fn footer_hints(pane: Pane) -> Vec<FTok> {
    use super::app::FooterCmd as F;
    let mk = |key, label, cmd| FTok { key, label, cmd };
    match pane {
        Pane::Content => vec![
            mk("↑↓", "scroll", None),
            mk("←→", "artifact", None),
            mk("e", "editor", Some(F::Editor)),
            mk("y", "copy", Some(F::Copy)),
            mk("s", "select", Some(F::Select)),
            mk(":", "palette", Some(F::Palette)),
            mk("/", "search", Some(F::Search)),
            mk("?", "help", Some(F::Help)),
            mk("q", "quit", Some(F::Quit)),
        ],
        Pane::Campaigns => vec![
            mk("↹", "pane", None),
            mk("↑↓", "move", None),
            mk("⇧↑↓", "reorder", None),
            mk("⏎", "switch", None),
            mk("N", "new", Some(F::NewCampaign)),
            mk("E", "edit", Some(F::EditCampaign)),
            mk("F", "fork", Some(F::ForkCampaign)),
            mk("r", "run", Some(F::Run)),
            mk(":", "palette", Some(F::Palette)),
            mk("/", "search", Some(F::Search)),
            mk("?", "help", Some(F::Help)),
            mk("q", "quit", Some(F::Quit)),
        ],
        Pane::Audio => vec![
            mk("↹", "pane", None),
            mk("↑↓", "move", None),
            mk("p", "play", None),
            mk("⏎", "open", None),
            mk("r", "run", Some(F::Run)),
            mk("t", "transcribe", Some(F::Transcribe)),
            mk("n", "notes", Some(F::Notes)),
            mk("/", "search", Some(F::Search)),
            mk(":", "palette", Some(F::Palette)),
            mk("?", "help", Some(F::Help)),
            mk("q", "quit", Some(F::Quit)),
        ],
        _ => vec![
            mk("↹", "pane", None),
            mk("↑↓", "move", None),
            mk("⏎", "open", None),
            mk("r", "run", Some(F::Run)),
            mk("t", "transcribe", Some(F::Transcribe)),
            mk("n", "notes", Some(F::Notes)),
            mk("y", "copy", Some(F::Copy)),
            mk("s", "select", Some(F::Select)),
            mk("/", "search", Some(F::Search)),
            mk(":", "palette", Some(F::Palette)),
            mk("?", "help", Some(F::Help)),
            mk("q", "quit", Some(F::Quit)),
        ],
    }
}

/// Greedily assign item indices to rows so each row's total width fits `width`.
fn assign_rows(widths: &[u16], width: u16) -> Vec<Vec<usize>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut col = 0u16;
    for (i, &w) in widths.iter().enumerate() {
        if col + w > width && col > 0 {
            rows.push(Vec::new());
            col = 0;
        }
        rows.last_mut().unwrap().push(i);
        col += w;
    }
    rows
}

fn draw_footer(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let hints = footer_hints(app.pane);
    let widths: Vec<u16> = hints.iter().map(tok_width).collect();
    let rows = assign_rows(&widths, area.width);

    let mut lines: Vec<Line> = Vec::new();

    for (ri, row) in rows.iter().enumerate() {
        let row_y = area.y + ri as u16;
        let mut spans: Vec<Span> = Vec::new();
        let mut x = area.x;
        for &i in row {
            let t = &hints[i];
            let w = widths[i];
            let clickable = t.cmd.is_some();
            let hovered = if let Some(cmd) = t.cmd {
                app.register_hit(Rect::new(x, row_y, w, 1), HitTarget::Footer(cmd));
                app.is_hovering(HitTarget::Footer(cmd))
            } else {
                false
            };
            let mut key_style = if clickable {
                th.accent_style()
            } else {
                th.muted_style()
            };
            let mut lbl_style = th.muted_style();
            if hovered {
                key_style = key_style.add_modifier(Modifier::UNDERLINED | Modifier::REVERSED);
                lbl_style = lbl_style.add_modifier(Modifier::UNDERLINED);
            }
            spans.push(Span::styled(format!(" {} ", t.key), key_style));
            spans.push(Span::styled(format!("{}  ", t.label), lbl_style));
            x += w;
        }
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines).style(th.base()), area);
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
    // Campaigns.
    let camp_inner = bordered_inner(parts[0]);
    prepare_list_state(
        &mut app.camp_state,
        Some(app.campaign_idx),
        app.campaigns.len(),
        camp_inner.height as usize,
    );
    app.register_hit(camp_inner, HitTarget::CampaignList);
    register_list_rows(
        app,
        camp_inner,
        app.camp_state.offset(),
        app.campaigns.len(),
        HitTarget::CampaignRow,
    );
    let mut camp_items: Vec<ListItem> = app
        .campaigns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            ListItem::new(c.name.clone())
                .style(hover_style(th, app.is_hovering(HitTarget::CampaignRow(i))))
        })
        .collect();
    if camp_items.is_empty() {
        camp_items
            .push(ListItem::new("no campaigns - press N to create one").style(th.muted_style()));
    }
    let camp_list = List::new(camp_items)
        .block(section_block("Campaigns", th, app.pane == Pane::Campaigns))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(camp_list, parts[0], &mut app.camp_state);

    // Sessions (row 0 is the synthetic Campaign Log).
    let sess_inner = bordered_inner(parts[1]);
    let session_selected = if app.log_selected {
        Some(0)
    } else {
        Some(app.session_idx + 1)
    };
    prepare_list_state(
        &mut app.sess_state,
        session_selected,
        app.sessions.len() + 1,
        sess_inner.height as usize,
    );
    app.register_hit(sess_inner, HitTarget::SessionList);
    register_list_rows(
        app,
        sess_inner,
        app.sess_state.offset(),
        app.sessions.len() + 1,
        HitTarget::SessionRow,
    );
    let mut sess_items: Vec<ListItem> = Vec::new();
    sess_items.push(
        ListItem::new(Line::from(vec![
            Span::styled("≡ ", th.accent_style()),
            Span::styled("Campaign Log", th.title_style(false)),
        ]))
        .style(hover_style(th, app.is_hovering(HitTarget::SessionRow(0)))),
    );
    for (i, s) in app.sessions.iter().enumerate() {
        let done = s.artifacts.iter().filter(|b| **b).count();
        let total = s.artifacts.len();
        let mark = if done == total {
            "✓"
        } else if done == 0 {
            "·"
        } else {
            "◐"
        };
        let age = crate::audio::human_age(s.modified);
        sess_items.push(
            ListItem::new(Line::from(vec![
                Span::styled(format!("{mark} "), th.success_style()),
                Span::raw(s.stem.clone()),
                Span::styled(format!("  {done}/{total} · {age}"), th.muted_style()),
            ]))
            .style(hover_style(
                th,
                app.is_hovering(HitTarget::SessionRow(i + 1)),
            )),
        );
    }
    if app.sessions.is_empty() {
        sess_items.push(
            ListItem::new("no sessions yet — select audio and press r").style(th.muted_style()),
        );
    }
    let sess_list = List::new(sess_items)
        .block(section_block("Sessions", th, app.pane == Pane::Sessions))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(sess_list, parts[1], &mut app.sess_state);

    // Audio.
    let audio_inner = bordered_inner(parts[2]);
    prepare_list_state(
        &mut app.audio_state,
        Some(app.audio_idx),
        app.audio.len(),
        audio_inner.height as usize,
    );
    app.register_hit(audio_inner, HitTarget::AudioList);
    register_list_rows(
        app,
        audio_inner,
        app.audio_state.offset(),
        app.audio.len(),
        HitTarget::AudioRow,
    );
    let mut audio_items: Vec<ListItem> = app
        .audio
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mark = if f.already_transcribed { "✓" } else { "○" };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{mark} "), th.muted_style()),
                Span::raw(f.stem()),
                Span::styled(
                    f.duration_secs
                        .map(|duration| {
                            format!("  {}", crate::audio::human_duration(Some(duration)))
                        })
                        .unwrap_or_default(),
                    th.muted_style(),
                ),
            ]))
            .style(hover_style(th, app.is_hovering(HitTarget::AudioRow(i))))
        })
        .collect();
    if audio_items.is_empty() {
        audio_items.push(ListItem::new("drop recordings into ./audio").style(th.muted_style()));
        audio_items.push(ListItem::new("(wav mp3 m4a flac ogg opus...)").style(th.muted_style()));
    }
    let audio_list = List::new(audio_items)
        .block(section_block("Audio", th, app.pane == Pane::Audio))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
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

    if app.models.is_some() {
        draw_models_pane(frame, app, th, main_area);
    } else if app.viewing_log {
        draw_log_viewer(frame, app, th, main_area);
    } else if app.open_session.is_some() {
        draw_viewer(frame, app, th, main_area);
    } else {
        draw_welcome(frame, app, th, main_area);
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
    let mut x = inner.x;
    for (i, a) in ALL_ARTIFACTS.iter().enumerate() {
        let exists = sess_arts
            .as_ref()
            .and_then(|v| v.get(i).copied())
            .unwrap_or(false);
        let mark = if exists { " ✓" } else { "" };
        let label = format!(" {}{} ", a.label(), mark);
        let w = label.chars().count() as u16;
        let selected = i == app.artifact_tab;
        let tab_rect = Rect::new(x, inner.y, w, 1);
        app.register_hit(tab_rect, HitTarget::ArtifactTab(i));
        let hovered = app.is_hovering(HitTarget::ArtifactTab(i));
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
        spans.push(Span::styled(label, style));
        x += w;
        if i + 1 < ALL_ARTIFACTS.len() {
            spans.push(Span::styled("│", th.muted_style()));
            x += 1;
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)).style(th.base()), inner);

    // Preview with markdown rendering + scrollbar.
    let stem = app
        .open_session
        .and_then(|si| app.sessions.get(si))
        .map(|s| s.stem.clone())
        .unwrap_or_default();
    let mut title = format!("{} · {}", stem, ALL_ARTIFACTS[app.artifact_tab].filename());
    if app.viewing_quotes() {
        let quotes = app.quote_positions();
        if quotes.is_empty() {
            title.push_str("  ·  (no timestamps — re-run Quotes)");
        } else {
            title.push_str(&format!(
                "  ·  ↑↓ pick quote ({}/{}) · p ▶ play",
                (app.quote_idx + 1).min(quotes.len()),
                quotes.len()
            ));
        }
    }
    if app.has_candidate() {
        if app.viewing_candidate {
            title.push_str("  ·  ▶ viewing NEW version — a: keep this (new) · c: compare");
        } else {
            title.push_str(
                "  ·  ◀ viewing CURRENT version (a new one is ready) — c: compare · a: keep this",
            );
        }
    }
    let highlight = if app.viewing_quotes() {
        app.selected_quote_range()
    } else {
        None
    };
    draw_markdown_pane(frame, app, th, parts[1], &title, highlight);
}

fn draw_log_viewer(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    draw_markdown_pane(
        frame,
        app,
        th,
        area,
        "Campaign Log · _campaign-log.md",
        None,
    );
}

/// Render `app.viewer_lines` as markdown into a bordered, scrollable pane.
/// `highlight` is an inclusive raw-line range to emphasise (the selected quote).
fn draw_markdown_pane(
    frame: &mut Frame,
    app: &mut App,
    th: &Theme,
    area: Rect,
    title: &str,
    highlight: Option<(usize, usize)>,
) {
    let inner = bordered_inner(area);
    app.register_hit(inner, HitTarget::Viewer);
    if inner.height > 0 {
        app.register_hit(
            Rect::new(
                area.x.saturating_add(area.width.saturating_sub(1)),
                inner.y,
                1,
                inner.height,
            ),
            HitTarget::ViewerScrollbar,
        );
    }
    let mut lines = markdown::render_lines(&app.viewer_lines, th);
    if let Some((lo, hi)) = highlight {
        let sel = Style::default().bg(th.selection_bg);
        for line in lines.iter_mut().take(hi + 1).skip(lo) {
            line.style = sel;
        }
    }
    let para = Paragraph::new(Text::from(lines))
        .block(section_block(title, th, app.pane == Pane::Content))
        .style(th.base())
        .wrap(Wrap { trim: false })
        .scroll((app.viewer_scroll, 0));
    frame.render_widget(para, area);

    let mut sb_state =
        ScrollbarState::new(app.viewer_lines.len().max(1)).position(app.viewer_scroll as usize);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight).style(th.muted_style()),
        area,
        &mut sb_state,
    );
}

fn draw_welcome(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    app.register_hit(bordered_inner(area), HitTarget::Viewer);
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
    const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let elapsed = if app.job_running {
        app.job_started.map(|s| human_time(s.elapsed().as_secs()))
    } else {
        app.job_elapsed.map(|d| human_time(d.as_secs()))
    }
    .unwrap_or_default();
    let title = if app.job_running {
        let g = SPIN[(app.tick as usize / 2) % SPIN.len()];
        if elapsed.is_empty() {
            format!("Working · {} {g}", app.job_title)
        } else {
            format!("Working · {} {g}  ⏱ {elapsed}", app.job_title)
        }
    } else if elapsed.is_empty() {
        format!("Job · {}", app.job_title)
    } else {
        format!("Job · {}  ✓ {elapsed}", app.job_title)
    };
    let focused = matches!(app.pane, Pane::Content);
    let block = section_block(&title, th, focused);
    let inner = block.inner(area);
    app.register_hit(inner, HitTarget::Job);
    frame.render_widget(block, area);

    // Compose the inner rows: [stage timeline] [progress bar] [log].
    let has_stages = !app.job_stages.is_empty() && inner.height >= 3;
    let has_bar = app.job_progress.is_some() && inner.height >= 2;
    let mut constraints: Vec<Constraint> = Vec::new();
    if has_stages {
        constraints.push(Constraint::Length(1));
    }
    if has_bar {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(1));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);
    let mut ri = 0;

    // --- Stage timeline -------------------------------------------------
    if has_stages {
        let strip = rows[ri];
        ri += 1;
        frame.render_widget(stage_timeline(app, th, strip.width as usize), strip);
    }

    // --- Progress bar ---------------------------------------------------
    if has_bar {
        let bar_area = rows[ri];
        ri += 1;
        if let Some((label, pos, total, rate)) = app.job_progress.clone() {
            if total > 0 {
                let ratio = (pos as f64 / total as f64).clamp(0.0, 1.0);
                let pct = (ratio * 100.0).round() as u16;
                let counts = fmt_counts(&label, pos, total);
                let gauge = ratatui::widgets::Gauge::default()
                    .gauge_style(th.accent_style())
                    .ratio(ratio)
                    .label(format!("{label}  {counts}  {pct}%"));
                frame.render_widget(gauge, bar_area);
            } else {
                // Indeterminate: a marquee pulse gliding over a dim track.
                let progress_label = if pos > 0 {
                    let rate = rate
                        .map(|value| format!(" · {value:.1} tok/s"))
                        .unwrap_or_default();
                    format!("{label} · {pos} tok{rate}")
                } else {
                    label
                };
                frame.render_widget(
                    pulse_line(&progress_label, bar_area.width as usize, app.tick, th),
                    bar_area,
                );
            }
        }
    }

    let log_area = rows[ri];

    // Wrap every log entry to the inner width so long strings roll over.
    let width = (log_area.width as usize).max(1);
    let mut rows: Vec<Line> = Vec::new();
    if !app.job_queue.is_empty() {
        let queued = app
            .job_queue
            .iter()
            .map(|(_, title)| title.as_str())
            .collect::<Vec<_>>()
            .join(" → ");
        rows.push(Line::from(Span::styled(
            middle_truncate(&format!("queued: {queued}"), width),
            th.muted_style(),
        )));
    }
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

    let visible = log_area.height as usize;
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
    frame.render_widget(Paragraph::new(slice).style(th.base()), log_area);

    if rows.len() > visible {
        let mut sb = ScrollbarState::new(rows.len()).position(off);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).style(th.muted_style()),
            area,
            &mut sb,
        );
    }
}

fn middle_truncate(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let left = (width - 1).div_ceil(2);
    let right = width - 1 - left;
    format!(
        "{}…{}",
        chars[..left].iter().collect::<String>(),
        chars[chars.len() - right..].iter().collect::<String>()
    )
}

/// Render the animated pipeline stage timeline (e.g.
/// `✓ Transcribe → ✓ Outline → ◐ Notes`). Older stages are dropped with a
/// leading ellipsis when the strip is too narrow.
fn stage_timeline(app: &App, th: &Theme, width: usize) -> Paragraph<'static> {
    const DOTS: [&str; 4] = ["◐", "◓", "◑", "◒"];
    struct Seg {
        sym: String,
        sym_style: Style,
        name: String,
        name_style: Style,
    }
    let n = app.job_stages.len();
    let mut segs: Vec<Seg> = Vec::with_capacity(n);
    for (i, (name, started)) in app.job_stages.iter().enumerate() {
        let active = app.job_running && i + 1 == n;
        let (sym, sym_style) = if active {
            (
                DOTS[(app.tick as usize / 2) % DOTS.len()].to_string(),
                th.accent_style().add_modifier(Modifier::BOLD),
            )
        } else {
            ("✓".to_string(), th.success_style())
        };
        let name_style = if active {
            th.title_style(false)
        } else {
            th.muted_style()
        };
        let end = app
            .job_stages
            .get(i + 1)
            .map(|(_, next_started)| *next_started)
            .or_else(|| {
                if app.job_running {
                    Some(std::time::Instant::now())
                } else {
                    app.job_finished_at
                }
            })
            .unwrap_or(*started);
        let duration = stage_duration(*started, end);
        segs.push(Seg {
            sym,
            sym_style,
            name: format!("{name} {duration}"),
            name_style,
        });
    }

    let seg_w = |s: &Seg| 2 + s.name.chars().count(); // "SYM NAME"
    const SEP_W: usize = 3; // " → "
    let mut start = 0;
    loop {
        let mut total = 0usize;
        for (idx, s) in segs.iter().enumerate().skip(start) {
            if idx > start {
                total += SEP_W;
            }
            total += seg_w(s);
        }
        let prefix = if start > 0 { 2 } else { 0 };
        if total + prefix <= width || start + 1 >= segs.len().max(1) {
            break;
        }
        start += 1;
    }

    let mut spans: Vec<Span> = Vec::new();
    if start > 0 {
        spans.push(Span::styled("… ".to_string(), th.muted_style()));
    }
    for (idx, s) in segs.into_iter().enumerate().skip(start) {
        if idx > start {
            spans.push(Span::styled(" → ".to_string(), th.muted_style()));
        }
        spans.push(Span::styled(format!("{} ", s.sym), s.sym_style));
        spans.push(Span::styled(s.name, s.name_style));
    }
    Paragraph::new(Line::from(spans)).style(th.base())
}

fn stage_duration(start: std::time::Instant, end: std::time::Instant) -> String {
    human_time(end.saturating_duration_since(start).as_secs())
}

/// An indeterminate progress row: a bright block gliding over a dim track,
/// prefixed by the current label.
fn pulse_line(label: &str, width: usize, tick: u64, th: &Theme) -> Paragraph<'static> {
    let prefix = format!("{label} ");
    let pw = prefix.chars().count();
    let bar_w = width.saturating_sub(pw).max(4);
    let win = (bar_w / 4).max(3);
    let period = (bar_w + win).max(1);
    let phase = (tick as usize) % period;
    let mut bar = String::with_capacity(bar_w);
    for i in 0..bar_w {
        let pos = phase as isize - win as isize;
        let bright = (i as isize) >= pos && (i as isize) < pos + win as isize;
        bar.push(if bright { '█' } else { '░' });
    }
    Paragraph::new(Line::from(vec![
        Span::styled(prefix, th.muted_style()),
        Span::styled(bar, th.accent_style()),
    ]))
    .style(th.base())
}

/// Format a progress counter, guessing the unit from the label / magnitude:
/// byte sizes for downloads, `m:ss` for transcription, plain counts otherwise.
fn fmt_counts(label: &str, pos: u64, total: u64) -> String {
    if total == 0 {
        return String::new();
    }
    let l = label.to_lowercase();
    if total >= 1_000_000 || l.contains("download") || l.contains("pull") {
        format!(
            "{}/{}",
            crate::models::human_bytes(pos),
            crate::models::human_bytes(total)
        )
    } else if l.contains("transcrib") {
        format!("{}/{}", human_time(pos), human_time(total))
    } else {
        format!("{pos}/{total}")
    }
}

/// Format seconds as `m:ss` (or `h:mm:ss` past an hour).
fn human_time(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
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
    let (query, filtered, cursor) = match &app.overlay {
        Overlay::Palette(palette) => (
            palette.query.clone(),
            palette.filtered.clone(),
            palette.cursor,
        ),
        _ => return,
    };
    let rect = centered(area, 60, 60);
    frame.render_widget(Clear, rect);
    app.register_hit(rect, HitTarget::OverlayBarrier);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(rect);

    let input = Paragraph::new(Line::from(vec![
        Span::styled("› ", th.accent_style()),
        Span::raw(query),
        Span::styled("▏", th.accent_style()),
    ]))
    .block(popup_block("Command palette", th))
    .style(th.base());
    frame.render_widget(input, parts[0]);

    let labels: Vec<&str> = super::app::Action::all()
        .iter()
        .map(|a| a.label())
        .collect();
    let list_inner = bordered_inner(parts[1]);
    prepare_list_state(
        &mut app.overlay_state,
        Some(cursor),
        filtered.len(),
        list_inner.height as usize,
    );
    let offset = app.overlay_state.offset();
    register_list_rows(
        app,
        list_inner,
        offset,
        filtered.len(),
        HitTarget::PaletteItem,
    );
    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(row, &i)| {
            ListItem::new(labels[i].to_string()).style(hover_style(
                th,
                app.is_hovering(HitTarget::PaletteItem(row)),
            ))
        })
        .collect();
    let list = List::new(items)
        .block(popup_block("Actions  (click to run)", th))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, parts[1], &mut app.overlay_state);
}

fn draw_search(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (query, all_campaigns, hits_len, cursor) = match &app.overlay {
        Overlay::Search(search) => (
            search.query.clone(),
            search.all_campaigns,
            search.hits.len(),
            search.cursor,
        ),
        _ => return,
    };
    let rect = centered(area, 72, 70);
    frame.render_widget(Clear, rect);
    app.register_hit(rect, HitTarget::OverlayBarrier);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(rect);

    let input = Paragraph::new(Line::from(vec![
        Span::styled("🔎 ", th.accent_style()),
        Span::raw(query.clone()),
        Span::styled("▏", th.accent_style()),
    ]))
    .block(popup_block(
        if all_campaigns {
            "Search notes · all campaigns (Ctrl-A)"
        } else {
            "Search notes · current campaign (Ctrl-A)"
        },
        th,
    ))
    .style(th.base());
    frame.render_widget(input, parts[0]);

    let list_inner = bordered_inner(parts[1]);
    prepare_list_state(
        &mut app.overlay_state,
        Some(cursor),
        hits_len,
        list_inner.height as usize,
    );
    let offset = app.overlay_state.offset();
    register_list_rows(app, list_inner, offset, hits_len, HitTarget::SearchItem);
    let Overlay::Search(search) = &app.overlay else {
        return;
    };
    let items: Vec<ListItem> = search
        .hits
        .iter()
        .enumerate()
        .map(|(row, h)| {
            let mut spans = Vec::new();
            if search.all_campaigns {
                if let Some(name) = search
                    .hit_campaigns
                    .get(row)
                    .and_then(|index| app.campaigns.get(*index))
                    .map(|campaign| campaign.name.as_str())
                {
                    spans.push(Span::styled(format!("{name} · "), th.muted_style()));
                }
            }
            spans.push(Span::styled(format!("{} ", h.session), th.accent_style()));
            spans.push(Span::styled(
                format!("[{}] ", h.kind),
                artifact_kind_style(&h.kind, th),
            ));
            spans.extend(search_snippet_spans(
                &h.snippet.replace('\n', " "),
                &query,
                th,
            ));
            ListItem::new(Line::from(spans))
                .style(hover_style(th, app.is_hovering(HitTarget::SearchItem(row))))
        })
        .collect();
    let empty = items.is_empty();
    let list = List::new(items)
        .block(popup_block(
            if empty {
                "No matches"
            } else {
                "Results  (click to open)"
            },
            th,
        ))
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, parts[1], &mut app.overlay_state);
}

fn artifact_kind_style(kind: &str, th: &Theme) -> Style {
    if kind.contains("summary") {
        th.accent_style()
    } else if kind.contains("quote") {
        th.warn_style()
    } else if kind.contains("dm") {
        th.success_style()
    } else {
        th.muted_style()
    }
}

fn search_snippet_spans(snippet: &str, query: &str, th: &Theme) -> Vec<Span<'static>> {
    let marked = crate::index::highlight_markers(snippet, query);
    let mut spans = Vec::new();
    let mut rest = marked.as_str();
    while let Some(start) = rest.find('«') {
        if start > 0 {
            spans.push(Span::raw(rest[..start].to_string()));
        }
        let after_start = &rest[start + '«'.len_utf8()..];
        let Some(end) = after_start.find('»') else {
            spans.push(Span::raw(rest[start..].to_string()));
            return spans;
        };
        spans.push(Span::styled(
            after_start[..end].to_string(),
            th.accent_style().add_modifier(Modifier::BOLD),
        ));
        rest = &after_start[end + '»'.len_utf8()..];
    }
    if !rest.is_empty() {
        spans.push(Span::raw(rest.to_string()));
    }
    spans
}

fn draw_picker(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (title, items_len, cursor) = match &app.overlay {
        Overlay::Picker(picker) => (picker.title.clone(), picker.items.len(), picker.cursor),
        _ => return,
    };
    let rect = centered(area, 64, 70);
    frame.render_widget(Clear, rect);
    app.register_hit(rect, HitTarget::OverlayBarrier);
    let block = popup_block(&title, th);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    app.register_hit(parts[1], HitTarget::PickerConfirm);
    prepare_list_state(
        &mut app.overlay_state,
        Some(cursor),
        items_len,
        parts[0].height as usize,
    );
    let offset = app.overlay_state.offset();
    register_list_rows(app, parts[0], offset, items_len, HitTarget::PickerItem);
    let Overlay::Picker(picker) = &app.overlay else {
        return;
    };
    let items: Vec<ListItem> = picker
        .items
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let checked = picker.checked.get(i).copied().unwrap_or(false);
            let box_ = if checked { "[x] " } else { "[ ] " };
            ListItem::new(Line::from(vec![
                Span::styled(
                    box_,
                    if checked {
                        th.success_style()
                    } else {
                        th.muted_style()
                    },
                ),
                Span::raw(label.clone()),
            ]))
            .style(hover_style(th, app.is_hovering(HitTarget::PickerItem(i))))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, parts[0], &mut app.overlay_state);
    let confirm_style = if app.is_hovering(HitTarget::PickerConfirm) {
        th.accent_style()
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        th.accent_style()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Enter ", confirm_style),
            Span::styled(" run", th.muted_style()),
        ]))
        .style(th.base()),
        parts[1],
    );
}

fn draw_campaign_form(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let rect = centered(area, 88, 90);
    frame.render_widget(Clear, rect);
    app.register_hit(rect, HitTarget::OverlayBarrier);
    let (title, rows, cursor, scroll, error) = {
        let Overlay::CampaignForm(form) = &mut app.overlay else {
            return;
        };
        let visible = rect.height.saturating_sub(5) as usize;
        form.ensure_cursor_visible(visible);
        (
            form.title(),
            form.rows(),
            form.cursor(),
            form.scroll_offset(),
            form.error.clone(),
        )
    };
    let block = popup_block(&title, th);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);
    app.overlay_state.select(Some(cursor));
    *app.overlay_state.offset_mut() = scroll.min(rows.len().saturating_sub(1));
    register_list_rows(
        app,
        parts[0],
        app.overlay_state.offset(),
        rows.len(),
        HitTarget::CampaignFormRow,
    );
    let label_width = rows
        .iter()
        .filter(|row| row.selectable)
        .map(|row| row.label.chars().count())
        .max()
        .unwrap_or(10)
        .clamp(10, 24);
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            if !row.selectable {
                return ListItem::new(Line::from(Span::styled(
                    format!(" {} ", row.label),
                    th.accent_style().add_modifier(Modifier::BOLD),
                )));
            }
            let selected = index == cursor;
            let hovered = app.is_hovering(HitTarget::CampaignFormRow(index));
            let label: String = row.label.chars().take(label_width).collect();
            let value: String = row.value.chars().take(72).collect();
            let label_style = if selected {
                th.selection()
            } else if hovered {
                th.base().add_modifier(Modifier::UNDERLINED)
            } else {
                th.base()
            };
            let value_style = if selected {
                th.selection()
            } else if hovered {
                th.muted_style().add_modifier(Modifier::UNDERLINED)
            } else {
                th.muted_style()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {label:<label_width$} "), label_style),
                Span::styled(value, value_style),
            ]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(th.selection())
        .highlight_symbol("▸");
    frame.render_stateful_widget(list, parts[0], &mut app.overlay_state);

    let hint = if let Some(error) = error {
        Line::from(Span::styled(error, th.error_style()))
    } else {
        Line::from(vec![
            Span::styled("Enter", th.accent_style()),
            Span::styled(" edit/toggle  ", th.muted_style()),
            Span::styled("Space", th.accent_style()),
            Span::styled(" cycle  ", th.muted_style()),
            Span::styled("a/x", th.accent_style()),
            Span::styled(" add/remove  ", th.muted_style()),
            Span::styled("Ctrl+S", th.accent_style()),
            Span::styled(" save  ", th.muted_style()),
            Span::styled("Esc", th.accent_style()),
            Span::styled(" back", th.muted_style()),
        ])
    };
    frame.render_widget(
        Paragraph::new(hint)
            .style(th.base())
            .wrap(Wrap { trim: true }),
        parts[1],
    );
}

fn draw_text_prompt(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (title, input, error) = match &app.overlay {
        Overlay::TextPrompt(prompt) => (
            prompt.title.clone(),
            prompt.input.with_cursor(),
            prompt.error.clone(),
        ),
        _ => return,
    };
    let rect = centered(area, 62, 28);
    frame.render_widget(Clear, rect);
    app.register_hit(rect, HitTarget::OverlayBarrier);
    let block = popup_block(&title, th);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    app.register_hit(parts[0], HitTarget::TextPromptInput);
    let submit_width = 12.min(parts[1].width);
    app.register_hit(
        Rect::new(parts[1].x, parts[1].y, submit_width, parts[1].height),
        HitTarget::TextPromptSubmit,
    );
    if parts[1].width > submit_width {
        app.register_hit(
            Rect::new(
                parts[1].x + submit_width,
                parts[1].y,
                parts[1].width - submit_width,
                parts[1].height,
            ),
            HitTarget::OverlayDismiss,
        );
    }
    let mut lines = vec![Line::from(Span::styled(input, th.base()))];
    if let Some(error) = error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(error, th.error_style())));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(th.base())
            .wrap(Wrap { trim: true }),
        parts[0],
    );
    let submit_style = if app.is_hovering(HitTarget::TextPromptSubmit) {
        th.accent_style()
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        th.accent_style()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Enter ", submit_style),
            Span::styled(" fork  Esc cancel", th.muted_style()),
        ]))
        .style(th.base()),
        parts[1],
    );
}

fn draw_speaker_map(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (labels_len, cursor) = match &app.overlay {
        Overlay::SpeakerMap(state) => (state.labels.len(), state.cursor),
        _ => return,
    };
    let rect = centered(area, 70, 62);
    frame.render_widget(Clear, rect);
    app.register_hit(rect, HitTarget::OverlayBarrier);
    let block = popup_block(
        "Map speakers  Enter cycle · p preview · w write · Esc cancel",
        th,
    );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    prepare_list_state(
        &mut app.overlay_state,
        Some(cursor),
        labels_len,
        (parts[0].height / 2) as usize,
    );
    let offset = app.overlay_state.offset();
    register_multiline_list_rows(app, parts[0], offset, labels_len, 2, HitTarget::SpeakerRow);
    let preview_width = 12.min(parts[1].width);
    app.register_hit(
        Rect::new(parts[1].x, parts[1].y, preview_width, parts[1].height),
        HitTarget::SpeakerPreview,
    );
    if parts[1].width > preview_width {
        app.register_hit(
            Rect::new(
                parts[1].x + preview_width,
                parts[1].y,
                parts[1].width - preview_width,
                parts[1].height,
            ),
            HitTarget::SpeakerSave,
        );
    }
    let Overlay::SpeakerMap(state) = &app.overlay else {
        return;
    };
    let items: Vec<ListItem> = state
        .labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let mapped = state.map.get(label).map(String::as_str).unwrap_or("Skip");
            let sample = state
                .samples
                .get(label)
                .and_then(|samples| samples.first())
                .cloned()
                .unwrap_or_default();
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{label}  "), th.accent_style()),
                    Span::styled(mapped.to_string(), th.success_style()),
                ]),
                Line::from(Span::styled(sample, th.muted_style())),
            ])
            .style(hover_style(
                th,
                app.is_hovering(HitTarget::SpeakerRow(index)),
            ))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, parts[0], &mut app.overlay_state);
    let preview_style = if app.is_hovering(HitTarget::SpeakerPreview) {
        th.accent_style()
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        th.accent_style()
    };
    let save_style = if app.is_hovering(HitTarget::SpeakerSave) {
        th.success_style()
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        th.success_style()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Preview ", preview_style),
            Span::styled("  Save mapping", save_style),
        ]))
        .style(th.base()),
        parts[1],
    );
}

fn draw_message(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (title, body, error) = match &app.overlay {
        Overlay::Message { title, body, error } => (title.clone(), body.clone(), *error),
        _ => return,
    };
    let rect = centered(area, 50, 30);
    frame.render_widget(Clear, rect);
    app.register_hit(rect, HitTarget::OverlayBarrier);
    app.register_hit(rect, HitTarget::OverlayDismiss);
    let style = if error { th.error_style() } else { th.base() };
    let para = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(body, style)),
        Line::from(""),
        Line::from(Span::styled("Press any key to dismiss.", th.muted_style())),
    ]))
    .block(popup_block(&title, th))
    .style(th.base())
    .wrap(Wrap { trim: true });
    frame.render_widget(para, rect);
}

fn draw_confirm(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let (title, body) = match &app.overlay {
        Overlay::Confirm { title, body } => (title.clone(), body.clone()),
        _ => return,
    };
    let rect = centered(area, 54, 34);
    frame.render_widget(Clear, rect);
    app.register_hit(rect, HitTarget::OverlayBarrier);
    let block = popup_block(&title, th);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let lines: Vec<Line> = body
        .split('\n')
        .map(|l| Line::from(Span::styled(l.to_string(), th.base())))
        .collect();
    let (yes, no) = match app.pending_confirm.as_ref() {
        Some(super::app::ConfirmAction::Quit) => (" quit", " keep job"),
        Some(super::app::ConfirmAction::DiscardCampaignForm) => (" discard", " keep editing"),
        Some(super::app::ConfirmAction::SaveCampaignRename) => (" move and save", " keep editing"),
        _ => (" re-transcribe", " keep transcript"),
    };
    let yes_label = format!(" Y{yes} ");
    let no_label = format!(" N{no} ");
    let cancel_label = " Esc cancel ";
    let yes_width = yes_label.chars().count() as u16;
    let no_width = no_label.chars().count() as u16;
    app.register_hit(
        Rect::new(parts[1].x, parts[1].y, yes_width.min(parts[1].width), 1),
        HitTarget::ConfirmYes,
    );
    if parts[1].width > yes_width {
        app.register_hit(
            Rect::new(
                parts[1].x + yes_width,
                parts[1].y,
                no_width.min(parts[1].width - yes_width),
                1,
            ),
            HitTarget::ConfirmNo,
        );
    }
    if parts[1].width > yes_width.saturating_add(no_width) {
        app.register_hit(
            Rect::new(
                parts[1].x + yes_width + no_width,
                parts[1].y,
                (cancel_label.chars().count() as u16).min(parts[1].width - yes_width - no_width),
                1,
            ),
            HitTarget::ConfirmCancel,
        );
    }
    let para = Paragraph::new(Text::from(lines))
        .style(th.base())
        .wrap(Wrap { trim: true });
    frame.render_widget(para, parts[0]);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(yes_label, th.accent_style().add_modifier(Modifier::BOLD)),
            Span::styled(no_label, th.muted_style().add_modifier(Modifier::BOLD)),
            Span::styled(cancel_label, th.muted_style()),
        ]))
        .style(th.base()),
        parts[1],
    );
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
    app.register_hit(rect, HitTarget::OverlayBarrier);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(rect);

    let list_block = popup_block("Themes  (click to preview · ⏎ apply)", &th);
    let list_inner = list_block.inner(cols[0]);
    frame.render_widget(list_block, cols[0]);
    let list_parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(list_inner);
    prepare_list_state(
        &mut app.overlay_state,
        Some(cursor),
        app.themes.len(),
        list_parts[0].height as usize,
    );
    let offset = app.overlay_state.offset();
    register_list_rows(
        app,
        list_parts[0],
        offset,
        app.themes.len(),
        HitTarget::ThemeRow,
    );
    app.register_hit(list_parts[1], HitTarget::ThemeApply);
    let items: Vec<ListItem> = app
        .themes
        .iter()
        .enumerate()
        .map(|(i, theme)| {
            ListItem::new(theme.name.clone())
                .style(hover_style(&th, app.is_hovering(HitTarget::ThemeRow(i))))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(th.selection())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, list_parts[0], &mut app.overlay_state);
    let apply_style = if app.is_hovering(HitTarget::ThemeApply) {
        th.success_style()
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        th.success_style()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Enter ", apply_style),
            Span::styled(" apply · Esc cancel", th.muted_style()),
        ]))
        .style(th.base()),
        list_parts[1],
    );

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

fn draw_models_pane(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let queued = app.job_queue.len();
    let title = if queued > 0 {
        format!(
            "Models (catalog {}) — {queued} queued · ⏎ default · i check · d delete installed · g CUDA · u Ollama · Esc",
            crate::models::OLLAMA_CATALOG_UPDATED
        )
    } else {
        format!(
            "Models (catalog {}) — ⏎ default · i check · d delete installed · g CUDA · u Ollama · Esc",
            crate::models::OLLAMA_CATALOG_UPDATED
        )
    };
    let focused = matches!(app.pane, Pane::Content);
    let block = section_block(&title, th, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Reserve the top line for a fixed legend/keychart; the list scrolls below.
    let (legend_area, list_area) = if inner.height > 2 {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);
        (Some(parts[0]), parts[1])
    } else {
        (None, inner)
    };
    if let Some(la) = legend_area {
        let legend = Line::from(vec![
            Span::styled("▸ ", th.accent_style()),
            Span::styled("selected  ", th.muted_style()),
            Span::styled("● ", th.accent_style()),
            Span::styled("default  ", th.muted_style()),
            Span::styled("✓ ", th.success_style()),
            Span::styled("installed  ", th.muted_style()),
            Span::styled("· ", th.muted_style()),
            Span::styled("available    date age: ", th.muted_style()),
            Span::styled("2y+", th.warn_style()),
            Span::styled(" · ", th.muted_style()),
            Span::styled("1–2y", th.muted_style()),
            Span::styled(" · ", th.muted_style()),
            Span::styled("<1y", th.success_style()),
            Span::styled(" · — unknown", th.muted_style()),
        ]);
        frame.render_widget(Paragraph::new(legend).style(th.base()), la);
    }
    app.register_hit(list_area, HitTarget::ModelList);

    let visible = list_area.height as usize;
    let (rows_len, _) = {
        let s = app.models.as_ref().unwrap();
        (s.rows.len(), s.cursor)
    };
    // Keep the cursor visible.
    if let Some(s) = &mut app.models {
        if visible > 0 {
            if s.cursor < s.scroll {
                s.scroll = s.cursor;
            } else if s.cursor >= s.scroll + visible {
                s.scroll = s.cursor + 1 - visible;
            }
        }
        if s.scroll >= rows_len {
            s.scroll = 0;
        }
    }
    let (scroll, cursor, model_rows) = {
        let state = app.models.as_ref().unwrap();
        (state.scroll, state.cursor, state.rows.clone())
    };

    let mut lines: Vec<Line> = Vec::new();
    // Size the id column to the longest model name so every column lines up,
    // even for long org/model ids. Capped so it never crowds out the buttons.
    let id_w = model_rows
        .iter()
        .filter(|r| !r.header)
        .map(|r| r.display.chars().count())
        .max()
        .unwrap_or(18)
        .clamp(12, 30);
    for vi in 0..visible {
        let ri = scroll + vi;
        if ri >= model_rows.len() {
            break;
        }
        let r = &model_rows[ri];
        let y = list_area.y + vi as u16;
        if r.header {
            if r.col_header {
                // Column labels aligned to the data columns below.
                let head = format!(
                    "    {:<id_w$} {:>8} {:>9}  actions",
                    "model", "released", "size"
                );
                lines.push(Line::from(Span::styled(head, th.muted_style())));
            } else {
                lines.push(Line::from(Span::styled(r.id.clone(), th.title_style(true))));
            }
            continue;
        }
        app.register_hit(
            Rect::new(list_area.x, y, list_area.width, 1),
            HitTarget::ModelRow(ri),
        );
        let selected = ri == cursor;
        let mark = if r.is_default {
            "● "
        } else if r.installed {
            "✓ "
        } else if r.family {
            "  "
        } else {
            "· "
        };
        let mark_style = if r.is_default {
            th.accent_style()
        } else if r.installed {
            th.success_style()
        } else {
            th.muted_style()
        };

        // Expandable family row: caret + name + variant count (no buttons).
        if r.family {
            let caret = if r.expanded { "▾ " } else { "▸ " };
            let name = format!("{:<id_w$} ", r.display);
            let mut spans: Vec<Span> = vec![
                Span::styled(caret.to_string(), th.accent_style()),
                Span::styled(mark.to_string(), mark_style),
                Span::styled(
                    name,
                    if selected {
                        th.selection()
                    } else {
                        th.title_style(false)
                    },
                ),
                released_span(th, &r.released),
                Span::styled(
                    format!("  {} variants ▸", r.variant_count),
                    th.muted_style(),
                ),
            ];
            if r.expanded {
                if let Some(last) = spans.last_mut() {
                    *last = Span::styled(
                        format!("  {} variants ▾", r.variant_count),
                        th.muted_style(),
                    );
                }
            }
            lines.push(Line::from(spans));
            continue;
        }

        let size = if r.size > 0 {
            crate::models::human_bytes(r.size)
        } else {
            "—".to_string()
        };
        // Children are indented; the id column shrinks so later columns align.
        let indent = if r.indent > 0 { 2u16 } else { 0 };
        let eff_w = (id_w as u16).saturating_sub(indent) as usize;

        let mut spans: Vec<Span> = Vec::new();
        let mut x = list_area.x;
        if indent > 0 {
            spans.push(Span::styled("  ", th.muted_style()));
            x += indent;
        }
        spans.push(Span::styled(
            if selected { "▸ " } else { "  " },
            th.accent_style(),
        ));
        x += 2;
        spans.push(Span::styled(mark.to_string(), mark_style));
        x += 2;
        let id_disp: String = if r.display.chars().count() > eff_w {
            r.display.chars().take(eff_w).collect()
        } else {
            r.display.clone()
        };
        let id_field = format!("{id_disp:<eff_w$} ");
        let idw = id_field.chars().count() as u16;
        spans.push(Span::styled(
            id_field,
            if selected { th.selection() } else { th.base() },
        ));
        x += idw;
        // Release date column (colour-coded by age).
        let rel = released_span(th, &r.released);
        let relw = rel.content.chars().count() as u16;
        spans.push(rel);
        x += relw;
        let size_field = format!("{size:>9}  ");
        let sw = size_field.chars().count() as u16;
        spans.push(Span::styled(size_field, th.muted_style()));
        x += sw;

        // ASR bridge engines are prepared locally (uv envs or GGUF weights).
        // ⏎ still sets the model as the default.
        if r.kind == ModelKind::Asr {
            let label = if r.installed {
                "[ check ]"
            } else {
                "[ prepare ]"
            };
            let lw = label.chars().count() as u16;
            let install_target = HitTarget::ModelButton {
                row: ri,
                install: true,
            };
            app.register_hit(Rect::new(x, y, lw, 1), install_target);
            let mut st = th.success_style();
            if app.is_hovering(install_target) {
                st = st.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            spans.push(Span::styled(label.to_string(), st));
            x += lw;
            if r.installed {
                spans.push(Span::raw(" "));
                x += 1;
                let del_label = "[ delete ]";
                let dw = del_label.chars().count() as u16;
                let delete_target = HitTarget::ModelButton {
                    row: ri,
                    install: false,
                };
                app.register_hit(Rect::new(x, y, dw, 1), delete_target);
                let mut del_style = th.error_style();
                if app.is_hovering(delete_target) {
                    del_style = del_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                }
                spans.push(Span::styled(del_label.to_string(), del_style));
            }
            let tag = crate::asr::find(&r.id)
                .map(|m| format!("  via {}", m.engine.label()))
                .unwrap_or_else(|| "  via uv".to_string());
            spans.push(Span::styled(tag, th.muted_style()));
            lines.push(Line::from(spans));
            continue;
        }

        // [install/check] button. Installed rows perform a cheap remote
        // metadata check before downloading anything.
        let inst_label = if r.installed {
            "[ check ]"
        } else {
            "[ install ]"
        };
        let iw = inst_label.chars().count() as u16;
        let install_target = HitTarget::ModelButton {
            row: ri,
            install: true,
        };
        app.register_hit(Rect::new(x, y, iw, 1), install_target);
        let mut inst_style = th.success_style();
        if app.is_hovering(install_target) {
            inst_style = inst_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }
        spans.push(Span::styled(inst_label.to_string(), inst_style));
        x += iw;
        if r.installed {
            spans.push(Span::raw(" "));
            x += 1;
            let del_label = "[ delete ]";
            let dw = del_label.chars().count() as u16;
            let delete_target = HitTarget::ModelButton {
                row: ri,
                install: false,
            };
            app.register_hit(Rect::new(x, y, dw, 1), delete_target);
            let mut del_style = th.error_style();
            if app.is_hovering(delete_target) {
                del_style = del_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            spans.push(Span::styled(del_label.to_string(), del_style));
        }

        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines).style(th.base()), list_area);

    if rows_len > visible {
        let mut sb = ScrollbarState::new(rows_len).position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).style(th.muted_style()),
            area,
            &mut sb,
        );
    }
}

fn draw_help(frame: &mut Frame, app: &mut App, th: &Theme, area: Rect) {
    let rect = centered(area, 64, 80);
    frame.render_widget(Clear, rect);
    app.register_hit(rect, HitTarget::OverlayBarrier);
    app.register_hit(rect, HitTarget::OverlayDismiss);
    let rows = [
        ("Tab / Shift-Tab", "cycle panes"),
        (
            "↑ ↓  /  j k",
            "move selection · scroll viewer · pick quote (Quotes)",
        ),
        ("← →  /  h l  /  1-6", "switch artifact tab"),
        ("Enter", "open session · switch campaign"),
        ("Shift+↑↓  /  K J", "reorder campaigns (saved)"),
        ("N / E / F", "new campaign · edit campaign · fork campaign"),
        ("r / t / n", "run pipeline · transcribe · notes"),
        ("R", "re-run this session (palette: keep-both to compare)"),
        ("e", "open current artifact in $EDITOR"),
        ("y", "copy current view to clipboard"),
        ("s", "select mode (mouse off; Shift+drag selects text)"),
        ("m", "manage models (install / delete / default)"),
        ("u", "update Ollama (in model manager)"),
        (
            "p",
            "play audio — selected file (Audio) or selected quote (Quotes)",
        ),
        (
            "space · , . · - +",
            "player: pause · seek ∓10s · volume · (S stop)",
        ),
        ("c / a", "compare re-run versions · keep the one shown"),
        ("a (in pickers)", "toggle all checkboxes"),
        ("/", "search notes"),
        (": or Ctrl-P", "command palette"),
        ("T", "cycle theme"),
        ("PgUp / PgDn", "scroll viewer"),
        (
            "mouse",
            "click to select · wheel to scroll · hover to highlight",
        ),
        ("tmux", "set -g mouse on to forward mouse reports"),
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

fn bordered_inner(rect: Rect) -> Rect {
    Rect::new(
        rect.x.saturating_add(1),
        rect.y.saturating_add(1),
        rect.width.saturating_sub(2),
        rect.height.saturating_sub(2),
    )
}

fn register_list_rows(
    app: &mut App,
    inner: Rect,
    offset: usize,
    len: usize,
    target: impl Fn(usize) -> HitTarget,
) {
    for visual_row in 0..inner.height as usize {
        let index = offset + visual_row;
        if index >= len {
            break;
        }
        app.register_hit(
            Rect::new(inner.x, inner.y + visual_row as u16, inner.width, 1),
            target(index),
        );
    }
}

fn prepare_list_state(
    state: &mut ratatui::widgets::ListState,
    selected: Option<usize>,
    len: usize,
    visible: usize,
) {
    let selected = selected.filter(|index| *index < len);
    state.select(selected);
    if len == 0 || visible == 0 {
        *state.offset_mut() = 0;
        return;
    }
    let offset = state.offset_mut();
    if let Some(selected) = selected {
        if selected < *offset {
            *offset = selected;
        } else if selected >= *offset + visible {
            *offset = selected + 1 - visible;
        }
    }
    *offset = (*offset).min(len.saturating_sub(visible));
}

fn register_multiline_list_rows(
    app: &mut App,
    inner: Rect,
    offset: usize,
    len: usize,
    row_height: u16,
    target: impl Fn(usize) -> HitTarget,
) {
    if row_height == 0 {
        return;
    }
    for visual_row in 0..(inner.height / row_height) as usize {
        let index = offset + visual_row;
        if index >= len {
            break;
        }
        app.register_hit(
            Rect::new(
                inner.x,
                inner.y + visual_row as u16 * row_height,
                inner.width,
                row_height,
            ),
            target(index),
        );
    }
}

/// Render a model's release date, colour-coded by age so stale models stand out.
fn released_span(th: &Theme, released: &str) -> Span<'static> {
    let style = match released_age_months(released) {
        None => th.muted_style(),               // unknown release date
        Some(m) if m >= 24 => th.warn_style(),  // 2+ years — clearly old
        Some(m) if m >= 12 => th.muted_style(), // 1–2 years
        Some(_) => th.success_style(),          // < 1 year — fresh
    };
    Span::styled(format!("{released:>8} "), style)
}

/// Months between a `YYYY-MM` string and now; `None` if unparseable.
fn released_age_months(s: &str) -> Option<i64> {
    let (y, m) = s.split_once('-')?;
    let y: i64 = y.trim().parse().ok()?;
    let m: i64 = m.trim().parse().ok()?;
    let now = time::OffsetDateTime::now_utc();
    let ny = now.year() as i64;
    let nm = u8::from(now.month()) as i64;
    Some((ny - y) * 12 + (nm - m))
}

/// Map a hover position inside a bordered list `Rect` to an item index.
fn section_block(title: &str, th: &Theme, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(th.border_style(focused))
        .title(Span::styled(format!(" {title} "), th.title_style(focused)))
}

fn popup_block(title: &str, th: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(th.border_focus)
                .add_modifier(Modifier::BOLD),
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_duration_uses_its_supplied_completion_anchor() {
        let start = std::time::Instant::now();
        assert_eq!(
            stage_duration(start, start + std::time::Duration::from_secs(65)),
            "1:05"
        );
    }

    #[test]
    fn queued_titles_middle_truncate_at_narrow_widths() {
        let truncated = middle_truncate("queued: Install large-v3 → Delete base", 16);
        assert!(truncated.contains('…'));
        assert!(truncated.starts_with("queued:"));
        assert!(truncated.ends_with("base"));
    }
}
