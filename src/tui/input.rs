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
    Action, App, ConfirmAction, FooterCmd, JobRequestBuilder, Overlay, PaletteState, Pane,
    PickerKind, PickerState, SearchState,
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
            Overlay::CampaignSettings(_) => {
                self.on_key_campaign_settings(key);
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
                let name = self.theme().name.clone();
                self.global.ui.theme = name.clone();
                self.global.save().ok();
                self.status = format!("Theme: {name}");
                self.overlay = Overlay::None;
            }
            _ => {}
        }
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
                if let Overlay::SpeakerMap(state) = &mut self.overlay {
                    let Some(label) = state.labels.get(state.cursor).cloned() else {
                        return;
                    };
                    let current = state
                        .map
                        .get(&label)
                        .and_then(|name| state.choices.iter().position(|choice| choice == name));
                    let next = current
                        .map(|index| (index + 1) % state.choices.len())
                        .unwrap_or(0);
                    let choice = &state.choices[next];
                    if choice == "Skip" {
                        state.map.remove(&label);
                    } else {
                        state.map.insert(label, choice.clone());
                    }
                }
            }
            KeyCode::Char('p') => {
                let preview = if let Overlay::SpeakerMap(state) = &self.overlay {
                    state.labels.get(state.cursor).and_then(|label| {
                        state
                            .preview_offsets
                            .get(label)
                            .zip(state.audio.clone())
                            .map(|(offset, audio)| (audio, label.clone(), *offset))
                    })
                } else {
                    None
                };
                if let Some((audio, label, offset)) = preview {
                    self.start_player(&audio, &label, offset);
                } else {
                    self.status =
                        "No source audio or diarized SRT cue available for preview".into();
                }
            }
            KeyCode::Char('w') => {
                let result = if let (Overlay::SpeakerMap(state), Some(campaign)) =
                    (&self.overlay, &self.campaign)
                {
                    crate::speakers::apply_to_session(
                        &campaign.transcripts_dir(),
                        &state.stem,
                        &state.map,
                    )
                } else {
                    Ok(())
                };
                match result {
                    Ok(()) => {
                        self.overlay = Overlay::None;
                        self.load_campaign_data();
                        self.status = "Speaker mapping saved".into();
                    }
                    Err(error) => {
                        self.message("Could not save mapping", &format!("{error:#}"), true)
                    }
                }
            }
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
                    None => {}
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.overlay = Overlay::None;
                match self.pending_confirm.take() {
                    Some(ConfirmAction::Rerun(target, artifacts, candidate)) => {
                        self.start_rerun(target, artifacts, candidate, false);
                    }
                    Some(ConfirmAction::Quit) | None => {}
                }
            }
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                self.pending_confirm = None;
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

    fn open_campaign_settings(&mut self) {
        let Some(campaign) = self.campaign.as_ref() else {
            self.message("No campaign", "Select a campaign before opening settings.", true);
            return;
        };
        let mut presets: Vec<String> = std::fs::read_dir("presets")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
                    .then(|| path.file_stem()?.to_str().map(str::to_string))
                    .flatten()
            })
            .collect();
        presets.sort();
        if !presets.contains(&campaign.system.preset) {
            presets.push(campaign.system.preset.clone());
            presets.sort();
        }
        let preset_index = presets
            .iter()
            .position(|preset| preset == &campaign.system.preset)
            .unwrap_or(0);
        self.overlay = Overlay::CampaignSettings(super::app::CampaignSettingsState {
            cursor: 0,
            artifacts: ALL_ARTIFACTS
                .iter()
                .map(|artifact| campaign.outputs.default.iter().any(|id| id == artifact.id()))
                .collect(),
            diarize: campaign.asr.diarize.unwrap_or(self.global.asr.diarize),
            vad: campaign.asr.vad.unwrap_or(self.global.asr.vad),
            presets,
            preset_index,
        });
    }

    fn on_key_campaign_settings(&mut self, key: KeyEvent) {
        let row_count = ALL_ARTIFACTS.len() + 3;
        match key.code {
            KeyCode::Esc => self.overlay = Overlay::None,
            KeyCode::Up | KeyCode::Char('k') => {
                if let Overlay::CampaignSettings(settings) = &mut self.overlay {
                    settings.cursor = settings.cursor.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Overlay::CampaignSettings(settings) = &mut self.overlay {
                    settings.cursor = (settings.cursor + 1).min(row_count - 1);
                }
            }
            KeyCode::Left | KeyCode::Char('h') => self.cycle_settings_preset(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_settings_preset(1),
            KeyCode::Char(' ') => {
                if let Overlay::CampaignSettings(settings) = &mut self.overlay {
                    match settings.cursor {
                        index if index < settings.artifacts.len() => {
                            settings.artifacts[index] = !settings.artifacts[index];
                        }
                        index if index == ALL_ARTIFACTS.len() => settings.diarize = !settings.diarize,
                        index if index == ALL_ARTIFACTS.len() + 1 => settings.vad = !settings.vad,
                        _ => self.cycle_settings_preset(1),
                    }
                }
            }
            KeyCode::Enter => self.save_campaign_settings(),
            _ => {}
        }
    }

    fn cycle_settings_preset(&mut self, delta: i32) {
        if let Overlay::CampaignSettings(settings) = &mut self.overlay {
            settings.preset_index = step_idx(settings.preset_index, delta, settings.presets.len());
        }
    }

    fn save_campaign_settings(&mut self) {
        let Overlay::CampaignSettings(settings) = &self.overlay else {
            return;
        };
        let Some(path) = self.campaigns.get(self.campaign_idx).map(|entry| entry.path.clone()) else {
            return;
        };
        let Some(mut campaign) = self.campaign.clone() else {
            return;
        };
        campaign.outputs.default = ALL_ARTIFACTS
            .iter()
            .zip(&settings.artifacts)
            .filter_map(|(artifact, selected)| selected.then(|| artifact.id().to_string()))
            .collect();
        campaign.asr.diarize = Some(settings.diarize);
        campaign.asr.vad = Some(settings.vad);
        if let Some(preset) = settings.presets.get(settings.preset_index) {
            campaign.system.preset = preset.clone();
        }
        match campaign.save(&path) {
            Ok(()) => {
                self.campaign = Some(campaign);
                self.load_campaign_data();
                self.status = "Campaign settings saved".into();
                self.overlay = Overlay::None;
            }
            Err(error) => self.message("Could not save settings", &error.to_string(), true),
        }
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
            Action::NewCampaign => self.request_new_campaign(),
            Action::CampaignSettings => self.open_campaign_settings(),
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
                if self.models.is_some() && rect_contains(self.rects.models_pane, ev.column, ev.row)
                {
                    self.models_move(1);
                } else if rect_contains(self.rects.job, ev.column, ev.row) {
                    self.job_scroll_by(3);
                } else if rect_contains(self.rects.viewer, ev.column, ev.row) {
                    self.scroll_viewer(3);
                } else {
                    self.move_selection(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if self.models.is_some() && rect_contains(self.rects.models_pane, ev.column, ev.row)
                {
                    self.models_move(-1);
                } else if rect_contains(self.rects.job, ev.column, ev.row) {
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
        self.status = if self.mouse_enabled {
            "Mouse re-enabled".into()
        } else {
            "Select mode — mouse off; drag to select & copy, press s to resume".into()
        };
    }

    /// Scroll (move the cursor of) whichever overlay list is active.
    fn overlay_scroll(&mut self, delta: i32) {
        let code = if delta > 0 {
            KeyCode::Down
        } else {
            KeyCode::Up
        };
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
            FooterCmd::Quit => self.request_quit(),
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
        if rect_contains(r.player_track, col, row) {
            if let Some(player) = &self.player {
                if player.duration() > 0.0 {
                    if let Some(position) = track_position(r.player_track, col, player.duration()) {
                        self.player_seek_to(position);
                    }
                }
            }
            return;
        }
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
            if let Some(i) = list_row(
                r.campaigns,
                row,
                self.camp_state.offset(),
                self.campaigns.len(),
            ) {
                self.campaign_idx = i;
                self.camp_state.select(Some(i));
                self.load_campaign_data();
            }
        } else if rect_contains(r.sessions, col, row) {
            self.pane = Pane::Sessions;
            if let Some(i) = list_row(
                r.sessions,
                row,
                self.sess_state.offset(),
                self.sessions.len() + 1,
            ) {
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
        } else if self.models.is_some() && rect_contains(r.models_pane, col, row) {
            self.pane = Pane::Content;
            // A per-row [install]/[delete] button?
            if let Some((_, _, _, ri, install)) = r
                .model_buttons
                .iter()
                .find(|(a, b, by, _, _)| row == *by && col >= *a && col < *b)
            {
                self.model_action_at(*ri, *install);
            } else {
                // Otherwise select the clicked row; toggle it if it's a family.
                let inner_top = r.models_pane.y;
                let mut toggle = false;
                if let Some(s) = &mut self.models {
                    if row >= inner_top {
                        let ri = s.scroll + (row - inner_top) as usize;
                        if let Some(rw) = s.rows.get(ri) {
                            if rw.selectable() {
                                s.cursor = ri;
                                toggle = rw.family;
                            }
                        }
                    }
                }
                if toggle {
                    self.toggle_models_expand();
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

fn track_position(track: ratatui::layout::Rect, col: u16, duration: f64) -> Option<f64> {
    if track.width < 2 || duration <= 0.0 || !rect_contains(track, col, track.y) {
        return None;
    }
    let offset = (col - track.x) as f64;
    Some((offset / (track.width - 1) as f64 * duration).clamp(0.0, duration))
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
