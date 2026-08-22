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
    Action, App, ConfirmAction, FooterCmd, HitTarget, JobRequestBuilder, Overlay, PaletteState,
    Pane, PickerKind, PickerState, SearchState,
};
use super::fuzzy;
use super::jobs::JobKind;

impl App {
    pub fn on_key(&mut self, key: KeyEvent) {
        // Global quit shortcut.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.request_quit();
            return;
        }

        // Overlays capture input first.
        match &mut self.overlay {
            Overlay::Message { .. } => {
                self.overlay = Overlay::None;
                return;
            }
            Overlay::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
                ) {
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
            Overlay::CampaignForm(form) => {
                let action = form.handle_key(key);
                self.handle_campaign_form_action(action);
                return;
            }
            Overlay::TextPrompt(_) => {
                self.on_key_text_prompt(key);
                return;
            }
            Overlay::SpeakerMap(_) => {
                self.on_key_speaker_map(key);
                return;
            }
            Overlay::ThemePicker { .. } => {
                self.on_key_theme_picker(key);
                return;
            }
            Overlay::Confirm { .. } => {
                self.on_key_confirm(key);
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
        // Model manager owns navigation keys while it occupies the content pane.
        if self.models.is_some()
            && matches!(self.pane, Pane::Content)
            && self.on_key_models_view(key)
        {
            return;
        }
        // Reorder campaigns (Campaigns pane): Shift+↑/↓ or K/J.
        if matches!(self.pane, Pane::Campaigns) {
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                KeyCode::Char('N') => {
                    self.dispatch(Action::NewCampaign);
                    return;
                }
                KeyCode::Char('E') => {
                    self.dispatch(Action::CampaignSettings);
                    return;
                }
                KeyCode::Char('F') => {
                    self.dispatch(Action::ForkCampaign);
                    return;
                }
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
            KeyCode::Char('q') => self.request_quit(),
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
            KeyCode::Char('m') => self.dispatch(Action::ManageModels),
            KeyCode::Char('R') => self.dispatch(Action::RerunReplace),
            KeyCode::Char('p') => self.play_context(),
            // Audio player transport (active only while something is playing).
            KeyCode::Char(' ') if self.player.is_some() => self.player_toggle_pause(),
            KeyCode::Char(',') if self.player.is_some() => self.player_seek(-10.0),
            KeyCode::Char('.') if self.player.is_some() => self.player_seek(10.0),
            KeyCode::Char('<') if self.player.is_some() => self.player_seek(-30.0),
            KeyCode::Char('>') if self.player.is_some() => self.player_seek(30.0),
            KeyCode::Char('-') if self.player.is_some() => self.player_volume_change(-10),
            KeyCode::Char('+') | KeyCode::Char('=') if self.player.is_some() => {
                self.player_volume_change(10)
            }
            KeyCode::Char('S') => self.stop_audio(),
            KeyCode::Esc if self.player.is_some() => self.stop_audio(),
            KeyCode::Char('c') => self.toggle_candidate_view(),
            KeyCode::Char('a') => self.keep_shown_version(),
            KeyCode::Char(c @ '1'..='6') => {
                let idx = (c as u8 - b'1') as usize;
                if self.open_session.is_some() && idx < ALL_ARTIFACTS.len() {
                    self.artifact_tab = idx;
                    self.viewing_candidate = false;
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
        let Overlay::Palette(p) = &mut self.overlay else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up if p.cursor > 0 => p.cursor -= 1,
            KeyCode::Down if p.cursor + 1 < p.filtered.len() => p.cursor += 1,
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
                self.apply_theme_picker();
            }
            _ => {}
        }
    }

    fn apply_theme_picker(&mut self) {
        let name = self.theme().name.clone();
        self.global.ui.theme = name.clone();
        self.global.save().ok();
        self.status = format!("Theme: {name}");
        self.overlay = Overlay::None;
    }

    fn on_key_text_prompt(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.overlay = Overlay::None;
            return;
        }
        if key.code == KeyCode::Enter {
            self.submit_text_prompt();
            return;
        }
        let Overlay::TextPrompt(prompt) = &mut self.overlay else {
            return;
        };
        match key.code {
            KeyCode::Backspace => prompt.input.backspace(),
            KeyCode::Delete => prompt.input.delete(),
            KeyCode::Left => prompt.input.left(),
            KeyCode::Right => prompt.input.right(),
            KeyCode::Home => prompt.input.home(),
            KeyCode::End => prompt.input.end(),
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                prompt.input.insert(character)
            }
            _ => {}
        }
        prompt.error = None;
    }

