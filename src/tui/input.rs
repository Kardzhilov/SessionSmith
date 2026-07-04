//! Keyboard and mouse input handling for the TUI. Implemented as methods on
//! [`App`] living in a sibling module to keep `app.rs` readable.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::audio;
use crate::index;
use crate::prompts::{Artifact, ALL_ARTIFACTS};
use crate::session::SessionInput;

use super::app::{
    Action, App, FooterCmd, JobRequestBuilder, Overlay, Pane, PaletteState, PickerKind,
    PickerState, SearchState,
};
use super::fuzzy;
use super::jobs::JobKind;

impl App {
    pub fn on_key(&mut self, key: KeyEvent) {
        // Global quit shortcut.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Overlays capture input first.
        match &mut self.overlay {
            Overlay::Message { .. } => {
                self.overlay = Overlay::None;
                return;
            }
            Overlay::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                    self.overlay = Overlay::None;
                }
                return;
            }
            Overlay::Palette(_) => {
                self.on_key_palette(key);
                return;
            }
            Overlay::Search(_) => {
                self.on_key_search(key);
                return;
            }
            Overlay::Picker(_) => {
                self.on_key_picker(key);
                return;
            }
            Overlay::ThemePicker { .. } => {
                self.on_key_theme_picker(key);
                return;
            }
            Overlay::None => {}
        }

        self.on_key_main(key);
    }

    fn on_key_main(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.open_palette();
            return;
        }
        // Reorder campaigns (Campaigns pane): Shift+↑/↓ or K/J.
        if matches!(self.pane, Pane::Campaigns) {
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                KeyCode::Char('K') => {
                    self.move_campaign(-1);
                    return;
                }
                KeyCode::Char('J') => {
                    self.move_campaign(1);
                    return;
                }
                KeyCode::Up if shift => {
                    self.move_campaign(-1);
                    return;
                }
                KeyCode::Down if shift => {
                    self.move_campaign(1);
                    return;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char(':') => self.open_palette(),
            KeyCode::Char('?') => self.overlay = Overlay::Help,
            KeyCode::Char('/') => self.open_search(),
            KeyCode::Tab => self.cycle_pane(1),
            KeyCode::BackTab => self.cycle_pane(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.scroll_viewer(10),
            KeyCode::PageUp => self.scroll_viewer(-10),
            KeyCode::Left | KeyCode::Char('h') => self.switch_tab(-1),
            KeyCode::Right | KeyCode::Char('l') => self.switch_tab(1),
            KeyCode::Enter => self.activate(),
            KeyCode::Char('r') => self.dispatch(Action::RunPipeline),
            KeyCode::Char('t') => self.dispatch(Action::Transcribe),
            KeyCode::Char('n') => self.dispatch(Action::GenerateNotes),
            KeyCode::Char('e') => self.dispatch(Action::OpenInEditor),
            KeyCode::Char('T') => self.dispatch(Action::CycleTheme),
            KeyCode::Char('y') => self.copy_current(),
            KeyCode::Char('s') => self.toggle_select(),
            KeyCode::Char(c @ '1'..='6') => {
                let idx = (c as u8 - b'1') as usize;
                if self.open_session.is_some() && idx < ALL_ARTIFACTS.len() {
                    self.artifact_tab = idx;
                    self.refresh_viewer();
                }
            }
            _ => {}
        }
    }

    // ---- palette ---------------------------------------------------------

    fn open_palette(&mut self) {
        let labels: Vec<&str> = Action::all().iter().map(|a| a.label()).collect();
        let filtered = (0..labels.len()).collect();
        self.overlay = Overlay::Palette(PaletteState {
            query: String::new(),
            filtered,
            cursor: 0,
        });
        self.overlay_state.select(Some(0));
    }

    fn on_key_palette(&mut self, key: KeyEvent) {
        let Overlay::Palette(p) = &mut self.overlay else { return };
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up => {
                if p.cursor > 0 {
                    p.cursor -= 1;
                }
            }
            KeyCode::Down => {
                if p.cursor + 1 < p.filtered.len() {
                    p.cursor += 1;
                }
            }
            KeyCode::Backspace => {
                p.query.pop();
                self.refilter_palette();
            }
            KeyCode::Char(c) => {
                p.query.push(c);
                self.refilter_palette();
            }
            KeyCode::Enter => {
                if let Some(&ai) = p.filtered.get(p.cursor) {
                    let action = Action::all()[ai];
                    self.overlay = Overlay::None;
                    self.dispatch(action);
                }
            }
            _ => {}
        }
    }

    fn refilter_palette(&mut self) {
        let labels: Vec<&str> = Action::all().iter().map(|a| a.label()).collect();
        if let Overlay::Palette(p) = &mut self.overlay {
            p.filtered = if p.query.is_empty() {
                (0..labels.len()).collect()
            } else {
                fuzzy::rank(&p.query, &labels)
            };
            if p.cursor >= p.filtered.len() {
                p.cursor = p.filtered.len().saturating_sub(1);
            }
        }
    }

    // ---- theme picker ----------------------------------------------------

    fn open_theme_picker(&mut self) {
        self.overlay = Overlay::ThemePicker {
            cursor: self.theme_idx,
            original: self.theme_idx,
        };
    }

    fn on_key_theme_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                if let Overlay::ThemePicker { original, .. } = &self.overlay {
                    self.theme_idx = *original;
                }
                self.overlay = Overlay::None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Overlay::ThemePicker { cursor, .. } = &mut self.overlay {
                    if *cursor > 0 {
                        *cursor -= 1;
                    }
                    self.theme_idx = *cursor;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let n = self.themes.len();
                if let Overlay::ThemePicker { cursor, .. } = &mut self.overlay {
                    if *cursor + 1 < n {
                        *cursor += 1;
                    }
                    self.theme_idx = *cursor;
                }
            }
            KeyCode::Enter => {
                let name = self.theme().name.clone();
                self.global.ui.theme = name.clone();
                self.global.save().ok();
                self.status = format!("Theme: {name}");
                self.overlay = Overlay::None;
            }
            _ => {}
        }
    }

    // ---- search ----------------------------------------------------------

    fn open_search(&mut self) {
        self.overlay = Overlay::Search(SearchState {
            query: String::new(),
            hits: Vec::new(),
            cursor: 0,
        });
    }

    fn on_key_search(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                return;
            }
            KeyCode::Up => {
                if let Overlay::Search(s) = &mut self.overlay {
                    if s.cursor > 0 {
                        s.cursor -= 1;
                    }
                }
                return;
            }
            KeyCode::Down => {
                if let Overlay::Search(s) = &mut self.overlay {
                    if s.cursor + 1 < s.hits.len() {
                        s.cursor += 1;
                    }
                }
                return;
            }
            KeyCode::Enter => {
                self.open_search_hit();
                return;
            }
            KeyCode::Backspace => {
                if let Overlay::Search(s) = &mut self.overlay {
                    s.query.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Overlay::Search(s) = &mut self.overlay {
                    s.query.push(c);
                }
            }
            _ => return,
        }
        self.run_search();
    }

    fn run_search(&mut self) {
        let Some(cfg) = &self.campaign else { return };
        let query = if let Overlay::Search(s) = &self.overlay {
            s.query.clone()
        } else {
            return;
        };
        let hits = if query.trim().is_empty() {
            Vec::new()
        } else {
            index::search(cfg, query.trim()).unwrap_or_default()
        };
        if let Overlay::Search(s) = &mut self.overlay {
            s.hits = hits;
            if s.cursor >= s.hits.len() {
                s.cursor = 0;
            }
        }
    }

    fn open_search_hit(&mut self) {
        let (stem, kind) = if let Overlay::Search(s) = &self.overlay {
            match s.hits.get(s.cursor) {
                Some(h) => (h.session.clone(), h.kind.clone()),
                None => return,
            }
        } else {
            return;
        };
        if let Some(si) = self.sessions.iter().position(|s| s.stem == stem) {
            self.viewing_log = false;
            self.open_session = Some(si);
            self.session_idx = si;
            self.log_selected = false;
            self.sess_state.select(Some(si + 1));
            if let Some(tab) = ALL_ARTIFACTS.iter().position(|a| a.filename() == kind) {
                self.artifact_tab = tab;
            }
            self.pane = Pane::Content;
            self.refresh_viewer();
        }
        self.overlay = Overlay::None;
    }

    // ---- pickers ---------------------------------------------------------

    fn open_audio_picker(&mut self, kind: PickerKind) {
        if self.audio.is_empty() {
            self.message("No audio", "No audio files found in the audio/ directory.", true);
            return;
        }
        let items: Vec<String> = self
            .audio
            .iter()
            .map(|f| {
                let mark = if f.already_transcribed { "✓" } else { " " };
                format!("{mark} {}  ({})", f.stem(), audio::human_age(f.mtime))
            })
            .collect();
        let mut checked = vec![false; items.len()];
        if self.audio_idx < checked.len() {
            checked[self.audio_idx] = true;
        }
        let title = match kind {
            PickerKind::AudioRun => "Run pipeline — space to toggle, ⏎ to run",
            PickerKind::AudioTranscribe => "Transcribe — space to toggle, ⏎ to run",
            PickerKind::Artifacts => "",
        }
        .to_string();
        self.overlay = Overlay::Picker(PickerState {
            title,
            kind,
            items,
            checked,
            cursor: 0,
            target: None,
        });
    }

    fn open_artifact_picker(&mut self) {
        let Some(sess) = self.sessions.get(self.session_idx) else {
            self.message("No session", "Select a transcript in the Sessions pane first.", true);
            return;
        };
        let target = sess.transcript.clone();
        let defaults = self.default_artifacts();
        let items: Vec<String> = ALL_ARTIFACTS.iter().map(|a| a.label().to_string()).collect();
        let checked: Vec<bool> = ALL_ARTIFACTS.iter().map(|a| defaults.contains(a)).collect();
        self.overlay = Overlay::Picker(PickerState {
            title: format!("Generate notes for {} — space to toggle, ⏎ to run", sess.stem),
            kind: PickerKind::Artifacts,
            items,
            checked,
            cursor: 0,
            target: Some(target),
        });
    }

    fn on_key_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up | KeyCode::Char('k') => {
                if let Overlay::Picker(p) = &mut self.overlay {
                    if p.cursor > 0 {
                        p.cursor -= 1;
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Overlay::Picker(p) = &mut self.overlay {
                    if p.cursor + 1 < p.items.len() {
                        p.cursor += 1;
                    }
                }
            }
            KeyCode::Char(' ') => {
                if let Overlay::Picker(p) = &mut self.overlay {
                    let c = p.cursor;
                    if c < p.checked.len() {
                        p.checked[c] = !p.checked[c];
                    }
                }
            }
            KeyCode::Char('a') => {
                if let Overlay::Picker(p) = &mut self.overlay {
                    let all = p.checked.iter().all(|&b| b);
                    for b in p.checked.iter_mut() {
                        *b = !all;
                    }
                }
            }
            KeyCode::Enter => self.confirm_picker(),
            _ => {}
        }
    }

    fn confirm_picker(&mut self) {
        let Overlay::Picker(p) = &self.overlay else { return };
        match p.kind {
            PickerKind::AudioRun | PickerKind::AudioTranscribe => {
                let sessions: Vec<SessionInput> = self
                    .audio
                    .iter()
                    .zip(p.checked.iter())
                    .filter(|(_, &c)| c)
                    .map(|(f, _)| SessionInput {
                        files: vec![f.path.clone()],
                        name: f.stem(),
                    })
                    .collect();
                if sessions.is_empty() {
                    self.message("Nothing selected", "Select at least one audio file (space).", true);
                    return;
                }
                let (kind, title) = if p.kind == PickerKind::AudioRun {
                    (JobKind::Run, "Run pipeline")
                } else {
                    (JobKind::Transcribe, "Transcribe")
                };
                self.overlay = Overlay::None;
                self.start_job(JobRequestBuilder {
                    title: title.into(),
                    kind,
                    sessions,
                    transcripts: Vec::new(),
                    artifacts: self.default_artifacts(),
                });
            }
            PickerKind::Artifacts => {
                let artifacts: Vec<Artifact> = ALL_ARTIFACTS
                    .iter()
                    .zip(p.checked.iter())
                    .filter(|(_, &c)| c)
                    .map(|(a, _)| *a)
                    .collect();
                let target = p.target.clone();
                if artifacts.is_empty() {
                    self.message("Nothing selected", "Select at least one artifact (space).", true);
                    return;
                }
                let Some(target) = target else { return };
                self.overlay = Overlay::None;
                self.start_job(JobRequestBuilder {
                    title: "Generate notes".into(),
                    kind: JobKind::Notes,
                    sessions: Vec::new(),
                    transcripts: vec![target],
                    artifacts,
                });
            }
        }
    }

    // ---- navigation ------------------------------------------------------

    fn cycle_pane(&mut self, delta: i32) {
        let order = [Pane::Campaigns, Pane::Sessions, Pane::Audio, Pane::Content];
        let cur = order.iter().position(|p| *p == self.pane).unwrap_or(0) as i32;
        let n = order.len() as i32;
        let next = ((cur + delta) % n + n) % n;
        self.pane = order[next as usize];
    }

    fn move_selection(&mut self, delta: i32) {
        match self.pane {
            Pane::Campaigns => {
                self.campaign_idx = step_idx(self.campaign_idx, delta, self.campaigns.len());
                self.camp_state.select(Some(self.campaign_idx));
            }
            Pane::Sessions => {
                // Row 0 is the synthetic Campaign Log; rows 1.. are sessions.
                let total = self.sessions.len() + 1;
                let pos = if self.log_selected { 0 } else { self.session_idx + 1 };
                let newpos = (pos as i32 + delta).clamp(0, total as i32 - 1) as usize;
                if newpos == 0 {
                    self.log_selected = true;
                } else {
                    self.log_selected = false;
                    self.session_idx = newpos - 1;
                }
                self.sess_state.select(Some(newpos));
            }
            Pane::Audio => {
                self.audio_idx = step_idx(self.audio_idx, delta, self.audio.len());
                self.audio_state.select(Some(self.audio_idx));
            }
            Pane::Content => self.scroll_viewer(delta),
        }
    }

    fn scroll_viewer(&mut self, delta: i32) {
        let max = self.viewer_lines.len().saturating_sub(1) as i32;
        let next = (self.viewer_scroll as i32 + delta).clamp(0, max.max(0));
        self.viewer_scroll = next as u16;
    }

    fn switch_tab(&mut self, delta: i32) {
        if self.open_session.is_none() {
            return;
        }
        let n = ALL_ARTIFACTS.len() as i32;
        let next = ((self.artifact_tab as i32 + delta) % n + n) % n;
        self.artifact_tab = next as usize;
        self.refresh_viewer();
    }

    fn activate(&mut self) {
        match self.pane {
            Pane::Campaigns => {
                if self.campaign_idx < self.campaigns.len() {
                    self.load_campaign_data();
                    self.status = format!(
                        "Switched to {}",
                        self.campaigns.get(self.campaign_idx).map(|c| c.name.as_str()).unwrap_or("")
                    );
                }
            }
            Pane::Sessions => {
                if self.log_selected {
                    self.open_log();
                } else {
                    self.open_selected_session();
                }
            }
            Pane::Audio => self.dispatch(Action::RunPipeline),
            Pane::Content => {}
        }
    }

    fn open_selected_session(&mut self) {
        if self.session_idx < self.sessions.len() {
            self.viewing_log = false;
            self.open_session = Some(self.session_idx);
            self.artifact_tab = 0;
            self.pane = Pane::Content;
            self.refresh_viewer();
        }
    }

    fn open_log(&mut self) {
        self.viewing_log = true;
        self.open_session = None;
        self.pane = Pane::Content;
        self.refresh_viewer();
    }

    // ---- actions ---------------------------------------------------------

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::RunPipeline => self.open_audio_picker(PickerKind::AudioRun),
            Action::Transcribe => self.open_audio_picker(PickerKind::AudioTranscribe),
            Action::GenerateNotes => self.open_artifact_picker(),
            Action::OpenSession => self.open_selected_session(),
            Action::OpenInEditor => {
                if let Some(path) = self.current_artifact_path() {
                    if path.exists() {
                        self.pending_editor = Some(path);
                    } else {
                        self.message("Not generated", "That artifact has not been generated yet.", true);
                    }
                } else {
                    self.message("No artifact", "Open a session in the viewer first.", true);
                }
            }
            Action::Search => self.open_search(),
            Action::NextCampaign => {
                if !self.campaigns.is_empty() {
                    self.campaign_idx = (self.campaign_idx + 1) % self.campaigns.len();
                    self.camp_state.select(Some(self.campaign_idx));
                    self.load_campaign_data();
                    self.status = format!(
                        "Switched to {}",
                        self.campaigns.get(self.campaign_idx).map(|c| c.name.as_str()).unwrap_or("")
                    );
                }
            }
            Action::CycleTheme => self.open_theme_picker(),
            Action::RebuildLog => self.start_job(JobRequestBuilder {
                title: "Rebuild campaign log".into(),
                kind: JobKind::RebuildLog,
                sessions: Vec::new(),
                transcripts: Vec::new(),
                artifacts: Vec::new(),
            }),
            Action::SystemCheck => self.start_job(JobRequestBuilder {
                title: "System check".into(),
                kind: JobKind::Doctor,
                sessions: Vec::new(),
                transcripts: Vec::new(),
                artifacts: Vec::new(),
            }),
            Action::Quit => self.should_quit = true,
        }
    }

    // ---- mouse -----------------------------------------------------------

    pub fn on_mouse(&mut self, ev: MouseEvent) {
        // Track hover position for interactable highlighting.
        self.hover_col = ev.column;
        self.hover_row = ev.row;

        // Overlays get their own click/scroll handling.
        if !matches!(self.overlay, Overlay::None) {
            match ev.kind {
                MouseEventKind::Down(MouseButton::Left) => self.on_overlay_click(ev.column, ev.row),
                MouseEventKind::ScrollDown => self.overlay_scroll(1),
                MouseEventKind::ScrollUp => self.overlay_scroll(-1),
                _ => {}
            }
            return;
        }
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => self.on_click(ev.column, ev.row),
            MouseEventKind::ScrollDown => {
                if rect_contains(self.rects.job, ev.column, ev.row) {
                    self.job_scroll_by(3);
                } else if rect_contains(self.rects.viewer, ev.column, ev.row) {
                    self.scroll_viewer(3);
                } else {
                    self.move_selection(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if rect_contains(self.rects.job, ev.column, ev.row) {
                    self.job_scroll_by(-3);
                } else if rect_contains(self.rects.viewer, ev.column, ev.row) {
                    self.scroll_viewer(-3);
                } else {
                    self.move_selection(-1);
                }
            }
            _ => {}
        }
    }

    fn job_scroll_by(&mut self, delta: i32) {
        self.job_follow = false;
        self.job_scroll = (self.job_scroll as i32 + delta).max(0) as u16;
    }

    /// Copy the currently-shown artifact/log/job text to the system clipboard.
    fn copy_current(&mut self) {
        let text = if self.viewing_log || self.open_session.is_some() {
            self.viewer_lines.join("\n")
        } else if !self.job_log.is_empty() {
            self.job_log
                .iter()
                .map(|(_, m)| m.clone())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        if text.trim().is_empty() {
            self.message("Nothing to copy", "Open an artifact or run a job first.", true);
            return;
        }
        let n = text.lines().count();
        self.copy_pending = Some(text);
        self.status = format!("Copied {n} line(s) to the clipboard");
    }

    /// Toggle mouse capture so the terminal's own text selection works.
    fn toggle_select(&mut self) {
        self.mouse_enabled = !self.mouse_enabled;
        self.mouse_toggle_pending = true;
        self.status = if self.mouse_enabled {
            "Mouse re-enabled".into()
        } else {
            "Select mode — mouse off; drag to select & copy, press s to resume".into()
        };
    }

    /// Scroll (move the cursor of) whichever overlay list is active.
    fn overlay_scroll(&mut self, delta: i32) {
        let code = if delta > 0 { KeyCode::Down } else { KeyCode::Up };
        self.on_key(KeyEvent::from(code));
    }

    fn on_overlay_click(&mut self, col: u16, row: u16) {
        // Simple overlays dismiss on any click.
        if matches!(self.overlay, Overlay::Help | Overlay::Message { .. }) {
            self.overlay = Overlay::None;
            return;
        }

        let list = self.rects.overlay_list;
        if !rect_contains(list, col, row) {
            return;
        }
        let offset = self.overlay_state.offset();
        let len = match &self.overlay {
            Overlay::Palette(p) => p.filtered.len(),
            Overlay::Search(s) => s.hits.len(),
            Overlay::Picker(p) => p.items.len(),
            Overlay::ThemePicker { .. } => self.themes.len(),
            _ => 0,
        };
        let Some(idx) = list_row(list, row, offset, len) else {
            return;
        };

        enum Act {
            None,
            PaletteRun(usize),
            SearchOpen(usize),
            PickerToggle(usize),
            ThemePreview(usize),
        }
        let act = match &self.overlay {
            Overlay::Palette(p) => p
                .filtered
                .get(idx)
                .copied()
                .map(Act::PaletteRun)
                .unwrap_or(Act::None),
            Overlay::Search(_) => Act::SearchOpen(idx),
            Overlay::Picker(_) => Act::PickerToggle(idx),
            Overlay::ThemePicker { .. } => Act::ThemePreview(idx),
            _ => Act::None,
        };

        match act {
            Act::PaletteRun(ai) => {
                let action = Action::all()[ai];
                self.overlay = Overlay::None;
                self.dispatch(action);
            }
            Act::SearchOpen(i) => {
                if let Overlay::Search(s) = &mut self.overlay {
                    s.cursor = i;
                }
                self.open_search_hit();
            }
            Act::PickerToggle(i) => {
                if let Overlay::Picker(p) = &mut self.overlay {
                    if i < p.checked.len() {
                        p.checked[i] = !p.checked[i];
                    }
                    p.cursor = i;
                }
            }
            Act::ThemePreview(i) => {
                if let Overlay::ThemePicker { cursor, .. } = &mut self.overlay {
                    *cursor = i;
                }
                self.theme_idx = i;
            }
            Act::None => {}
        }
    }

    fn run_footer_cmd(&mut self, cmd: FooterCmd) {
        match cmd {
            FooterCmd::Palette => self.open_palette(),
            FooterCmd::Search => self.open_search(),
            FooterCmd::Help => self.overlay = Overlay::Help,
            FooterCmd::Quit => self.should_quit = true,
            FooterCmd::Editor => self.dispatch(Action::OpenInEditor),
            FooterCmd::Run => self.dispatch(Action::RunPipeline),
            FooterCmd::Transcribe => self.dispatch(Action::Transcribe),
            FooterCmd::Notes => self.dispatch(Action::GenerateNotes),
            FooterCmd::Copy => self.copy_current(),
            FooterCmd::Select => self.toggle_select(),
        }
    }

    fn on_click(&mut self, col: u16, row: u16) {
        let r = self.rects.clone();
        // Footer keybar is clickable.
        if rect_contains(r.footer, col, row) {
            if let Some((_, _, _, cmd)) = r
                .footer_hits
                .iter()
                .find(|(a, b, ry, _)| row == *ry && col >= *a && col < *b)
            {
                self.run_footer_cmd(*cmd);
            }
            return;
        }
        if rect_contains(r.campaigns, col, row) {
            self.pane = Pane::Campaigns;
            if let Some(i) = list_row(r.campaigns, row, self.camp_state.offset(), self.campaigns.len()) {
                self.campaign_idx = i;
                self.camp_state.select(Some(i));
                self.load_campaign_data();
            }
        } else if rect_contains(r.sessions, col, row) {
            self.pane = Pane::Sessions;
            if let Some(i) = list_row(r.sessions, row, self.sess_state.offset(), self.sessions.len() + 1) {
                self.sess_state.select(Some(i));
                if i == 0 {
                    self.log_selected = true;
                    self.open_log();
                } else {
                    self.log_selected = false;
                    self.session_idx = i - 1;
                    self.open_selected_session();
                }
            }
        } else if rect_contains(r.audio, col, row) {
            self.pane = Pane::Audio;
            if let Some(i) = list_row(r.audio, row, self.audio_state.offset(), self.audio.len()) {
                self.audio_idx = i;
                self.audio_state.select(Some(i));
            }
        } else if rect_contains(r.tabs, col, row) {
            // Exact hit-testing against the per-tab ranges recorded while drawing.
            if self.open_session.is_some() {
                if let Some(idx) = r.tab_ranges.iter().position(|(a, b)| col >= *a && col < *b) {
                    self.artifact_tab = idx;
                    self.pane = Pane::Content;
                    self.refresh_viewer();
                }
            }
        } else if rect_contains(r.viewer, col, row) {
            self.pane = Pane::Content;
        }
    }
}

fn step_idx(cur: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let n = len as i32;
    (((cur as i32 + delta) % n + n) % n) as usize
}

fn rect_contains(r: ratatui::layout::Rect, col: u16, row: u16) -> bool {
    r.width > 0
        && r.height > 0
        && col >= r.x
        && col < r.x + r.width
        && row >= r.y
        && row < r.y + r.height
}

/// Map a mouse row inside a bordered list `Rect` to an item index.
fn list_row(r: ratatui::layout::Rect, row: u16, offset: usize, len: usize) -> Option<usize> {
    // Account for the top border line.
    let inner_top = r.y + 1;
    if row < inner_top || row >= r.y + r.height {
        return None;
    }
    let idx = offset + (row - inner_top) as usize;
    if idx < len {
        Some(idx)
    } else {
        None
    }
}
