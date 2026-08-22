//! Campaign editor state and schema-to-form mapping.

use anyhow::{anyhow, bail, Result};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::{Path, PathBuf};

use crate::config::{CampaignConfig, Player};
use crate::prompts::ALL_ARTIFACTS;

use super::form::TextInput;

#[derive(Clone, Debug)]
pub(super) enum CampaignFormMode {
    Create,
    Edit { path: PathBuf },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CampaignField {
    Name,
    Gm,
    Setting,
    Notes,
    Players,
    Preset,
    SystemOverrides,
    Artifact(usize),
    BackendKind,
    BackendBaseUrl,
    BackendApiKey,
    BackendModel,
    AsrBinary,
    AsrModel,
    AsrModelDir,
    AsrThreads,
    AsrDiarize,
    AsrHfToken,
    AsrVad,
    AsrDevice,
    AsrEngine,
    Vocabulary,
    Replacements,
    VocabPrompt,
    Speakers,
    PromptBullets,
    PromptDmNotes,
    PromptRecap,
    PromptSummary,
    PromptStory,
    PromptQuotes,
    Save,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlayerField {
    Player,
    Character,
    Ancestry,
    Class,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListTarget {
    Vocabulary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MapTarget {
    Replacements,
    Speakers,
}

#[derive(Clone, Debug)]
enum CampaignFormPage {
    Main,
    Players {
        cursor: usize,
    },
    PlayerEditor {
        index: Option<usize>,
        cursor: usize,
        player: Player,
    },
    List {
        target: ListTarget,
        cursor: usize,
    },
    Map {
        target: MapTarget,
        cursor: usize,
    },
}

#[derive(Clone, Debug)]
enum InlineTarget {
    Field(CampaignField),
    Player(PlayerField),
    List {
        target: ListTarget,
        index: Option<usize>,
    },
    Map {
        target: MapTarget,
        index: Option<usize>,
    },
}

#[derive(Clone, Debug)]
struct InlineEdit {
    target: InlineTarget,
    input: TextInput,
}

#[derive(Clone, Copy)]
enum FormRow {
    Section(&'static str),
    Field(CampaignField),
}

pub(super) struct CampaignFormRow {
    pub label: String,
    pub value: String,
    pub selectable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CampaignFormAction {
    None,
    Save,
    Close,
    RequestDiscard,
    OpenLongText(CampaignField),
}

#[derive(Clone, Debug)]
pub(super) struct CampaignFormState {
    pub mode: CampaignFormMode,
    pub original: CampaignConfig,
    draft: CampaignConfig,
    presets: Vec<String>,
    cursor: usize,
    scroll: usize,
    page: CampaignFormPage,
    inline: Option<InlineEdit>,
    pub dirty: bool,
    pub error: Option<String>,
}

impl CampaignFormState {
    pub fn create(presets: Vec<String>) -> Self {
        let draft = crate::campaign_ops::new_campaign_config(
            "New Campaign".into(),
            String::new(),
            String::new(),
            Vec::new(),
            "generic".into(),
        );
        Self::with_config(CampaignFormMode::Create, draft, presets)
    }

    pub fn edit(config: &CampaignConfig, path: PathBuf, presets: Vec<String>) -> Self {
        Self::with_config(CampaignFormMode::Edit { path }, config.clone(), presets)
    }

    fn with_config(
        mode: CampaignFormMode,
        config: CampaignConfig,
        mut presets: Vec<String>,
    ) -> Self {
        if !presets.contains(&config.system.preset) {
            presets.push(config.system.preset.clone());
        }
        if !presets.iter().any(|preset| preset == "generic") {
            presets.push("generic".into());
        }
        presets.sort();
        presets.dedup();
        Self {
            mode,
            original: config.clone(),
            draft: config,
            presets,
            cursor: 1,
            scroll: 0,
            page: CampaignFormPage::Main,
            inline: None,
            dirty: false,
            error: None,
        }
    }

    pub fn title(&self) -> String {
        match &self.page {
            CampaignFormPage::Main => match self.mode {
                CampaignFormMode::Create => "New campaign".into(),
                CampaignFormMode::Edit { .. } => {
                    format!("Edit campaign - {}", self.draft.campaign.name)
                }
            },
            CampaignFormPage::Players { .. } => "Campaign players".into(),
            CampaignFormPage::PlayerEditor { index, .. } => {
                if index.is_some() {
                    "Edit player".into()
                } else {
                    "Add player".into()
                }
            }
            CampaignFormPage::List { target, .. } => match target {
                ListTarget::Vocabulary => "Campaign vocabulary".into(),
            },
            CampaignFormPage::Map { target, .. } => match target {
                MapTarget::Replacements => "Transcript replacements".into(),
                MapTarget::Speakers => "Speaker labels".into(),
            },
        }
    }

    pub fn rows(&self) -> Vec<CampaignFormRow> {
        match &self.page {
            CampaignFormPage::Main => self
                .main_rows()
                .into_iter()
                .map(|row| match row {
                    FormRow::Section(label) => CampaignFormRow {
                        label: label.into(),
                        value: String::new(),
                        selectable: false,
                    },
                    FormRow::Field(field) => CampaignFormRow {
                        label: self.field_label(field),
                        value: self.field_value(field),
                        selectable: true,
                    },
                })
                .collect(),
            CampaignFormPage::Players { .. } => {
                let mut rows: Vec<CampaignFormRow> = self
                    .draft
                    .players
                    .iter()
                    .map(|player| CampaignFormRow {
                        label: player.player.clone(),
                        value: player_summary(player),
                        selectable: true,
                    })
                    .collect();
                rows.push(CampaignFormRow {
                    label: "Add player".into(),
                    value: String::new(),
                    selectable: true,
                });
                rows
            }
            CampaignFormPage::PlayerEditor { player, .. } => vec![
                CampaignFormRow {
                    label: "Player".into(),
                    value: self.player_value(player, PlayerField::Player),
                    selectable: true,
                },
                CampaignFormRow {
                    label: "Character".into(),
                    value: self.player_value(player, PlayerField::Character),
                    selectable: true,
                },
                CampaignFormRow {
                    label: "Ancestry / species".into(),
                    value: self.player_value(player, PlayerField::Ancestry),
                    selectable: true,
                },
                CampaignFormRow {
                    label: "Class / role".into(),
                    value: self.player_value(player, PlayerField::Class),
                    selectable: true,
                },
                CampaignFormRow {
                    label: "Save player".into(),
                    value: String::new(),
                    selectable: true,
                },
            ],
            CampaignFormPage::List { target, .. } => {
                let mut rows: Vec<CampaignFormRow> = self
                    .list_values(*target)
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| CampaignFormRow {
                        label: format!("{}", index + 1),
                        value: self
                            .inline_list_value(*target, index)
                            .unwrap_or_else(|| value.clone()),
                        selectable: true,
                    })
                    .collect();
                rows.push(CampaignFormRow {
                    label: "Add entry".into(),
                    value: self
                        .inline_list_value(*target, self.list_values(*target).len())
                        .unwrap_or_default(),
                    selectable: true,
                });
                rows
            }
            CampaignFormPage::Map { target, .. } => {
                let entries = self.map_entries(*target);
                let mut rows: Vec<CampaignFormRow> = entries
                    .iter()
                    .enumerate()
                    .map(|(index, (key, value))| CampaignFormRow {
                        label: key.clone(),
                        value: self
                            .inline_map_value(*target, index)
                            .unwrap_or_else(|| value.clone()),
                        selectable: true,
                    })
                    .collect();
                rows.push(CampaignFormRow {
                    label: "Add mapping".into(),
                    value: self
                        .inline_map_value(*target, entries.len())
                        .unwrap_or_else(|| "key = value".into()),
                    selectable: true,
                });
                rows
            }
        }
    }

    pub fn cursor(&self) -> usize {
        match &self.page {
            CampaignFormPage::Main => self.cursor,
            CampaignFormPage::Players { cursor }
            | CampaignFormPage::List { cursor, .. }
            | CampaignFormPage::Map { cursor, .. } => *cursor,
            CampaignFormPage::PlayerEditor { cursor, .. } => *cursor,
        }
    }

    pub fn ensure_cursor_visible(&mut self, visible: usize) {
        if visible == 0 {
            return;
        }
        let rows_len = self.rows().len();
        let cursor = self.cursor();
        if cursor < self.scroll {
            self.scroll = cursor;
        } else if cursor >= self.scroll + visible {
            self.scroll = cursor + 1 - visible;
        }
        if self.scroll >= rows_len {
            self.scroll = rows_len.saturating_sub(1);
        }
    }

    pub fn build_config(&self) -> Result<CampaignConfig> {
        if self.inline.is_some() {
            bail!("finish the active field before saving");
        }
        let mut config = self.draft.clone();
        config.campaign.name = config.campaign.name.trim().to_string();
        if config.campaign.name.is_empty() {
            bail!("campaign name cannot be empty");
        }
        if config.system.preset.trim().is_empty() {
            bail!("a system preset is required");
        }
        Ok(config)
    }

    pub fn edit_path(&self) -> Option<&Path> {
        match &self.mode {
            CampaignFormMode::Create => None,
            CampaignFormMode::Edit { path } => Some(path),
        }
    }

    pub fn changed_slug(&self, config: &CampaignConfig) -> bool {
        self.original.slug() != config.slug()
    }

    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    pub fn take_long_text(&self, field: CampaignField) -> Option<String> {
        match field {
            CampaignField::Notes => Some(self.draft.campaign.notes.clone()),
            CampaignField::SystemOverrides => Some(self.draft.system.overrides.clone()),
            CampaignField::PromptBullets => {
                self.draft.prompts.bullets.clone().or(Some(String::new()))
            }
            CampaignField::PromptDmNotes => {
                self.draft.prompts.dm_notes.clone().or(Some(String::new()))
            }
            CampaignField::PromptRecap => self.draft.prompts.recap.clone().or(Some(String::new())),
            CampaignField::PromptSummary => {
                self.draft.prompts.summary.clone().or(Some(String::new()))
            }
            CampaignField::PromptStory => self.draft.prompts.story.clone().or(Some(String::new())),
            CampaignField::PromptQuotes => {
                self.draft.prompts.quotes.clone().or(Some(String::new()))
            }
            _ => None,
        }
    }

    pub fn apply_long_text(&mut self, field: CampaignField, value: String) {
        match field {
            CampaignField::Notes => self.draft.campaign.notes = value,
            CampaignField::SystemOverrides => self.draft.system.overrides = value,
            CampaignField::PromptBullets => self.draft.prompts.bullets = optional_text(value),
            CampaignField::PromptDmNotes => self.draft.prompts.dm_notes = optional_text(value),
            CampaignField::PromptRecap => self.draft.prompts.recap = optional_text(value),
            CampaignField::PromptSummary => self.draft.prompts.summary = optional_text(value),
            CampaignField::PromptStory => self.draft.prompts.story = optional_text(value),
            CampaignField::PromptQuotes => self.draft.prompts.quotes = optional_text(value),
            _ => return,
        }
        self.dirty = true;
        self.error = None;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> CampaignFormAction {
        if self.inline.is_some() {
            return self.handle_inline_key(key);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            return match self.page {
                CampaignFormPage::Main => CampaignFormAction::Save,
                CampaignFormPage::PlayerEditor { .. } => {
                    self.finish_player();
                    CampaignFormAction::None
                }
                _ => CampaignFormAction::None,
            };
        }

        match key.code {
            KeyCode::Esc => self.escape(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_cursor(-1);
                CampaignFormAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_cursor(1);
                CampaignFormAction::None
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.cycle_current(-1);
                CampaignFormAction::None
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => {
                self.cycle_current(1);
                CampaignFormAction::None
            }
            KeyCode::Char('a') => {
                self.add_current();
                CampaignFormAction::None
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                self.remove_current();
                CampaignFormAction::None
            }
            KeyCode::Enter => self.activate_current(),
            _ => CampaignFormAction::None,
        }
    }

    fn handle_inline_key(&mut self, key: KeyEvent) -> CampaignFormAction {
        let Some(edit) = &mut self.inline else {
            return CampaignFormAction::None;
        };
        match key.code {
            KeyCode::Esc => {
                self.inline = None;
                self.error = None;
            }
            KeyCode::Enter => {
                if let Err(error) = self.commit_inline() {
                    self.error = Some(error.to_string());
                }
            }
            KeyCode::Backspace => edit.input.backspace(),
            KeyCode::Delete => edit.input.delete(),
            KeyCode::Left => edit.input.left(),
            KeyCode::Right => edit.input.right(),
            KeyCode::Home => edit.input.home(),
            KeyCode::End => edit.input.end(),
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                edit.input.insert(character)
            }
            _ => {}
        }
        CampaignFormAction::None
    }

    fn escape(&mut self) -> CampaignFormAction {
        if !matches!(self.page, CampaignFormPage::Main) {
            self.page = CampaignFormPage::Main;
            self.inline = None;
            self.error = None;
            self.scroll = 0;
            return CampaignFormAction::None;
        }
        if self.dirty {
            CampaignFormAction::RequestDiscard
        } else {
            CampaignFormAction::Close
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        if matches!(self.page, CampaignFormPage::Main) {
            let selectable: Vec<usize> = self
                .main_rows()
                .iter()
                .enumerate()
                .filter_map(|(index, row)| matches!(row, FormRow::Field(_)).then_some(index))
                .collect();
            if selectable.is_empty() {
                return;
            }
            let current = selectable
                .iter()
                .position(|index| *index == self.cursor)
                .unwrap_or(0) as i32;
            let next = (current + delta).clamp(0, selectable.len() as i32 - 1) as usize;
            self.cursor = selectable[next];
            return;
        }

        let len = self.rows().len();
        let next = |current: &mut usize| {
            *current = (*current as i32 + delta).clamp(0, len.saturating_sub(1) as i32) as usize;
        };
        match &mut self.page {
            CampaignFormPage::Players { cursor }
            | CampaignFormPage::List { cursor, .. }
            | CampaignFormPage::Map { cursor, .. }
            | CampaignFormPage::PlayerEditor { cursor, .. } => next(cursor),
            CampaignFormPage::Main => {}
        }
    }

    fn cycle_current(&mut self, delta: i32) {
        let CampaignFormPage::Main = self.page else {
            return;
        };
        let Some(FormRow::Field(field)) = self.main_rows().get(self.cursor).copied() else {
            return;
        };
        match field {
            CampaignField::Preset => {
                if self.presets.is_empty() {
                    return;
                }
                let current = self
                    .presets
                    .iter()
                    .position(|preset| preset == &self.draft.system.preset)
                    .unwrap_or(0) as i32;
                let next = (current + delta).rem_euclid(self.presets.len() as i32) as usize;
                self.draft.system.preset = self.presets[next].clone();
                self.mark_dirty();
            }
            CampaignField::Artifact(index) => {
                let id = ALL_ARTIFACTS[index].id();
                if let Some(position) = self
                    .draft
                    .outputs
                    .default
                    .iter()
                    .position(|item| item == id)
                {
                    self.draft.outputs.default.remove(position);
                } else {
                    self.draft.outputs.default.push(id.to_string());
                }
                self.mark_dirty();
            }
            CampaignField::BackendKind => {
                cycle_optional_string(
                    &mut self.draft.backend.kind,
                    &["ollama", "openai", "anthropic"],
                    delta,
                );
                self.mark_dirty();
            }
            CampaignField::AsrDiarize => {
                cycle_tristate(&mut self.draft.asr.diarize, delta);
                self.mark_dirty();
            }
            CampaignField::AsrVad => {
                cycle_tristate(&mut self.draft.asr.vad, delta);
                self.mark_dirty();
            }
            CampaignField::AsrDevice => {
                cycle_optional_string(&mut self.draft.asr.device, &["auto", "cuda", "cpu"], delta);
                self.mark_dirty();
            }
            CampaignField::VocabPrompt => {
                self.draft.transcription.vocab_prompt = !self.draft.transcription.vocab_prompt;
                self.mark_dirty();
            }
            _ => {}
        }
    }

    fn activate_current(&mut self) -> CampaignFormAction {
        match &self.page {
            CampaignFormPage::Main => {
                let Some(FormRow::Field(field)) = self.main_rows().get(self.cursor).copied() else {
                    return CampaignFormAction::None;
                };
                match field {
                    CampaignField::Notes
                    | CampaignField::SystemOverrides
                    | CampaignField::PromptBullets
                    | CampaignField::PromptDmNotes
                    | CampaignField::PromptRecap
                    | CampaignField::PromptSummary
                    | CampaignField::PromptStory
                    | CampaignField::PromptQuotes => CampaignFormAction::OpenLongText(field),
                    CampaignField::Players => {
                        self.page = CampaignFormPage::Players { cursor: 0 };
                        self.scroll = 0;
                        CampaignFormAction::None
                    }
                    CampaignField::Vocabulary => {
                        self.page = CampaignFormPage::List {
                            target: ListTarget::Vocabulary,
                            cursor: 0,
                        };
                        self.scroll = 0;
                        CampaignFormAction::None
                    }
                    CampaignField::Replacements => {
                        self.page = CampaignFormPage::Map {
                            target: MapTarget::Replacements,
                            cursor: 0,
                        };
                        self.scroll = 0;
                        CampaignFormAction::None
                    }
                    CampaignField::Speakers => {
                        self.page = CampaignFormPage::Map {
                            target: MapTarget::Speakers,
                            cursor: 0,
                        };
                        self.scroll = 0;
                        CampaignFormAction::None
                    }
                    CampaignField::Save => CampaignFormAction::Save,
                    CampaignField::Preset
                    | CampaignField::Artifact(_)
                    | CampaignField::BackendKind
                    | CampaignField::AsrDiarize
                    | CampaignField::AsrVad
                    | CampaignField::AsrDevice
                    | CampaignField::VocabPrompt => {
                        self.cycle_current(1);
                        CampaignFormAction::None
                    }
                    _ => {
                        self.start_inline_field(field);
                        CampaignFormAction::None
                    }
                }
            }
            CampaignFormPage::Players { cursor } => {
                if *cursor < self.draft.players.len() {
                    self.open_player_editor(Some(*cursor));
                } else {
                    self.open_player_editor(None);
                }
                CampaignFormAction::None
            }
            CampaignFormPage::PlayerEditor { cursor, .. } => {
                if *cursor == 4 {
                    self.finish_player();
                } else if let Some(field) = player_field_at(*cursor) {
                    self.start_inline_player(field);
                }
                CampaignFormAction::None
            }
            CampaignFormPage::List { target, cursor } => {
                let index = (*cursor < self.list_values(*target).len()).then_some(*cursor);
                self.start_inline_list(*target, index);
                CampaignFormAction::None
            }
            CampaignFormPage::Map { target, cursor } => {
                let index = (*cursor < self.map_entries(*target).len()).then_some(*cursor);
                self.start_inline_map(*target, index);
                CampaignFormAction::None
            }
        }
    }

    fn add_current(&mut self) {
        match self.page {
            CampaignFormPage::Players { .. } => self.open_player_editor(None),
            CampaignFormPage::List { target, .. } => self.start_inline_list(target, None),
            CampaignFormPage::Map { target, .. } => self.start_inline_map(target, None),
            CampaignFormPage::Main | CampaignFormPage::PlayerEditor { .. } => {}
        }
    }

    fn remove_current(&mut self) {
        enum RemoveTarget {
            Player(usize),
            List(ListTarget, usize),
            Map(MapTarget, usize),
        }

        let target = match &self.page {
            CampaignFormPage::Players { cursor } if *cursor < self.draft.players.len() => {
                Some(RemoveTarget::Player(*cursor))
            }
            CampaignFormPage::List { target, cursor } => Some(RemoveTarget::List(*target, *cursor)),
            CampaignFormPage::Map { target, cursor } => Some(RemoveTarget::Map(*target, *cursor)),
            _ => None,
        };
        let Some(target) = target else {
            return;
        };

        let next_cursor = match target {
            RemoveTarget::Player(cursor) => {
                self.draft.players.remove(cursor);
                cursor.min(self.draft.players.len())
            }
            RemoveTarget::List(target, cursor) => {
                let values = self.list_values_mut(target);
                if cursor >= values.len() {
                    return;
                }
                values.remove(cursor);
                cursor.min(values.len())
            }
            RemoveTarget::Map(target, cursor) => {
                let entries = self.map_entries(target);
                let Some((key, _)) = entries.get(cursor) else {
                    return;
                };
                self.map_mut(target).remove(key);
                cursor.min(entries.len().saturating_sub(1))
            }
        };
        self.set_page_cursor(next_cursor);
        self.mark_dirty();
    }

    fn set_page_cursor(&mut self, cursor: usize) {
        match &mut self.page {
            CampaignFormPage::Players { cursor: current }
            | CampaignFormPage::List {
                cursor: current, ..
            }
            | CampaignFormPage::Map {
                cursor: current, ..
            }
            | CampaignFormPage::PlayerEditor {
                cursor: current, ..
            } => *current = cursor,
            CampaignFormPage::Main => self.cursor = cursor,
        }
    }

    fn start_inline_field(&mut self, field: CampaignField) {
        let Some(value) = self.field_edit_value(field) else {
            return;
        };
        self.inline = Some(InlineEdit {
            target: InlineTarget::Field(field),
            input: TextInput::new(value),
        });
        self.error = None;
    }

    fn start_inline_player(&mut self, field: PlayerField) {
        let Some(player) = self.current_player_editor() else {
            return;
        };
        let value = player_field_value(player, field).to_string();
        self.inline = Some(InlineEdit {
            target: InlineTarget::Player(field),
            input: TextInput::new(value),
        });
        self.error = None;
    }

    fn start_inline_list(&mut self, target: ListTarget, index: Option<usize>) {
        let value = index
            .and_then(|index| self.list_values(target).get(index).cloned())
            .unwrap_or_default();
        self.inline = Some(InlineEdit {
            target: InlineTarget::List { target, index },
            input: TextInput::new(value),
        });
        self.error = None;
    }

    fn start_inline_map(&mut self, target: MapTarget, index: Option<usize>) {
        let value = index
            .and_then(|index| self.map_entries(target).get(index).cloned())
            .map(|(key, value)| format!("{key} = {value}"))
            .unwrap_or_default();
        self.inline = Some(InlineEdit {
            target: InlineTarget::Map { target, index },
            input: TextInput::new(value),
        });
        self.error = None;
    }

    fn commit_inline(&mut self) -> Result<()> {
        let Some(edit) = self.inline.clone() else {
            return Ok(());
        };
        match edit.target {
            InlineTarget::Field(field) => self.set_field_text(field, edit.input.value)?,
            InlineTarget::Player(field) => self.set_player_text(field, edit.input.value)?,
            InlineTarget::List { target, index } => {
                self.set_list_text(target, index, edit.input.value)?
            }
            InlineTarget::Map { target, index } => {
                self.set_map_text(target, index, edit.input.value)?
            }
        }
        self.inline = None;
        self.mark_dirty();
        Ok(())
    }

    fn set_field_text(&mut self, field: CampaignField, value: String) -> Result<()> {
        match field {
            CampaignField::Name => self.draft.campaign.name = value,
            CampaignField::Gm => self.draft.campaign.gm = value,
            CampaignField::Setting => self.draft.campaign.setting = value,
            CampaignField::BackendBaseUrl => self.draft.backend.base_url = optional_text(value),
            CampaignField::BackendApiKey => self.draft.backend.api_key = optional_text(value),
            CampaignField::BackendModel => self.draft.backend.model = optional_text(value),
            CampaignField::AsrBinary => {
                self.draft.asr.binary = optional_text(value).map(PathBuf::from)
            }
            CampaignField::AsrModel => self.draft.asr.model = optional_text(value),
            CampaignField::AsrModelDir => {
                self.draft.asr.model_dir = optional_text(value).map(PathBuf::from)
            }
            CampaignField::AsrThreads => {
                self.draft.asr.threads = if value.trim().is_empty() {
                    None
                } else {
                    Some(
                        value
                            .trim()
                            .parse()
                            .map_err(|_| anyhow!("ASR threads must be a whole number"))?,
                    )
                };
            }
            CampaignField::AsrHfToken => self.draft.asr.hf_token = optional_text(value),
            CampaignField::AsrEngine => self.draft.asr.engine = optional_text(value),
            _ => bail!("this campaign field is not edited as text"),
        }
        Ok(())
    }

    fn set_player_text(&mut self, field: PlayerField, value: String) -> Result<()> {
        let Some(player) = self.current_player_editor_mut() else {
            bail!("no player is being edited");
        };
        match field {
            PlayerField::Player => player.player = value,
            PlayerField::Character => player.character = value,
            PlayerField::Ancestry => player.ancestry = value,
            PlayerField::Class => player.class = value,
        }
        Ok(())
    }

    fn set_list_text(
        &mut self,
        target: ListTarget,
        index: Option<usize>,
        value: String,
    ) -> Result<()> {
        let value = value.trim().to_string();
        if value.is_empty() {
            bail!("list entries cannot be empty");
        }
        let values = self.list_values_mut(target);
        if let Some(index) = index {
            if let Some(existing) = values.get_mut(index) {
                *existing = value;
            } else {
                bail!("list entry no longer exists");
            }
        } else {
            values.push(value);
        }
        Ok(())
    }

    fn set_map_text(
        &mut self,
        target: MapTarget,
        index: Option<usize>,
        value: String,
    ) -> Result<()> {
        let Some((key, mapped)) = value.split_once('=') else {
            bail!("enter mappings as key = value");
        };
        let key = key.trim();
        let mapped = mapped.trim();
        if key.is_empty() || mapped.is_empty() {
            bail!("both mapping key and value are required");
        }
        let old_key = index.and_then(|index| {
            self.map_entries(target)
                .get(index)
                .map(|(key, _)| key.clone())
        });
        let map = self.map_mut(target);
        if let Some(old_key) = old_key {
            map.remove(&old_key);
        }
        map.insert(key.to_string(), mapped.to_string());
        Ok(())
    }

    fn open_player_editor(&mut self, index: Option<usize>) {
        let player = index
            .and_then(|index| self.draft.players.get(index).cloned())
            .unwrap_or_default();
        self.page = CampaignFormPage::PlayerEditor {
            index,
            cursor: 0,
            player,
        };
        self.scroll = 0;
        self.error = None;
    }

    fn finish_player(&mut self) {
        let page = std::mem::replace(&mut self.page, CampaignFormPage::Main);
        let CampaignFormPage::PlayerEditor {
            index,
            cursor,
            player,
        } = page
        else {
            self.page = page;
            return;
        };
        if player.player.trim().is_empty() || player.character.trim().is_empty() {
            self.page = CampaignFormPage::PlayerEditor {
                index,
                cursor,
                player,
            };
            self.error = Some("player and character names are required".into());
            return;
        }
        let cursor = match index {
            Some(index) => {
                self.draft.players[index] = player;
                index
            }
            None => {
                self.draft.players.push(player);
                self.draft.players.len() - 1
            }
        };
        self.page = CampaignFormPage::Players { cursor };
        self.scroll = 0;
        self.mark_dirty();
    }

    fn main_rows(&self) -> Vec<FormRow> {
        let mut rows = vec![
            FormRow::Section("Campaign"),
            FormRow::Field(CampaignField::Name),
            FormRow::Field(CampaignField::Gm),
            FormRow::Field(CampaignField::Setting),
            FormRow::Field(CampaignField::Notes),
            FormRow::Field(CampaignField::Players),
            FormRow::Section("System"),
            FormRow::Field(CampaignField::Preset),
            FormRow::Field(CampaignField::SystemOverrides),
            FormRow::Section("Outputs"),
        ];
        rows.extend(
            (0..ALL_ARTIFACTS.len()).map(|index| FormRow::Field(CampaignField::Artifact(index))),
        );
        rows.extend([
            FormRow::Section("Backend overrides - empty inherits global"),
            FormRow::Field(CampaignField::BackendKind),
            FormRow::Field(CampaignField::BackendBaseUrl),
            FormRow::Field(CampaignField::BackendApiKey),
            FormRow::Field(CampaignField::BackendModel),
            FormRow::Section("ASR overrides - empty inherits global"),
            FormRow::Field(CampaignField::AsrBinary),
            FormRow::Field(CampaignField::AsrModel),
            FormRow::Field(CampaignField::AsrModelDir),
            FormRow::Field(CampaignField::AsrThreads),
            FormRow::Field(CampaignField::AsrDiarize),
            FormRow::Field(CampaignField::AsrHfToken),
            FormRow::Field(CampaignField::AsrVad),
            FormRow::Field(CampaignField::AsrDevice),
            FormRow::Field(CampaignField::AsrEngine),
            FormRow::Section("Transcription"),
            FormRow::Field(CampaignField::Vocabulary),
            FormRow::Field(CampaignField::Replacements),
            FormRow::Field(CampaignField::VocabPrompt),
            FormRow::Field(CampaignField::Speakers),
            FormRow::Section("Prompt overrides"),
            FormRow::Field(CampaignField::PromptBullets),
            FormRow::Field(CampaignField::PromptDmNotes),
            FormRow::Field(CampaignField::PromptRecap),
            FormRow::Field(CampaignField::PromptSummary),
            FormRow::Field(CampaignField::PromptStory),
            FormRow::Field(CampaignField::PromptQuotes),
            FormRow::Field(CampaignField::Save),
        ]);
        rows
    }

    fn field_label(&self, field: CampaignField) -> String {
        match field {
            CampaignField::Name => "Name".into(),
            CampaignField::Gm => "GM".into(),
            CampaignField::Setting => "Setting".into(),
            CampaignField::Notes => "Notes".into(),
            CampaignField::Players => "Players".into(),
            CampaignField::Preset => "Preset".into(),
            CampaignField::SystemOverrides => "System overrides".into(),
            CampaignField::Artifact(index) => ALL_ARTIFACTS[index].label().into(),
            CampaignField::BackendKind => "Backend kind".into(),
            CampaignField::BackendBaseUrl => "Backend URL".into(),
            CampaignField::BackendApiKey => "Backend API key".into(),
            CampaignField::BackendModel => "Backend model".into(),
            CampaignField::AsrBinary => "ASR binary".into(),
            CampaignField::AsrModel => "ASR model".into(),
            CampaignField::AsrModelDir => "ASR model directory".into(),
            CampaignField::AsrThreads => "ASR threads".into(),
            CampaignField::AsrDiarize => "Speaker diarization".into(),
            CampaignField::AsrHfToken => "Hugging Face token".into(),
            CampaignField::AsrVad => "Voice activity detection".into(),
            CampaignField::AsrDevice => "ASR device".into(),
            CampaignField::AsrEngine => "ASR engine".into(),
            CampaignField::Vocabulary => "Vocabulary".into(),
            CampaignField::Replacements => "Replacements".into(),
            CampaignField::VocabPrompt => "Vocabulary prompt".into(),
            CampaignField::Speakers => "Speaker labels".into(),
            CampaignField::PromptBullets => "Bullets prompt".into(),
            CampaignField::PromptDmNotes => "DM notes prompt".into(),
            CampaignField::PromptRecap => "Recap prompt".into(),
            CampaignField::PromptSummary => "Summary prompt".into(),
            CampaignField::PromptStory => "Story prompt".into(),
            CampaignField::PromptQuotes => "Quotes prompt".into(),
            CampaignField::Save => "Save campaign".into(),
        }
    }

    fn field_value(&self, field: CampaignField) -> String {
        if let Some(value) = self.inline_field_value(field) {
            return value;
        }
        match field {
            CampaignField::Name => self.draft.campaign.name.clone(),
            CampaignField::Gm => empty_label(&self.draft.campaign.gm),
            CampaignField::Setting => empty_label(&self.draft.campaign.setting),
            CampaignField::Notes => long_text_label(&self.draft.campaign.notes, false),
            CampaignField::Players => format!("{} configured", self.draft.players.len()),
            CampaignField::Preset => self.draft.system.preset.clone(),
            CampaignField::SystemOverrides => long_text_label(&self.draft.system.overrides, false),
            CampaignField::Artifact(index) => {
                let enabled = self
                    .draft
                    .outputs
                    .default
                    .iter()
                    .any(|item| item == ALL_ARTIFACTS[index].id());
                if enabled { "enabled" } else { "disabled" }.into()
            }
            CampaignField::BackendKind => optional_label(self.draft.backend.kind.as_deref()),
            CampaignField::BackendBaseUrl => optional_label(self.draft.backend.base_url.as_deref()),
            CampaignField::BackendApiKey => secret_label(self.draft.backend.api_key.as_deref()),
            CampaignField::BackendModel => optional_label(self.draft.backend.model.as_deref()),
            CampaignField::AsrBinary => optional_path_label(self.draft.asr.binary.as_deref()),
            CampaignField::AsrModel => optional_label(self.draft.asr.model.as_deref()),
            CampaignField::AsrModelDir => optional_path_label(self.draft.asr.model_dir.as_deref()),
            CampaignField::AsrThreads => self
                .draft
                .asr
                .threads
                .map(|threads| threads.to_string())
                .unwrap_or_else(|| "inherit".into()),
            CampaignField::AsrDiarize => tristate_label(self.draft.asr.diarize),
            CampaignField::AsrHfToken => secret_label(self.draft.asr.hf_token.as_deref()),
            CampaignField::AsrVad => tristate_label(self.draft.asr.vad),
            CampaignField::AsrDevice => optional_label(self.draft.asr.device.as_deref()),
            CampaignField::AsrEngine => optional_label(self.draft.asr.engine.as_deref()),
            CampaignField::Vocabulary => {
                format!("{} terms", self.draft.transcription.vocabulary.len())
            }
            CampaignField::Replacements => {
                format!("{} mappings", self.draft.transcription.replacements.len())
            }
            CampaignField::VocabPrompt => if self.draft.transcription.vocab_prompt {
                "enabled"
            } else {
                "disabled"
            }
            .into(),
            CampaignField::Speakers => {
                format!("{} mappings", self.draft.transcription.speakers.len())
            }
            CampaignField::PromptBullets => {
                long_optional_label(self.draft.prompts.bullets.as_deref())
            }
            CampaignField::PromptDmNotes => {
                long_optional_label(self.draft.prompts.dm_notes.as_deref())
            }
            CampaignField::PromptRecap => long_optional_label(self.draft.prompts.recap.as_deref()),
            CampaignField::PromptSummary => {
                long_optional_label(self.draft.prompts.summary.as_deref())
            }
            CampaignField::PromptStory => long_optional_label(self.draft.prompts.story.as_deref()),
            CampaignField::PromptQuotes => {
                long_optional_label(self.draft.prompts.quotes.as_deref())
            }
            CampaignField::Save => "Ctrl+S".into(),
        }
    }

    fn field_edit_value(&self, field: CampaignField) -> Option<String> {
        let value = match field {
            CampaignField::Name => self.draft.campaign.name.clone(),
            CampaignField::Gm => self.draft.campaign.gm.clone(),
            CampaignField::Setting => self.draft.campaign.setting.clone(),
            CampaignField::BackendBaseUrl => {
                self.draft.backend.base_url.clone().unwrap_or_default()
            }
            CampaignField::BackendApiKey => self.draft.backend.api_key.clone().unwrap_or_default(),
            CampaignField::BackendModel => self.draft.backend.model.clone().unwrap_or_default(),
            CampaignField::AsrBinary => self
                .draft
                .asr
                .binary
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            CampaignField::AsrModel => self.draft.asr.model.clone().unwrap_or_default(),
            CampaignField::AsrModelDir => self
                .draft
                .asr
                .model_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            CampaignField::AsrThreads => self
                .draft
                .asr
                .threads
                .map(|threads| threads.to_string())
                .unwrap_or_default(),
            CampaignField::AsrHfToken => self.draft.asr.hf_token.clone().unwrap_or_default(),
            CampaignField::AsrEngine => self.draft.asr.engine.clone().unwrap_or_default(),
            _ => return None,
        };
        Some(value)
    }

    fn inline_field_value(&self, field: CampaignField) -> Option<String> {
        self.inline.as_ref().and_then(|edit| match edit.target {
            InlineTarget::Field(current) if current == field => Some(edit.input.with_cursor()),
            _ => None,
        })
    }

    fn inline_list_value(&self, target: ListTarget, index: usize) -> Option<String> {
        self.inline.as_ref().and_then(|edit| match edit.target {
            InlineTarget::List {
                target: current,
                index: Some(current_index),
            } if current == target && current_index == index => Some(edit.input.with_cursor()),
            InlineTarget::List {
                target: current,
                index: None,
            } if current == target && index == self.list_values(target).len() => {
                Some(edit.input.with_cursor())
            }
            _ => None,
        })
    }

    fn inline_map_value(&self, target: MapTarget, index: usize) -> Option<String> {
        self.inline.as_ref().and_then(|edit| match edit.target {
            InlineTarget::Map {
                target: current,
                index: Some(current_index),
            } if current == target && current_index == index => Some(edit.input.with_cursor()),
            InlineTarget::Map {
                target: current,
                index: None,
            } if current == target && index == self.map_entries(target).len() => {
                Some(edit.input.with_cursor())
            }
            _ => None,
        })
    }

    fn player_value(&self, player: &Player, field: PlayerField) -> String {
        self.inline
            .as_ref()
            .and_then(|edit| match edit.target {
                InlineTarget::Player(current) if current == field => Some(edit.input.with_cursor()),
                _ => None,
            })
            .unwrap_or_else(|| empty_label(player_field_value(player, field)))
    }

    fn current_player_editor(&self) -> Option<&Player> {
        match &self.page {
            CampaignFormPage::PlayerEditor { player, .. } => Some(player),
            _ => None,
        }
    }

    fn current_player_editor_mut(&mut self) -> Option<&mut Player> {
        match &mut self.page {
            CampaignFormPage::PlayerEditor { player, .. } => Some(player),
            _ => None,
        }
    }

    fn list_values(&self, target: ListTarget) -> &Vec<String> {
        match target {
            ListTarget::Vocabulary => &self.draft.transcription.vocabulary,
        }
    }

    fn list_values_mut(&mut self, target: ListTarget) -> &mut Vec<String> {
        match target {
            ListTarget::Vocabulary => &mut self.draft.transcription.vocabulary,
        }
    }

    fn map_entries(&self, target: MapTarget) -> Vec<(String, String)> {
        self.map_ref(target)
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    fn map_ref(&self, target: MapTarget) -> &std::collections::BTreeMap<String, String> {
        match target {
            MapTarget::Replacements => &self.draft.transcription.replacements,
            MapTarget::Speakers => &self.draft.transcription.speakers,
        }
    }

    fn map_mut(&mut self, target: MapTarget) -> &mut std::collections::BTreeMap<String, String> {
        match target {
            MapTarget::Replacements => &mut self.draft.transcription.replacements,
            MapTarget::Speakers => &mut self.draft.transcription.speakers,
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.error = None;
    }
}

fn player_field_at(cursor: usize) -> Option<PlayerField> {
    match cursor {
        0 => Some(PlayerField::Player),
        1 => Some(PlayerField::Character),
        2 => Some(PlayerField::Ancestry),
        3 => Some(PlayerField::Class),
        _ => None,
    }
}

fn player_field_value(player: &Player, field: PlayerField) -> &str {
    match field {
        PlayerField::Player => &player.player,
        PlayerField::Character => &player.character,
        PlayerField::Ancestry => &player.ancestry,
        PlayerField::Class => &player.class,
    }
}

fn player_summary(player: &Player) -> String {
    let detail = [player.ancestry.as_str(), player.class.as_str()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if detail.is_empty() {
        player.character.clone()
    } else {
        format!("{} ({detail})", player.character)
    }
}

fn optional_text(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn empty_label(value: &str) -> String {
    if value.is_empty() {
        "(empty)".into()
    } else {
        value.into()
    }
}

fn optional_label(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or("inherit")
        .into()
}

fn optional_path_label(value: Option<&Path>) -> String {
    value
        .map(|path| path.display().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "inherit".into())
}

fn secret_label(value: Option<&str>) -> String {
    match value.filter(|value| !value.is_empty()) {
        None => "inherit".into(),
        Some(value) => {
            let suffix: String = value
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("set ...{suffix}")
        }
    }
}

fn long_text_label(value: &str, optional: bool) -> String {
    if value.trim().is_empty() {
        if optional { "inherit" } else { "(empty)" }.into()
    } else {
        format!("{} lines - edit in $EDITOR", value.lines().count())
    }
}

fn long_optional_label(value: Option<&str>) -> String {
    long_text_label(value.unwrap_or_default(), true)
}

fn tristate_label(value: Option<bool>) -> String {
    match value {
        None => "inherit".into(),
        Some(true) => "enabled".into(),
        Some(false) => "disabled".into(),
    }
}

fn cycle_tristate(value: &mut Option<bool>, delta: i32) {
    let values = [None, Some(true), Some(false)];
    let current = values
        .iter()
        .position(|candidate| candidate == value)
        .unwrap_or(0) as i32;
    *value = values[(current + delta).rem_euclid(values.len() as i32) as usize];
}

fn cycle_optional_string(value: &mut Option<String>, choices: &[&str], delta: i32) {
    let current = value
        .as_ref()
        .and_then(|value| choices.iter().position(|choice| *choice == value))
        .map(|index| index as i32 + 1)
        .unwrap_or(0);
    let len = choices.len() as i32 + 1;
    let next = (current + delta).rem_euclid(len);
    *value = if next == 0 {
        None
    } else {
        Some(choices[(next - 1) as usize].to_string())
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presets() -> Vec<String> {
        vec!["dnd5e".into(), "generic".into()]
    }

    #[test]
    fn edit_round_trips_a_real_campaign_without_changes() {
        let config: CampaignConfig =
            toml::from_str(include_str!("../../campaigns/DnDThursday.toml")).unwrap();
        let form = CampaignFormState::edit(
            &config,
            PathBuf::from("campaigns/DnDThursday.toml"),
            presets(),
        );
        let applied = form.build_config().unwrap();
        assert_eq!(
            toml::to_string(&config).unwrap(),
            toml::to_string(&applied).unwrap()
        );
    }

    #[test]
    fn tri_state_cycles_through_inherit_enabled_and_disabled() {
        let mut value = None;
        cycle_tristate(&mut value, 1);
        assert_eq!(value, Some(true));
        cycle_tristate(&mut value, 1);
        assert_eq!(value, Some(false));
        cycle_tristate(&mut value, 1);
        assert_eq!(value, None);
    }

    #[test]
    fn empty_optional_text_returns_to_global_inheritance() {
        let mut form = CampaignFormState::create(presets());
        form.set_field_text(CampaignField::BackendModel, "local-model".into())
            .unwrap();
        form.set_field_text(CampaignField::BackendModel, "  ".into())
            .unwrap();
        assert_eq!(form.draft.backend.model, None);
    }

    #[test]
    fn invalid_threads_and_blank_names_are_rejected() {
        let mut form = CampaignFormState::create(presets());
        assert!(form
            .set_field_text(CampaignField::AsrThreads, "many".into())
            .is_err());
        form.draft.campaign.name = "   ".into();
        assert!(form.build_config().is_err());
    }

    #[test]
    fn keyboard_editor_updates_players_lists_and_mappings() {
        let mut form = CampaignFormState::create(presets());
        form.page = CampaignFormPage::Players { cursor: 0 };
        form.handle_key(KeyEvent::from(KeyCode::Char('a')));
        for (field, value) in [(0, "Mina"), (1, "Tamsin")]
            .into_iter()
            .map(|(field, value)| (field, value.to_string()))
        {
            if let CampaignFormPage::PlayerEditor { cursor, .. } = &mut form.page {
                *cursor = field;
            }
            form.handle_key(KeyEvent::from(KeyCode::Enter));
            for character in value.chars() {
                form.handle_key(KeyEvent::from(KeyCode::Char(character)));
            }
            form.handle_key(KeyEvent::from(KeyCode::Enter));
        }
        form.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(form.draft.players.len(), 1);
        assert_eq!(form.draft.players[0].player, "Mina");
        assert_eq!(form.draft.players[0].character, "Tamsin");

        form.page = CampaignFormPage::List {
            target: ListTarget::Vocabulary,
            cursor: 0,
        };
        form.handle_key(KeyEvent::from(KeyCode::Char('a')));
        for character in "Damasus".chars() {
            form.handle_key(KeyEvent::from(KeyCode::Char(character)));
        }
        form.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(form.draft.transcription.vocabulary, vec!["Damasus"]);

        form.page = CampaignFormPage::Map {
            target: MapTarget::Replacements,
            cursor: 0,
        };
        form.handle_key(KeyEvent::from(KeyCode::Char('a')));
        for character in "Mosses = Damasus".chars() {
            form.handle_key(KeyEvent::from(KeyCode::Char(character)));
        }
        form.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(
            form.draft.transcription.replacements.get("Mosses"),
            Some(&"Damasus".to_string())
        );
    }
}