    fn on_key_speaker_map(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up | KeyCode::Char('k') => {
                if let Overlay::SpeakerMap(state) = &mut self.overlay {
                    state.cursor = state.cursor.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Overlay::SpeakerMap(state) = &mut self.overlay {
                    state.cursor = (state.cursor + 1).min(state.labels.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                let cursor = match &self.overlay {
                    Overlay::SpeakerMap(state) => state.cursor,
                    _ => return,
                };
                self.cycle_speaker_at(cursor);
            }
            KeyCode::Char('p') => {
                let cursor = match &self.overlay {
                    Overlay::SpeakerMap(state) => state.cursor,
                    _ => return,
                };
                self.preview_speaker_at(cursor);
            }
            KeyCode::Char('w') => self.save_speaker_map(),
            _ => {}
        }
    }

    fn cycle_speaker_at(&mut self, index: usize) {
        let Overlay::SpeakerMap(state) = &mut self.overlay else {
            return;
        };
        let Some(label) = state.labels.get(index).cloned() else {
            return;
        };
        if state.choices.is_empty() {
            return;
        }
        state.cursor = index;
        let current = state
            .map
            .get(&label)
            .and_then(|name| state.choices.iter().position(|choice| choice == name));
        let next = current
            .map(|choice| (choice + 1) % state.choices.len())
            .unwrap_or(0);
        let choice = &state.choices[next];
        if choice == "Skip" {
            state.map.remove(&label);
        } else {
            state.map.insert(label, choice.clone());
        }
    }

    fn preview_speaker_at(&mut self, index: usize) {
        let preview = if let Overlay::SpeakerMap(state) = &mut self.overlay {
            state.cursor = index;
            state.labels.get(index).and_then(|label| {
                state
                    .preview_samples
                    .get(label)
                    .zip(state.audio.clone())
                    .map(|(sample, audio)| (audio, label.clone(), sample.start, sample.end))
            })
        } else {
            None
        };
        if let Some((audio, label, start, end)) = preview {
            self.start_player_sample(&audio, &label, start, end);
        } else {
            self.status =
                "No source audio or timed diarized subtitle cue available for sample playback"
                    .into();
        }
    }

    fn save_speaker_map(&mut self) {
        let result = if let (Overlay::SpeakerMap(state), Some(campaign)) =
            (&self.overlay, &self.campaign)
        {
            crate::speakers::apply_to_session(&campaign.transcripts_dir(), &state.stem, &state.map)
        } else {
            Ok(())
        };
        match result {
            Ok(()) => {
                self.overlay = Overlay::None;
                self.load_campaign_data();
                self.status = "Speaker mapping saved".into();
            }
            Err(error) => self.message("Could not save mapping", &format!("{error:#}"), true),
        }
    }

    pub fn on_paste(&mut self, text: String) {
        let text: String = text
            .chars()
            .filter_map(|character| match character {
                '\n' | '\r' => Some(' '),
                _ if character.is_control() => None,
                _ => Some(character),
            })
            .collect();
        if text.is_empty() {
            return;
        }
        match &mut self.overlay {
            Overlay::Palette(palette) => {
                palette.query.push_str(&text);
                self.refilter_palette();
            }
            Overlay::Search(search) => {
                search.query.push_str(&text);
                self.run_search();
            }
            Overlay::TextPrompt(prompt) => {
                prompt.input.insert_paste(&text);
                prompt.error = None;
            }
            Overlay::CampaignForm(form) => form.paste(&text),
            _ => {}
        }
    }

    /// Handle a pending re-transcribe or quit confirmation.
    fn on_key_confirm(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                self.overlay = Overlay::None;
                match self.pending_confirm.take() {
                    Some(ConfirmAction::Rerun(target, artifacts, candidate)) => {
                        self.start_rerun(target, artifacts, candidate, true);
                    }
                    Some(ConfirmAction::Quit) => self.should_quit = true,
                    Some(ConfirmAction::DiscardCampaignForm) => {
                        self.discard_pending_campaign_form()
                    }
                    Some(ConfirmAction::SaveCampaignRename) => {
                        self.commit_pending_campaign_rename()
                    }
                    None => {}
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.overlay = Overlay::None;
                match self.pending_confirm.take() {
                    Some(ConfirmAction::Rerun(target, artifacts, candidate)) => {
                        self.start_rerun(target, artifacts, candidate, false);
                    }
                    Some(ConfirmAction::DiscardCampaignForm) => {
                        self.restore_pending_campaign_form()
                    }
                    Some(ConfirmAction::SaveCampaignRename) => self.restore_pending_campaign_save(),
                    Some(ConfirmAction::Quit) | None => {}
                }
            }
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                match self.pending_confirm.take() {
                    Some(ConfirmAction::DiscardCampaignForm) => {
                        self.restore_pending_campaign_form()
                    }
                    Some(ConfirmAction::SaveCampaignRename) => self.restore_pending_campaign_save(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // ---- model manager (content view) -----------------------------------

    /// Handle a key while the model manager occupies the content pane. Returns
    /// `true` if the key was consumed (so global keys still work otherwise).
    fn on_key_models_view(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.models = None;
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.models_move(-1);
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.models_move(1);
                true
            }
            KeyCode::Enter => {
                if self.selected_is_family() {
                    self.toggle_models_expand();
                } else {
                    self.set_model_default();
                }
                true
            }
            KeyCode::Right | KeyCode::Char('l') if self.selected_is_family() => {
                self.toggle_models_expand();
                true
            }
            KeyCode::Left | KeyCode::Char('h') if self.selected_is_family() => {
                self.toggle_models_expand();
                true
            }
            KeyCode::Char('i') => {
                self.model_action(true);
                true
            }
            KeyCode::Char('d') => {
                self.model_action(false);
                true
            }
            KeyCode::Char('u') => {
                self.request_ollama_update();
                true
            }
            KeyCode::Char('g') => {
                self.request_cuda_toolkit_install();
                true
            }
            _ => false,
        }
    }

    // ---- search ----------------------------------------------------------

    fn open_search(&mut self) {
        self.overlay = Overlay::Search(SearchState {
            query: String::new(),
            hits: Vec::new(),
            hit_campaigns: Vec::new(),
            all_campaigns: false,
            cursor: 0,
        });
    }

    fn on_key_search(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('a') {
            if let Overlay::Search(search) = &mut self.overlay {
                search.all_campaigns = !search.all_campaigns;
            }
            self.run_search();
            return;
        }
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
        let (query, all_campaigns) = if let Overlay::Search(search) = &self.overlay {
            (search.query.clone(), search.all_campaigns)
        } else {
            return;
        };
        let scoped_hits = if query.trim().is_empty() {
            Vec::new()
        } else if all_campaigns {
            let mut hits = Vec::new();
            for (index, entry) in self.campaigns.iter().enumerate() {
                if let Ok(campaign) = crate::config::CampaignConfig::load(&entry.path) {
                    hits.extend(
                        index::search(&campaign, query.trim())
                            .unwrap_or_default()
                            .into_iter()
                            .map(|hit| (index, hit)),
                    );
                }
            }
            hits
        } else {
            self.campaign
                .as_ref()
                .map(|campaign| {
                    index::search(campaign, query.trim())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|hit| (self.campaign_idx, hit))
                        .collect()
                })
                .unwrap_or_default()
        };
        if let Overlay::Search(s) = &mut self.overlay {
            s.hit_campaigns = scoped_hits.iter().map(|(index, _)| *index).collect();
            s.hits = scoped_hits.into_iter().map(|(_, hit)| hit).collect();
            if s.cursor >= s.hits.len() {
                s.cursor = 0;
            }
        }
    }

    fn open_search_hit(&mut self) {
        let (stem, kind, campaign_idx) = if let Overlay::Search(s) = &self.overlay {
            match s.hits.get(s.cursor) {
                Some(h) => (
                    h.session.clone(),
                    h.kind.clone(),
                    s.hit_campaigns.get(s.cursor).copied(),
                ),
                None => return,
            }
        } else {
            return;
        };
        if let Some(index) = campaign_idx.filter(|index| *index != self.campaign_idx) {
            self.campaign_idx = index;
            self.camp_state.select(Some(index));
            self.load_campaign_data();
        }
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
            self.message(
                "No audio",
                "No audio files found in the audio/ directory.",
                true,
            );
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
            PickerKind::AudioRun => "Run pipeline — space toggle · a all · ⏎ choose artifacts",
            PickerKind::AudioTranscribe => "Transcribe — space toggle · a all · ⏎ run",
            PickerKind::Artifacts
            | PickerKind::RunArtifacts
            | PickerKind::RerunReplace
            | PickerKind::RerunKeepBoth => "",
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
            self.message(
                "No session",
                "Select a transcript in the Sessions pane first.",
                true,
            );
            return;
        };
        let target = sess.transcript.clone();
        let defaults = self.default_artifacts();
        let items: Vec<String> = ALL_ARTIFACTS
            .iter()
            .map(|a| a.label().to_string())
            .collect();
        let checked: Vec<bool> = ALL_ARTIFACTS.iter().map(|a| defaults.contains(a)).collect();
        self.overlay = Overlay::Picker(PickerState {
            title: format!(
                "Generate notes for {} — space toggle · a all · ⏎ run",
                sess.stem
            ),
            kind: PickerKind::Artifacts,
            items,
            checked,
            cursor: 0,
            target: Some(target),
        });
    }

    /// Artifact picker shown after choosing audio for a full run. Summary is the
    /// required minimum; the rest can be generated later from the transcript.
    fn open_run_artifact_picker(&mut self) {
        let defaults = self.default_artifacts();
        let items: Vec<String> = ALL_ARTIFACTS
            .iter()
            .map(|a| {
                if *a == Artifact::Summary {
                    format!("{} (always)", a.label())
                } else {
                    a.label().to_string()
                }
            })
            .collect();
        let checked: Vec<bool> = ALL_ARTIFACTS
            .iter()
            .map(|a| *a == Artifact::Summary || defaults.contains(a))
            .collect();
        self.overlay = Overlay::Picker(PickerState {
            title: "Artifacts to create now — space toggle · a all · ⏎ run".to_string(),
            kind: PickerKind::RunArtifacts,
            items,
            checked,
            cursor: 0,
            target: None,
        });
    }

    /// Artifact picker for re-running the open (or selected) session. Defaults
    /// to the artifacts that already exist for that session.
    fn open_rerun_picker(&mut self, candidate: bool) {
        // Prefer the open session; fall back to the highlighted one.
        let si = self
            .open_session
            .or(if self.log_selected || self.sessions.is_empty() {
                None
            } else {
                Some(self.session_idx)
            });
        let Some(si) = si else {
            self.message("No session", "Open or select a session first.", true);
            return;
        };
        let Some(sess) = self.sessions.get(si) else {
            return;
        };
        let stem = sess.stem.clone();
        let target = sess.transcript.clone();
        let items: Vec<String> = ALL_ARTIFACTS
            .iter()
            .map(|a| a.label().to_string())
            .collect();
        // Default to the artifacts that already exist for this session.
        let mut checked: Vec<bool> = ALL_ARTIFACTS
            .iter()
            .enumerate()
            .map(|(i, _)| sess.artifacts.get(i).copied().unwrap_or(false))
            .collect();
        if !checked.iter().any(|&b| b) {
            let defaults = self.default_artifacts();
            checked = ALL_ARTIFACTS.iter().map(|a| defaults.contains(a)).collect();
        }
        let (kind, mode) = if candidate {
            (PickerKind::RerunKeepBoth, "keep both to compare")
        } else {
            (PickerKind::RerunReplace, "replace")
        };
        self.overlay = Overlay::Picker(PickerState {
            title: format!("Re-run {stem} ({mode}) — space toggle · a all · ⏎ run"),
            kind,
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
        let Overlay::Picker(p) = &self.overlay else {
            return;
        };
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
                    self.message(
                        "Nothing selected",
                        "Select at least one audio file (space).",
                        true,
                    );
                    return;
                }
                if p.kind == PickerKind::AudioRun {
                    // Choose which artifacts to generate before running.
                    self.pending_run_sessions = sessions;
                    self.open_run_artifact_picker();
                } else {
                    self.overlay = Overlay::None;
                    self.start_job(JobRequestBuilder {
                        title: "Transcribe".into(),
                        kind: JobKind::Transcribe,
                        sessions,
                        transcripts: Vec::new(),
                        artifacts: Vec::new(),
                    });
                }
            }
            PickerKind::RunArtifacts => {
                let mut artifacts: Vec<Artifact> = ALL_ARTIFACTS
                    .iter()
                    .zip(p.checked.iter())
                    .filter(|(_, &c)| c)
                    .map(|(a, _)| *a)
                    .collect();
                // Summary is the guaranteed minimum.
                if !artifacts.contains(&Artifact::Summary) {
                    artifacts.push(Artifact::Summary);
                }
                let sessions = std::mem::take(&mut self.pending_run_sessions);
                if sessions.is_empty() {
                    self.overlay = Overlay::None;
                    return;
                }
                self.overlay = Overlay::None;
                self.start_job(JobRequestBuilder {
                    title: "Run pipeline".into(),
                    kind: JobKind::Run,
                    sessions,
                    transcripts: Vec::new(),
                    artifacts,
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
                    self.message(
                        "Nothing selected",
                        "Select at least one artifact (space).",
                        true,
                    );
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
            PickerKind::RerunReplace | PickerKind::RerunKeepBoth => {
                let candidate = p.kind == PickerKind::RerunKeepBoth;
                let artifacts: Vec<Artifact> = ALL_ARTIFACTS
                    .iter()
                    .zip(p.checked.iter())
                    .filter(|(_, &c)| c)
                    .map(|(a, _)| *a)
                    .collect();
                let target = p.target.clone();
                if artifacts.is_empty() {
                    self.message(
                        "Nothing selected",
                        "Select at least one artifact (space).",
                        true,
                    );
                    return;
                }
                let Some(target) = target else { return };
                self.overlay = Overlay::None;
                // If the ASR model changed since this session was transcribed
                // (and audio is available), ask whether to re-transcribe.
                let stem = target
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Some((old_model, new_model)) = self.rerun_model_change(&stem) {
                    self.pending_confirm = Some(ConfirmAction::Rerun(target, artifacts, candidate));
                    self.overlay = Overlay::Confirm {
                        title: "Re-transcribe?".into(),
                        body: format!(
                            "The audio model changed (was {old_model}, now {new_model}).\n\nRe-transcribe the audio with the new model, or reuse the existing transcript?"
                        ),
                    };
                } else {
                    self.start_rerun(target, artifacts, candidate, false);
                }
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
                let pos = if self.log_selected {
                    0
                } else {
                    self.session_idx + 1
                };
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
            Pane::Content => {
                // In the Quotes tab, arrows move between quotes (highlighting +
                // scrolling to each); elsewhere they scroll the viewer.
                if self.viewing_quotes() && self.move_quote(delta) {
                    return;
                }
                self.scroll_viewer(delta);
            }
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
        self.viewing_candidate = false;
        self.refresh_viewer();
    }

    fn activate(&mut self) {
        match self.pane {
            Pane::Campaigns => {
                if self.campaign_idx < self.campaigns.len() {
                    self.load_campaign_data();
                    self.status = format!(
                        "Switched to {}",
                        self.campaigns
                            .get(self.campaign_idx)
                            .map(|c| c.name.as_str())
                            .unwrap_or("")
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
            self.models = None;
            self.viewing_log = false;
            self.open_session = Some(self.session_idx);
            self.artifact_tab = 0;
            self.viewing_candidate = false;
            self.pane = Pane::Content;
            self.refresh_viewer();
        }
    }

    fn open_log(&mut self) {
        self.models = None;
        self.viewing_log = true;
        self.open_session = None;
        self.pane = Pane::Content;
        self.refresh_viewer();
    }

    // ---- actions ---------------------------------------------------------

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::NewCampaign => self.open_new_campaign_form(),
            Action::CampaignSettings => self.open_campaign_editor(),
            Action::ForkCampaign => self.request_fork_campaign(),
            Action::RunPipeline => self.open_audio_picker(PickerKind::AudioRun),
            Action::Transcribe => self.open_audio_picker(PickerKind::AudioTranscribe),
            Action::GenerateNotes => self.open_artifact_picker(),
            Action::OpenSession => self.open_selected_session(),
            Action::OpenInEditor => {
                if let Some(path) = self.current_artifact_path() {
                    if path.exists() {
                        self.pending_editor = Some(path);
                    } else {
                        self.message(
                            "Not generated",
                            "That artifact has not been generated yet.",
                            true,
                        );
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
                        self.campaigns
                            .get(self.campaign_idx)
                            .map(|c| c.name.as_str())
                            .unwrap_or("")
                    );
                }
            }
            Action::CycleTheme => self.open_theme_picker(),
            Action::ManageModels => self.open_models(),
            Action::UpdateOllama => self.request_ollama_update(),
            Action::InstallCudaToolkit => self.request_cuda_toolkit_install(),
            Action::RerunReplace => self.open_rerun_picker(false),
            Action::RerunKeepBoth => self.open_rerun_picker(true),
            Action::ToggleDiarize => self.toggle_diarize(),
            Action::MapSpeakers => self.request_speaker_mapping(),
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
            Action::Quit => self.request_quit(),
        }
    }

    // ---- mouse -----------------------------------------------------------

    pub fn on_mouse(&mut self, ev: MouseEvent) {
        self.set_hover(ev.column, ev.row);
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let target = self.hit_target_at(ev.column, ev.row);
                self.player_dragging = matches!(target, Some(HitTarget::PlayerTrack));
                self.viewer_dragging = matches!(target, Some(HitTarget::ViewerScrollbar));
                if let Some(target) = target {
                    self.dispatch_mouse_target(target, ev.column, ev.row);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.player_dragging {
                    self.seek_player_at(ev.column);
                } else if self.viewer_dragging {
                    self.drag_viewer_scroll(ev.row);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.player_dragging = false;
                self.viewer_dragging = false;
            }
            MouseEventKind::ScrollDown => self.scroll_mouse_target(ev.column, ev.row, 1),
            MouseEventKind::ScrollUp => self.scroll_mouse_target(ev.column, ev.row, -1),
            _ => {}
        }
    }

    fn dispatch_mouse_target(&mut self, target: HitTarget, col: u16, row: u16) {
        match target {
            HitTarget::CampaignList => self.pane = Pane::Campaigns,
            HitTarget::CampaignRow(index) => {
                if index < self.campaigns.len() {
                    self.pane = Pane::Campaigns;
                    self.campaign_idx = index;
                    self.camp_state.select(Some(index));
                    self.load_campaign_data();
                }
            }
            HitTarget::SessionList => self.pane = Pane::Sessions,
            HitTarget::SessionRow(index) => {
                self.pane = Pane::Sessions;
                if index == 0 {
                    self.log_selected = true;
                    self.sess_state.select(Some(0));
                    self.open_log();
                } else if index - 1 < self.sessions.len() {
                    self.log_selected = false;
                    self.session_idx = index - 1;
                    self.sess_state.select(Some(index));
                    self.open_selected_session();
                }
            }
            HitTarget::AudioList => self.pane = Pane::Audio,
            HitTarget::AudioRow(index) => {
                if index < self.audio.len() {
                    self.pane = Pane::Audio;
                    self.audio_idx = index;
                    self.audio_state.select(Some(index));
                }
            }
            HitTarget::Viewer => self.pane = Pane::Content,
            HitTarget::ViewerScrollbar => self.drag_viewer_scroll(row),
            HitTarget::Job => self.pane = Pane::Content,
            HitTarget::ModelList => self.pane = Pane::Content,
            HitTarget::ModelRow(index) => {
                self.pane = Pane::Content;
                let mut expand = false;
                if let Some(models) = &mut self.models {
                    if let Some(model) = models.rows.get(index) {
                        if model.selectable() {
                            models.cursor = index;
                            expand = model.family;
                        }
                    }
                }
                if expand {
                    self.toggle_models_expand();
                }
            }
            HitTarget::PlayerTrack => self.seek_player_at(col),
            HitTarget::Footer(command) => self.run_footer_cmd(command),
            HitTarget::ArtifactTab(index) => {
                if self.open_session.is_some() {
                    self.artifact_tab = index;
                    self.pane = Pane::Content;
                    self.refresh_viewer();
                }
            }
            HitTarget::ModelButton { row, install } => self.model_action_at(row, install),
            HitTarget::OverlayBarrier => {}
            HitTarget::OverlayDismiss => self.on_key(KeyEvent::from(KeyCode::Esc)),
            HitTarget::PaletteItem(index) => {
                let action = if let Overlay::Palette(palette) = &self.overlay {
                    palette
                        .filtered
                        .get(index)
                        .copied()
                        .map(|item| Action::all()[item])
                } else {
                    None
                };
                if let Some(action) = action {
                    self.overlay = Overlay::None;
                    self.dispatch(action);
                }
            }
            HitTarget::SearchItem(index) => {
                if let Overlay::Search(search) = &mut self.overlay {
                    search.cursor = index;
                }
                self.open_search_hit();
            }
            HitTarget::PickerItem(index) => {
                if let Overlay::Picker(picker) = &mut self.overlay {
                    if index < picker.checked.len() {
                        picker.checked[index] = !picker.checked[index];
                    }
                    picker.cursor = index;
                }
            }
            HitTarget::PickerConfirm => self.confirm_picker(),
            HitTarget::CampaignFormRow(index) => {
                let action = if let Overlay::CampaignForm(form) = &mut self.overlay {
                    form.click_row(index)
                } else {
                    return;
                };
                self.handle_campaign_form_action(action);
            }
            HitTarget::TextPromptInput => {}
            HitTarget::TextPromptSubmit => self.submit_text_prompt(),
            HitTarget::SpeakerRow(index) => self.cycle_speaker_at(index),
            HitTarget::SpeakerPreview => {
                let index = match &self.overlay {
                    Overlay::SpeakerMap(state) => state.cursor,
                    _ => return,
                };
                self.preview_speaker_at(index);
            }
            HitTarget::SpeakerSave => self.save_speaker_map(),
            HitTarget::ThemeRow(index) => {
                if index < self.themes.len() {
                    if let Overlay::ThemePicker { cursor, .. } = &mut self.overlay {
                        *cursor = index;
                    }
                    self.theme_idx = index;
                }
            }
            HitTarget::ThemeApply => self.apply_theme_picker(),
            HitTarget::ConfirmYes => self.on_key_confirm(KeyEvent::from(KeyCode::Char('y'))),
            HitTarget::ConfirmNo => self.on_key_confirm(KeyEvent::from(KeyCode::Char('n'))),
            HitTarget::ConfirmCancel => self.on_key_confirm(KeyEvent::from(KeyCode::Esc)),
        }
    }

    fn scroll_mouse_target(&mut self, col: u16, row: u16, delta: i32) {
        let Some(target) = self.hit_target_at(col, row) else {
            return;
        };
        match target {
            HitTarget::CampaignList | HitTarget::CampaignRow(_) => self.wheel_campaigns(delta),
            HitTarget::SessionList | HitTarget::SessionRow(_) => self.wheel_sessions(delta),
            HitTarget::AudioList | HitTarget::AudioRow(_) => self.wheel_audio(delta),
            HitTarget::Viewer | HitTarget::ViewerScrollbar => self.scroll_viewer(delta * 3),
            HitTarget::Job => self.job_scroll_by(delta * 3),
            HitTarget::ModelList | HitTarget::ModelRow(_) | HitTarget::ModelButton { .. } => {
                for _ in 0..delta.unsigned_abs() {
                    self.models_move(delta.signum());
                }
            }
            HitTarget::OverlayBarrier
            | HitTarget::PaletteItem(_)
            | HitTarget::SearchItem(_)
            | HitTarget::PickerItem(_)
            | HitTarget::PickerConfirm
            | HitTarget::CampaignFormRow(_)
            | HitTarget::TextPromptInput
            | HitTarget::TextPromptSubmit
            | HitTarget::SpeakerRow(_)
            | HitTarget::SpeakerPreview
            | HitTarget::SpeakerSave
            | HitTarget::ThemeRow(_)
            | HitTarget::ThemeApply
            | HitTarget::ConfirmYes
            | HitTarget::ConfirmNo
            | HitTarget::ConfirmCancel => self.overlay_scroll(delta),
            HitTarget::PlayerTrack
            | HitTarget::Footer(_)
            | HitTarget::ArtifactTab(_)
            | HitTarget::OverlayDismiss => {}
        }
    }

    fn wheel_campaigns(&mut self, delta: i32) {
        if self.campaigns.is_empty() {
            return;
        }
        self.pane = Pane::Campaigns;
        let last = self.campaigns.len().saturating_sub(1) as i32;
        self.campaign_idx = (self.campaign_idx as i32 + delta).clamp(0, last) as usize;
        self.camp_state.select(Some(self.campaign_idx));
    }

    fn wheel_sessions(&mut self, delta: i32) {
        self.pane = Pane::Sessions;
        let total = self.sessions.len() + 1;
        let current = if self.log_selected {
            0
        } else {
            self.session_idx + 1
        };
        let index = (current as i32 + delta).clamp(0, total.saturating_sub(1) as i32) as usize;
        self.log_selected = index == 0;
        if index > 0 {
            self.session_idx = index - 1;
        }
        self.sess_state.select(Some(index));
    }

    fn wheel_audio(&mut self, delta: i32) {
        if self.audio.is_empty() {
            return;
        }
        self.pane = Pane::Audio;
        let last = self.audio.len().saturating_sub(1) as i32;
        self.audio_idx = (self.audio_idx as i32 + delta).clamp(0, last) as usize;
        self.audio_state.select(Some(self.audio_idx));
    }

    fn seek_player_at(&mut self, col: u16) {
        let Some(track) = self.hit_rect(HitTarget::PlayerTrack) else {
            return;
        };
        let duration = self
            .player
            .as_ref()
            .map(|player| player.duration())
            .unwrap_or(0.0);
        if let Some(position) = track_position(track, col, duration) {
            self.player_seek_to(position);
        }
    }

    fn drag_viewer_scroll(&mut self, row: u16) {
        let Some(track) = self.hit_rect(HitTarget::ViewerScrollbar) else {
            return;
        };
        let max = self.viewer_lines.len().saturating_sub(1);
        let relative_row = row
            .saturating_sub(track.y)
            .min(track.height.saturating_sub(1)) as usize;
        let denominator = track.height.saturating_sub(1).max(1) as usize;
        self.viewer_scroll =
            (max.saturating_mul(relative_row) / denominator).min(u16::MAX as usize) as u16;
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
            self.message(
                "Nothing to copy",
                "Open an artifact or run a job first.",
                true,
            );
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
        if !self.mouse_enabled {
            self.player_dragging = false;
            self.viewer_dragging = false;
            self.clear_hover();
        }
        self.status = if self.mouse_enabled {
            "Mouse re-enabled".into()
        } else {
            "Select mode — mouse off; drag to select & copy, press s to resume".into()
        };
    }

    /// Scroll (move the cursor of) whichever overlay list is active.
    fn overlay_scroll(&mut self, delta: i32) {
        let theme_count = self.themes.len();
        match &mut self.overlay {
            Overlay::Palette(palette) => {
                palette.cursor = clamp_index(palette.cursor, delta, palette.filtered.len());
            }
            Overlay::Search(search) => {
                search.cursor = clamp_index(search.cursor, delta, search.hits.len());
            }
            Overlay::Picker(picker) => {
                picker.cursor = clamp_index(picker.cursor, delta, picker.items.len());
            }
            Overlay::CampaignForm(form) => form.scroll_by(delta),
            Overlay::SpeakerMap(state) => {
                state.cursor = clamp_index(state.cursor, delta, state.labels.len());
            }
            Overlay::ThemePicker { cursor, .. } => {
                *cursor = clamp_index(*cursor, delta, theme_count);
                self.theme_idx = *cursor;
            }
            Overlay::None
            | Overlay::Help
            | Overlay::TextPrompt(_)
            | Overlay::Message { .. }
            | Overlay::Confirm { .. } => {}
        }
    }

    fn run_footer_cmd(&mut self, cmd: FooterCmd) {
        match cmd {
            FooterCmd::Palette => self.open_palette(),
            FooterCmd::Search => self.open_search(),
            FooterCmd::Help => self.overlay = Overlay::Help,
            FooterCmd::Quit => self.request_quit(),
            FooterCmd::NewCampaign => self.dispatch(Action::NewCampaign),
            FooterCmd::EditCampaign => self.dispatch(Action::CampaignSettings),
            FooterCmd::ForkCampaign => self.dispatch(Action::ForkCampaign),
            FooterCmd::Editor => self.dispatch(Action::OpenInEditor),
            FooterCmd::Run => self.dispatch(Action::RunPipeline),
            FooterCmd::Transcribe => self.dispatch(Action::Transcribe),
            FooterCmd::Notes => self.dispatch(Action::GenerateNotes),
            FooterCmd::Copy => self.copy_current(),
            FooterCmd::Select => self.toggle_select(),
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

fn clamp_index(current: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (current as i32 + delta).clamp(0, len.saturating_sub(1) as i32) as usize
}

fn track_position(track: ratatui::layout::Rect, col: u16, duration: f64) -> Option<f64> {
    if track.width < 2 || duration <= 0.0 {
        return None;
    }
    let last_col = track.x.saturating_add(track.width.saturating_sub(1));
    let offset = col.clamp(track.x, last_col).saturating_sub(track.x) as f64;
    Some((offset / (track.width - 1) as f64 * duration).clamp(0.0, duration))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn track_midpoint_maps_to_the_middle_of_known_audio() {
        let track = Rect::new(10, 4, 11, 1);
        assert_eq!(track_position(track, 15, 120.0), Some(60.0));
        assert_eq!(track_position(track, 15, 0.0), None);
    }
}
