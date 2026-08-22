//! TUI application state and behaviour. Rendering lives in [`super::draw`];
//! background work lives in [`super::jobs`].

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::SystemTime;

use ratatui::layout::Rect;
use ratatui::widgets::ListState;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::audio::{self, AudioFile};
use crate::config::{CampaignConfig, GlobalConfig};
use crate::index::SearchHit;
use crate::presets::{self, Preset};
use crate::prompts::{Artifact, ALL_ARTIFACTS};
use crate::session::SessionInput;
use crate::ui::UiEvent;

use super::campaign_form::{CampaignField, CampaignFormAction, CampaignFormState};
use super::form::TextInput;
use super::jobs::{self, JobKind, JobRequest, ModelJob};
use super::theme::{self, Theme};

/// Which pane currently has keyboard focus.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Campaigns,
    Sessions,
    Audio,
    Content,
}

/// Severity for a line in the live job log.
#[derive(Clone, Copy)]
pub enum LogLevel {
    Info,
    Step,
    Ok,
    Warn,
    Error,
}

pub struct CampaignEntry {
    pub name: String,
    pub path: PathBuf,
}

pub struct SessionEntry {
    pub stem: String,
    pub transcript: PathBuf,
    /// Modification time of the transcript, used to show session age.
    pub modified: SystemTime,
    /// Existence of each artifact, aligned with [`ALL_ARTIFACTS`].
    pub artifacts: Vec<bool>,
}

/// A palette command.
#[derive(Clone, Copy)]
pub enum Action {
    NewCampaign,
    CampaignSettings,
    ForkCampaign,
    RunPipeline,
    Transcribe,
    GenerateNotes,
    OpenSession,
    OpenInEditor,
    Search,
    NextCampaign,
    CycleTheme,
    ManageModels,
    UpdateOllama,
    InstallCudaToolkit,
    RerunReplace,
    RerunKeepBoth,
    ToggleDiarize,
    MapSpeakers,
    RebuildLog,
    SystemCheck,
    Quit,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::NewCampaign => "New campaign — create in app",
            Action::CampaignSettings => "Edit campaign — settings, players, prompts",
            Action::ForkCampaign => "Fork campaign — copy config and content",
            Action::RunPipeline => "Run pipeline — transcribe + notes",
            Action::Transcribe => "Transcribe audio — audio → transcript",
            Action::GenerateNotes => "Generate notes — transcript → notes",
            Action::OpenSession => "Open session in viewer",
            Action::OpenInEditor => "Open current artifact in $EDITOR",
            Action::Search => "Search notes",
            Action::NextCampaign => "Switch campaign",
            Action::CycleTheme => "Change theme",
            Action::ManageModels => "Manage models — install / check / delete",
            Action::UpdateOllama => "Update Ollama — run the official installer",
            Action::InstallCudaToolkit => "Install CUDA toolkit — enable transcribe.cpp GPU",
            Action::RerunReplace => "Re-run session — regenerate & replace artifacts",
            Action::RerunKeepBoth => "Re-run session — keep both to compare",
            Action::ToggleDiarize => "Toggle speaker diarization (on/off)",
            Action::MapSpeakers => "Map speakers for the open session",
            Action::RebuildLog => "Rebuild campaign log",
            Action::SystemCheck => "System check (doctor)",
            Action::Quit => "Quit",
        }
    }
    pub fn all() -> &'static [Action] {
        &[
            Action::NewCampaign,
            Action::CampaignSettings,
            Action::ForkCampaign,
            Action::RunPipeline,
            Action::Transcribe,
            Action::GenerateNotes,
            Action::OpenSession,
            Action::OpenInEditor,
            Action::Search,
            Action::NextCampaign,
            Action::CycleTheme,
            Action::ManageModels,
            Action::UpdateOllama,
            Action::InstallCudaToolkit,
            Action::RerunReplace,
            Action::RerunKeepBoth,
            Action::ToggleDiarize,
            Action::MapSpeakers,
            Action::RebuildLog,
            Action::SystemCheck,
            Action::Quit,
        ]
    }
}

pub struct PaletteState {
    pub query: String,
    pub filtered: Vec<usize>,
    pub cursor: usize,
}

pub struct SearchState {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub hit_campaigns: Vec<usize>,
    pub all_campaigns: bool,
    pub cursor: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub enum PickerKind {
    AudioRun,
    AudioTranscribe,
    Artifacts,
    /// Artifact selection shown after choosing audio for a full run.
    RunArtifacts,
    /// Artifact selection for re-running the open session (replace in place).
    RerunReplace,
    /// Artifact selection for re-running the open session (keep both to compare).
    RerunKeepBoth,
}

pub struct PickerState {
    pub title: String,
    pub kind: PickerKind,
    pub items: Vec<String>,
    pub checked: Vec<bool>,
    pub cursor: usize,
    /// For the artifact picker: the transcript to generate notes for.
    pub target: Option<PathBuf>,
}

pub(super) enum TextPromptAction {
    ForkCampaign,
}

pub(super) struct TextPromptState {
    pub title: String,
    pub input: TextInput,
    pub error: Option<String>,
    pub action: TextPromptAction,
}

pub(super) enum Overlay {
    None,
    Help,
    Palette(PaletteState),
    Search(SearchState),
    Picker(PickerState),
    CampaignForm(CampaignFormState),
    TextPrompt(TextPromptState),
    SpeakerMap(SpeakerMapState),
    /// Theme chooser with live preview. `original` is restored on Esc.
    ThemePicker {
        cursor: usize,
        original: usize,
    },
    Message {
        title: String,
        body: String,
        error: bool,
    },
    /// A yes/no prompt. On confirm, `App::pending_confirm` drives the action.
    Confirm {
        title: String,
        body: String,
    },
}

pub struct SpeakerMapState {
    pub stem: String,
    pub labels: Vec<String>,
    pub samples: BTreeMap<String, Vec<String>>,
    pub map: BTreeMap<String, String>,
    pub choices: Vec<String>,
    pub cursor: usize,
    pub preview_samples: BTreeMap<String, crate::speakers::TimedSpeakerSample>,
    pub audio: Option<PathBuf>,
}

pub(super) enum ConfirmAction {
    Rerun(PathBuf, Vec<Artifact>, bool),
    Quit,
    DiscardCampaignForm,
    SaveCampaignRename,
}

pub(super) enum EditorReturn {
    Viewer,
    CampaignLongText(CampaignField),
}

struct PendingCampaignSave {
    form: CampaignFormState,
    config: CampaignConfig,
    path: PathBuf,
    old_root: PathBuf,
    new_root: PathBuf,
}

/// Which family a model row belongs to.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum ModelKind {
    #[default]
    Whisper,
    Ollama,
    /// A modern ASR engine run via the uv bridge (faster-whisper, Parakeet,
    /// Canary, Voxtral, Cohere). Selectable as the default; prepared lazily on first use.
    Asr,
}

#[derive(Default, Clone)]
pub struct ModelRow {
    /// A non-selectable section header when true.
    pub header: bool,
    /// A non-selectable column-labels row (a kind of header) when true.
    pub col_header: bool,
    /// An expandable model family (has multiple variants) when true.
    pub family: bool,
    /// Whether this family is currently expanded (for the caret).
    pub expanded: bool,
    /// Indentation level (0 = family/leaf, 1 = variant under a family).
    pub indent: u8,
    pub kind: ModelKind,
    /// The pull id (may include an `org/` prefix / `hf.co/` for community models).
    pub id: String,
    /// Name shown in the list.
    pub display: String,
    /// Number of variants (for family rows).
    pub variant_count: usize,
    /// Key used to toggle a family's expansion.
    pub expand_key: String,
    pub installed: bool,
    pub is_default: bool,
    pub size: u64,
    /// Approximate release date (`YYYY-MM`) or `"—"`.
    pub released: String,
}

pub struct ModelsState {
    pub rows: Vec<ModelRow>,
    pub cursor: usize,
    pub scroll: usize,
    /// Display names of expanded families.
    pub expanded: std::collections::HashSet<String>,
    /// Locally-installed Ollama models `pull_id -> size` (from `/api/tags`).
    pub installed: std::collections::HashMap<String, u64>,
}

impl ModelRow {
    /// Whether the cursor can land on this row (not a section/column header).
    pub fn selectable(&self) -> bool {
        !self.header && !self.col_header
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HitTarget {
    CampaignList,
    CampaignRow(usize),
    SessionList,
    SessionRow(usize),
    AudioList,
    AudioRow(usize),
    Viewer,
    ViewerScrollbar,
    Job,
    ModelList,
    ModelRow(usize),
    PlayerTrack,
    Footer(FooterCmd),
    ArtifactTab(usize),
    ModelButton { row: usize, install: bool },
    OverlayBarrier,
    OverlayDismiss,
    PaletteItem(usize),
    SearchItem(usize),
    PickerItem(usize),
    PickerConfirm,
    CampaignFormRow(usize),
    TextPromptInput,
    TextPromptSubmit,
    SpeakerRow(usize),
    SpeakerPreview,
    SpeakerSave,
    ThemeRow(usize),
    ThemeApply,
    ConfirmYes,
    ConfirmNo,
    ConfirmCancel,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct HitRegion {
    pub rect: Rect,
    pub target: HitTarget,
}

#[derive(Default, Clone, Debug)]
pub(super) struct HitRegions {
    regions: Vec<HitRegion>,
}

impl HitRegions {
    pub fn clear(&mut self) {
        self.regions.clear();
    }

    pub fn push(&mut self, rect: Rect, target: HitTarget) {
        if rect.width > 0 && rect.height > 0 {
            self.regions.push(HitRegion { rect, target });
        }
    }

    pub fn target_at(&self, col: u16, row: u16) -> Option<HitTarget> {
        self.regions
            .iter()
            .rev()
            .find(|region| rect_contains(region.rect, col, row))
            .map(|region| region.target)
    }

    pub fn rect_for(&self, target: HitTarget) -> Option<Rect> {
        self.regions
            .iter()
            .rev()
            .find(|region| region.target == target)
            .map(|region| region.rect)
    }
}

fn rect_contains(rect: Rect, col: u16, row: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && col >= rect.x
        && col < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

/// A clickable footer shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FooterCmd {
    Palette,
    Search,
    Help,
    Quit,
    NewCampaign,
    EditCampaign,
    ForkCampaign,
    Editor,
    Run,
    Transcribe,
    Notes,
    Copy,
    Select,
}

pub struct App {
    pub handle: tokio::runtime::Handle,

    pub themes: Vec<Theme>,
    pub theme_idx: usize,

    pub should_quit: bool,
    pub pending_editor: Option<PathBuf>,
    editor_return: EditorReturn,
    /// A shell command to run with the TUI suspended (e.g. the Ollama updater):
    /// `(title, command)`.
    pub pending_shell: Option<(String, String)>,
    /// Reload the selected campaign after an external interactive action exits.
    pub pending_data_reload: bool,
    /// Active audio player (quote playback / audio scrubbing), if any.
    pub player: Option<super::player::Player>,

    /// Action awaiting confirmation while `Overlay::Confirm` is open.
    pub(super) pending_confirm: Option<ConfirmAction>,
    pending_campaign_form: Option<CampaignFormState>,
    pending_campaign_save: Option<PendingCampaignSave>,

    pub campaigns: Vec<CampaignEntry>,
    pub campaign_idx: usize,
    pub campaign: Option<CampaignConfig>,
    pub preset: Option<Preset>,
    pub preset_name: String,
    pub global: GlobalConfig,
    /// Cached ASR labels for the header; refreshed when campaign/config changes.
    asr_model_label: String,
    asr_device_label: String,

    pub audio: Vec<AudioFile>,
    pub audio_idx: usize,
    /// Results from the nonblocking sidebar duration probes.
    audio_probe_rx: Option<std::sync::mpsc::Receiver<(usize, usize, Option<f64>)>>,
    audio_probe_generation: usize,

    pub sessions: Vec<SessionEntry>,
    pub session_idx: usize,

    pub pane: Pane,

    /// True when the synthetic "Campaign Log" row is highlighted in Sessions.
    pub log_selected: bool,

    pub open_session: Option<usize>,
    /// When true, the viewer shows the `.candidate` version of the current
    /// artifact (from a keep-both re-run) instead of the kept one.
    pub viewing_candidate: bool,
    /// True when the viewer is showing the rolling campaign log.
    pub viewing_log: bool,
    pub artifact_tab: usize,
    /// Audio selected for a run, held while the artifact picker is shown.
    pub pending_run_sessions: Vec<SessionInput>,
    pub viewer_lines: Vec<String>,
    pub viewer_scroll: u16,
    /// Selected quote index in the Quotes tab (for navigation + playback).
    pub quote_idx: usize,

    pub job_running: bool,
    pub job_title: String,
    pub job_log: Vec<(LogLevel, String)>,
    pub job_rx: Option<UnboundedReceiver<UiEvent>>,
    /// Scroll offset (in wrapped rows) into the job log pane.
    pub job_scroll: u16,
    /// When true, the job pane stays pinned to the newest output.
    pub job_follow: bool,
    /// Latest progress update `(label, pos, total, rate)`; `total == 0` = indeterminate.
    pub job_progress: Option<(String, u64, u64, Option<f64>)>,
    /// Ordered high-level pipeline phases seen during the current job (for the
    /// animated stage timeline). The last entry is the active phase.
    pub job_stages: Vec<(String, std::time::Instant)>,
    /// When the current job started, for the elapsed-time readout.
    pub job_started: Option<std::time::Instant>,
    /// Frozen elapsed time of the last-finished job (so the readout stops
    /// counting once the job is done).
    pub job_elapsed: Option<std::time::Duration>,
    /// Completion anchor used to freeze stage durations after a job ends.
    pub job_finished_at: Option<std::time::Instant>,
    /// Animation frame counter (advances each draw) for the working spinner.
    pub tick: u64,
    /// Pending model jobs waiting for the current one to finish.
    pub job_queue: VecDeque<(ModelJob, String)>,
    /// When set, the content area shows the interactive model manager.
    pub models: Option<ModelsState>,
    /// Receives locally-installed Ollama models `(name, size)` to enrich the list.
    pub model_refresh_rx: Option<UnboundedReceiver<Vec<(String, u64)>>>,

    pub(super) hover: Option<(u16, u16)>,
    pub(super) player_dragging: bool,
    pub(super) viewer_dragging: bool,
    /// Whether mouse capture is currently on. Toggled for native text
    /// selection ("select mode").
    pub mouse_enabled: bool,
    /// Set when the event loop should toggle terminal mouse capture.
    pub mouse_toggle_pending: bool,
    /// Text queued to be copied to the system clipboard (via OSC 52).
    pub copy_pending: Option<String>,

    pub(super) overlay: Overlay,
    pub status: String,

    pub camp_state: ListState,
    pub sess_state: ListState,
    pub audio_state: ListState,
    pub overlay_state: ListState,

    pub(super) hit_regions: HitRegions,
}

impl App {
    pub fn request_quit(&mut self) {
        if self.job_running {
            self.pending_confirm = Some(ConfirmAction::Quit);
            self.overlay = Overlay::Confirm {
                title: "Quit?".into(),
                body: "A job is running and will be cancelled.".into(),
            };
        } else {
            self.should_quit = true;
        }
    }

    pub fn new(handle: tokio::runtime::Handle) -> Self {
        let global = GlobalConfig::load_or_default().unwrap_or_default();
        let (themes, theme_idx) = theme::resolve(&global.ui.theme);
        let mouse_enabled = global.ui.mouse && std::env::var_os("SESSIONSMITH_NO_MOUSE").is_none();

        let mut app = Self {
            handle,
            themes,
            theme_idx,
            should_quit: false,
            pending_editor: None,
            editor_return: EditorReturn::Viewer,
            pending_shell: None,
            pending_data_reload: false,
            player: None,
            pending_confirm: None,
            pending_campaign_form: None,
            pending_campaign_save: None,
            campaigns: Vec::new(),
            campaign_idx: 0,
            campaign: None,
            preset: None,
            preset_name: "—".into(),
            global,
            asr_model_label: String::new(),
            asr_device_label: String::new(),
            audio: Vec::new(),
            audio_idx: 0,
            audio_probe_rx: None,
            audio_probe_generation: 0,
            sessions: Vec::new(),
            session_idx: 0,
            pane: Pane::Campaigns,
            log_selected: false,
            open_session: None,
            viewing_candidate: false,
            viewing_log: false,
            artifact_tab: 0,
            pending_run_sessions: Vec::new(),
            viewer_lines: Vec::new(),
            viewer_scroll: 0,
            quote_idx: 0,
            job_running: false,
            job_title: String::new(),
            job_log: Vec::new(),
            job_rx: None,
            job_scroll: 0,
            job_follow: true,
            job_progress: None,
            job_stages: Vec::new(),
            job_started: None,
            job_elapsed: None,
            job_finished_at: None,
            tick: 0,
            job_queue: VecDeque::new(),
            models: None,
            model_refresh_rx: None,
            hover: None,
            player_dragging: false,
            viewer_dragging: false,
            mouse_enabled,
            mouse_toggle_pending: false,
            copy_pending: None,
            overlay: Overlay::None,
            status: "Welcome to SessionSmith. Press : for the command palette, ? for help.".into(),
            camp_state: ListState::default(),
            sess_state: ListState::default(),
            audio_state: ListState::default(),
            overlay_state: ListState::default(),
            hit_regions: HitRegions::default(),
        };
        app.load_campaigns();
        app.load_campaign_data();
        app
    }

    pub fn theme(&self) -> &Theme {
        &self.themes[self.theme_idx]
    }

    pub(super) fn begin_frame(&mut self) {
        self.hit_regions.clear();
    }

    pub(super) fn register_hit(&mut self, rect: Rect, target: HitTarget) {
        self.hit_regions.push(rect, target);
    }

    pub(super) fn hit_target_at(&self, col: u16, row: u16) -> Option<HitTarget> {
        self.hit_regions.target_at(col, row)
    }

    pub(super) fn hit_rect(&self, target: HitTarget) -> Option<Rect> {
        self.hit_regions.rect_for(target)
    }

    pub(super) fn set_hover(&mut self, col: u16, row: u16) {
        self.hover = Some((col, row));
    }

    pub(super) fn clear_hover(&mut self) {
        self.hover = None;
    }

    pub(super) fn is_hovering(&self, target: HitTarget) -> bool {
        self.hover
            .and_then(|(col, row)| self.hit_target_at(col, row))
            .is_some_and(|hovered| hovered == target)
    }

    // ---- data loading ----------------------------------------------------

    fn load_campaigns(&mut self) {
        let mut entries = Vec::new();
        let dir = PathBuf::from("campaigns");
        if dir.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                let mut paths: Vec<PathBuf> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().and_then(|e| e.to_str()) == Some("toml")
                            && !p
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| n.starts_with('.'))
                                .unwrap_or(true)
                    })
                    .collect();
                paths.sort();
                for p in paths {
                    let name = CampaignConfig::load(&p)
                        .map(|c| c.campaign.name)
                        .unwrap_or_else(|_| {
                            p.file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string()
                        });
                    entries.push(CampaignEntry { name, path: p });
                }
            }
        }
        // Backward compat: root campaign.toml.
        let root = PathBuf::from("campaign.toml");
        if entries.is_empty() && root.exists() {
            if let Ok(c) = CampaignConfig::load(&root) {
                entries.push(CampaignEntry {
                    name: c.campaign.name,
                    path: root,
                });
            }
        }
        // Apply the user's saved ordering (by file stem); unlisted campaigns
        // keep their alphabetical position after the ordered ones.
        let order = &self.global.ui.campaign_order;
        if !order.is_empty() {
            entries.sort_by_key(|e| {
                let stem = campaign_stem(&e.path);
                order.iter().position(|o| *o == stem).unwrap_or(usize::MAX)
            });
        }
        self.campaigns = entries;
        if self.campaign_idx >= self.campaigns.len() {
            self.campaign_idx = 0;
        }
        self.camp_state.select(if self.campaigns.is_empty() {
            None
        } else {
            Some(self.campaign_idx)
        });
    }

    fn campaign_presets(&self) -> Vec<String> {
        let mut presets: Vec<String> = std::fs::read_dir("presets")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension().and_then(|extension| extension.to_str()) == Some("toml"))
                    .then(|| path.file_stem()?.to_str().map(str::to_string))
                    .flatten()
            })
            .collect();
        presets.sort();
        presets.dedup();
        presets
    }

    fn can_mutate_campaigns(&mut self) -> bool {
        if self.job_running {
            self.message(
                "Campaign busy",
                "Wait for the active job to finish before changing campaign files or output.",
                true,
            );
            false
        } else {
            true
        }
    }

    pub(super) fn open_new_campaign_form(&mut self) {
        if !self.can_mutate_campaigns() {
            return;
        }
        self.overlay = Overlay::CampaignForm(CampaignFormState::create(self.campaign_presets()));
        self.overlay_state.select(Some(0));
    }

    pub(super) fn open_campaign_editor(&mut self) {
        if !self.can_mutate_campaigns() {
            return;
        }
        let Some(path) = self
            .campaigns
            .get(self.campaign_idx)
            .map(|entry| entry.path.clone())
        else {
            self.message("No campaign", "Select a campaign before editing it.", true);
            return;
        };
        let Some(campaign) = self.campaign.as_ref() else {
            self.message(
                "Could not load campaign",
                "The selected campaign configuration is invalid.",
                true,
            );
            return;
        };
        self.overlay = Overlay::CampaignForm(CampaignFormState::edit(
            campaign,
            path,
            self.campaign_presets(),
        ));
        self.overlay_state.select(Some(0));
    }

    pub(super) fn request_fork_campaign(&mut self) {
        if !self.can_mutate_campaigns() {
            return;
        }
        let Some(campaign) = self.campaign.as_ref() else {
            self.message("No campaign", "Select a campaign before forking it.", true);
            return;
        };
        self.overlay = Overlay::TextPrompt(TextPromptState {
            title: "Fork campaign - new name".into(),
            input: TextInput::new(format!("{} fork", campaign.campaign.name)),
            error: None,
            action: TextPromptAction::ForkCampaign,
        });
    }

    pub(super) fn handle_campaign_form_action(&mut self, action: CampaignFormAction) {
        match action {
            CampaignFormAction::None => {}
            CampaignFormAction::Close => self.overlay = Overlay::None,
            CampaignFormAction::RequestDiscard => self.confirm_discard_campaign_form(),
            CampaignFormAction::OpenLongText(field) => self.open_campaign_long_text_editor(field),
            CampaignFormAction::Save => self.save_campaign_form(),
        }
    }

    fn confirm_discard_campaign_form(&mut self) {
        let overlay = std::mem::replace(&mut self.overlay, Overlay::None);
        let Overlay::CampaignForm(form) = overlay else {
            self.overlay = overlay;
            return;
        };
        self.pending_campaign_form = Some(form);
        self.pending_confirm = Some(ConfirmAction::DiscardCampaignForm);
        self.overlay = Overlay::Confirm {
            title: "Discard campaign changes?".into(),
            body: "Unsaved campaign edits will be lost.".into(),
        };
    }

    fn open_campaign_long_text_editor(&mut self, field: CampaignField) {
        let content = match &self.overlay {
            Overlay::CampaignForm(form) => form.take_long_text(field),
            _ => None,
        };
        let Some(content) = content else {
            return;
        };
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "sessionsmith-campaign-{}-{unique}.txt",
            std::process::id()
        ));
        if let Err(error) = std::fs::write(&path, content) {
            self.message(
                "Could not open editor",
                &format!("could not prepare temporary campaign field: {error}"),
                true,
            );
            return;
        }
        self.editor_return = EditorReturn::CampaignLongText(field);
        self.pending_editor = Some(path);
    }

    pub(super) fn finish_editor(&mut self, path: &std::path::Path) {
        match std::mem::replace(&mut self.editor_return, EditorReturn::Viewer) {
            EditorReturn::Viewer => {
                let scroll = self.viewer_scroll;
                self.refresh_viewer();
                self.viewer_scroll = scroll.min(
                    self.viewer_lines
                        .len()
                        .saturating_sub(1)
                        .min(u16::MAX as usize) as u16,
                );
            }
            EditorReturn::CampaignLongText(field) => {
                let result = std::fs::read_to_string(path);
                let _ = std::fs::remove_file(path);
                match result {
                    Ok(value) => {
                        if let Overlay::CampaignForm(form) = &mut self.overlay {
                            form.apply_long_text(field, value);
                        }
                    }
                    Err(error) => self.message(
                        "Could not read editor changes",
                        &format!("could not read temporary campaign field: {error}"),
                        true,
                    ),
                }
            }
        }
    }

    pub(super) fn submit_text_prompt(&mut self) {
        let overlay = std::mem::replace(&mut self.overlay, Overlay::None);
        let Overlay::TextPrompt(mut prompt) = overlay else {
            self.overlay = overlay;
            return;
        };
        match prompt.action {
            TextPromptAction::ForkCampaign => {
                let Some(source) = self.campaign.clone() else {
                    self.message("No campaign", "Select a campaign before forking it.", true);
                    return;
                };
                let outcome = crate::campaign_ops::fork_campaign(
                    std::path::Path::new("campaigns"),
                    &crate::config::output_dir(),
                    &source,
                    &prompt.input.value,
                );
                match outcome {
                    Ok(outcome) => {
                        let index_result = CampaignConfig::load(&outcome.config_path)
                            .and_then(|campaign| crate::campaign_ops::reindex_campaign(&campaign));
                        self.reload_select_campaign(&outcome.config_path);
                        self.status = match index_result {
                            Ok(indexed) => format!(
                                "Forked {} - {} files copied, {indexed} sessions indexed",
                                prompt.input.value.trim(),
                                outcome.files_copied
                            ),
                            Err(error) => format!(
                                "Forked {} - {} files copied; search index needs rebuild: {error}",
                                prompt.input.value.trim(),
                                outcome.files_copied
                            ),
                        };
                    }
                    Err(error) => {
                        prompt.error = Some(error.to_string());
                        self.overlay = Overlay::TextPrompt(prompt);
                    }
                }
            }
        }
    }

    fn save_campaign_form(&mut self) {
        let overlay = std::mem::replace(&mut self.overlay, Overlay::None);
        let Overlay::CampaignForm(mut form) = overlay else {
            self.overlay = overlay;
            return;
        };
        let config = match form.build_config() {
            Ok(config) => config,
            Err(error) => {
                form.set_error(error.to_string());
                self.overlay = Overlay::CampaignForm(form);
                return;
            }
        };

        let Some(path) = form.edit_path().map(std::path::Path::to_path_buf) else {
            match crate::campaign_ops::create_campaign(
                std::path::Path::new("campaigns"),
                &crate::config::output_dir(),
                &config,
            ) {
                Ok(path) => {
                    self.reload_select_campaign(&path);
                    self.status = format!("Campaign created: {}", config.campaign.name);
                }
                Err(error) => {
                    form.set_error(error.to_string());
                    self.overlay = Overlay::CampaignForm(form);
                }
            }
            return;
        };

        let old_root = form.original.output_root();
        let new_root = config.output_root();
        if form.changed_slug(&config) && new_root.exists() && old_root != new_root {
            form.set_error(format!(
                "output already exists at {}; choose another name or move it first",
                new_root.display()
            ));
            self.overlay = Overlay::CampaignForm(form);
            return;
        }
        if form.changed_slug(&config) && old_root.exists() {
            self.pending_campaign_save = Some(PendingCampaignSave {
                form,
                config,
                path,
                old_root: old_root.clone(),
                new_root: new_root.clone(),
            });
            self.pending_confirm = Some(ConfirmAction::SaveCampaignRename);
            self.overlay = Overlay::Confirm {
                title: "Move campaign output?".into(),
                body: format!(
                    "Renaming this campaign moves:\n{}\nto:\n{}",
                    old_root.display(),
                    new_root.display()
                ),
            };
            return;
        }
        self.commit_campaign_edit(form, config, path, None);
    }

    pub(super) fn commit_pending_campaign_rename(&mut self) {
        let Some(pending) = self.pending_campaign_save.take() else {
            return;
        };
        self.commit_campaign_edit(
            pending.form,
            pending.config,
            pending.path,
            Some((pending.old_root, pending.new_root)),
        );
    }

    fn commit_campaign_edit(
        &mut self,
        mut form: CampaignFormState,
        config: CampaignConfig,
        path: PathBuf,
        output_move: Option<(PathBuf, PathBuf)>,
    ) {
        let mut moved = None;
        if let Some((old_root, new_root)) = output_move {
            if let Err(error) = crate::campaign_ops::migrate_output_root(&old_root, &new_root) {
                form.set_error(error.to_string());
                self.overlay = Overlay::CampaignForm(form);
                return;
            }
            moved = Some((old_root, new_root));
        }
        if let Err(error) = config.save(&path) {
            if let Some((old_root, new_root)) = moved {
                let _ = crate::campaign_ops::migrate_output_root(&new_root, &old_root);
            }
            form.set_error(format!("could not save campaign: {error}"));
            self.overlay = Overlay::CampaignForm(form);
            return;
        }
        let index_result = crate::campaign_ops::reindex_campaign(&config);
        self.reload_select_campaign(&path);
        self.status = match index_result {
            Ok(indexed) => format!("Campaign saved - {indexed} sessions indexed"),
            Err(error) => format!("Campaign saved; search index needs rebuild: {error}"),
        };
    }

    fn reload_select_campaign(&mut self, path: &std::path::Path) {
        self.load_campaigns();
        if let Some(index) = self.campaigns.iter().position(|entry| entry.path == path) {
            self.campaign_idx = index;
            self.camp_state.select(Some(index));
        }
        self.load_campaign_data();
    }

    pub(super) fn restore_pending_campaign_form(&mut self) {
        if let Some(form) = self.pending_campaign_form.take() {
            self.overlay = Overlay::CampaignForm(form);
        }
    }

    pub(super) fn restore_pending_campaign_save(&mut self) {
        if let Some(pending) = self.pending_campaign_save.take() {
            self.overlay = Overlay::CampaignForm(pending.form);
        }
    }

    pub(super) fn discard_pending_campaign_form(&mut self) {
        self.pending_campaign_form = None;
    }

    pub(super) fn request_speaker_mapping(&mut self) {
        let Some(session) = self.open_session.and_then(|index| self.sessions.get(index)) else {
            self.message(
                "No session",
                "Open a diarized session before mapping speakers.",
                true,
            );
            return;
        };
        let Some(campaign) = self.campaign.as_ref() else {
            self.message(
                "No campaign",
                "Select a campaign before mapping speakers.",
                true,
            );
            return;
        };
        let raw_txt = campaign
            .transcripts_dir()
            .join(format!("{}.diarized.txt", session.stem));
        let txt = campaign
            .transcripts_dir()
            .join(format!("{}.txt", session.stem));
        let source = if raw_txt.exists() { raw_txt } else { txt };
        let Ok(text) = std::fs::read_to_string(&source) else {
            self.message(
                "No speaker labels",
                "Could not read this session's transcript.",
                true,
            );
            return;
        };
        let labels = crate::speakers::labels(&text);
        if labels.is_empty() {
            self.message(
                "No speaker labels",
                "This session has no diarized speaker labels to map.",
                true,
            );
            return;
        }
        let mut map = crate::meta::load(&campaign.transcripts_dir(), &session.stem)
            .and_then(|meta| meta.speaker_map)
            .unwrap_or_else(|| campaign.transcription.speakers.clone());
        map.retain(|label, _| labels.contains(label));
        let mut choices = roster_names(campaign);
        choices.push("Skip".into());
        let raw_srt = campaign
            .transcripts_dir()
            .join(format!("{}.diarized.srt", session.stem));
        let srt = if raw_srt.exists() {
            raw_srt
        } else {
            campaign
                .transcripts_dir()
                .join(format!("{}.srt", session.stem))
        };
        let timed_samples = std::fs::read_to_string(srt)
            .map(|srt| crate::speakers::detect_timed_samples(&srt))
            .unwrap_or_default();
        let mut samples = BTreeMap::new();
        let mut preview_samples = BTreeMap::new();
        for sample in timed_samples {
            let label = sample.label.clone();
            samples
                .entry(label.clone())
                .or_insert_with(Vec::new)
                .push(sample.text.clone());
            preview_samples.entry(label).or_insert(sample);
        }
        for sample in crate::speakers::detect_samples(&text) {
            let entries = samples.entry(sample.label).or_insert_with(Vec::new);
            if !entries.contains(&sample.text) {
                entries.push(sample.text);
            }
        }
        self.overlay = Overlay::SpeakerMap(SpeakerMapState {
            stem: session.stem.clone(),
            labels,
            samples,
            map,
            choices,
            cursor: 0,
            preview_samples,
            audio: self.session_source_audio(),
        });
    }

    /// Move the selected campaign up (`-1`) or down (`+1`) in the sidebar and
    /// persist the new order to the global config.
    pub(super) fn move_campaign(&mut self, delta: i32) {
        let len = self.campaigns.len();
        if len < 2 {
            return;
        }
        let from = self.campaign_idx;
        let to = from as i32 + delta;
        if to < 0 || to >= len as i32 {
            return;
        }
        let to = to as usize;
        self.campaigns.swap(from, to);
        self.campaign_idx = to;
        self.camp_state.select(Some(to));
        // Persist the full order by file stem.
        self.global.ui.campaign_order = self
            .campaigns
            .iter()
            .map(|c| campaign_stem(&c.path))
            .collect();
        self.global.save().ok();
        self.status = format!(
            "Moved '{}' — order saved",
            self.campaigns
                .get(to)
                .map(|c| c.name.as_str())
                .unwrap_or("")
        );
    }

    /// Load the selected campaign's config, preset, audio and sessions.
    pub fn load_campaign_data(&mut self) {
        self.campaign = None;
        self.preset = None;
        self.preset_name = "—".into();
        self.audio.clear();
        self.sessions.clear();
        self.open_session = None;
        self.viewing_log = false;
        self.log_selected = false;
        self.viewer_lines.clear();

        let Some(entry) = self.campaigns.get(self.campaign_idx) else {
            return;
        };
        let Ok(cfg) = CampaignConfig::load(&entry.path) else {
            return;
        };
        if let Ok(p) = presets::load(&cfg.system.preset) {
            self.preset_name = p.name.clone();
            self.preset = Some(p);
        }

        // Audio (newest-first). Duration probing is skipped to keep startup fast.
        let tx_dir = cfg.transcripts_dir();
        if let Ok(files) = audio::scan(&crate::config::audio_dir(), &tx_dir) {
            self.audio = files;
        }
        self.start_audio_duration_probes();
        self.audio_idx = 0;
        self.audio_state
            .select(if self.audio.is_empty() { None } else { Some(0) });

        // A session exists if it has a transcript OR a notes/<stem>/ directory,
        // so deleting transcripts doesn't hide sessions whose notes remain.
        let notes_dir = cfg.notes_dir();
        let mut stems: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        if let Ok(rd) = std::fs::read_dir(&tx_dir) {
            for p in rd.flatten().map(|e| e.path()) {
                if p.extension().and_then(|e| e.to_str()) == Some("txt") {
                    if let Some(s) = p.file_stem().and_then(|s| s.to_str()) {
                        stems.insert(s.to_string());
                    }
                }
            }
        }
        if let Ok(rd) = std::fs::read_dir(&notes_dir) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    if let Some(s) = e.file_name().to_str() {
                        // Skip reserved entries like `_campaign-log.*`.
                        if !s.starts_with('_') && !s.starts_with('.') {
                            stems.insert(s.to_string());
                        }
                    }
                }
            }
        }
        let mut sessions: Vec<SessionEntry> = stems
            .into_iter()
            .map(|stem| {
                let transcript = tx_dir.join(format!("{stem}.txt"));
                let nd = notes_dir.join(&stem);
                let artifacts: Vec<bool> = ALL_ARTIFACTS
                    .iter()
                    .map(|a| nd.join(a.filename()).exists())
                    .collect();
                // Newest mtime among the transcript and any artifact.
                let mut modified = std::fs::metadata(&transcript)
                    .and_then(|m| m.modified())
                    .ok();
                for a in ALL_ARTIFACTS {
                    if let Ok(m) =
                        std::fs::metadata(nd.join(a.filename())).and_then(|m| m.modified())
                    {
                        modified = Some(modified.map_or(m, |cur| cur.max(m)));
                    }
                }
                let modified = modified.unwrap_or(SystemTime::UNIX_EPOCH);
                SessionEntry {
                    stem,
                    transcript,
                    modified,
                    artifacts,
                }
            })
            .collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
        self.sessions = sessions;
        self.session_idx = 0;
        // Row 0 is the synthetic "Campaign Log"; default to the latest session.
        if self.sessions.is_empty() {
            self.log_selected = true;
            self.sess_state.select(Some(0));
        } else {
            self.log_selected = false;
            self.sess_state.select(Some(1));
        }

        self.campaign = Some(cfg);
        self.refresh_asr_labels();
    }

    fn refresh_asr_labels(&mut self) {
        let effective = self
            .campaign
            .as_ref()
            .map(|campaign| crate::config::effective(&self.global, campaign))
            .unwrap_or_else(|| self.global.clone());
        let hardware = crate::hardware::detect();
        self.asr_model_label = effective.asr.model.clone().unwrap_or_else(|| {
            crate::hardware::recommend(&hardware)
                .whisper_model
                .to_string()
        });
        self.asr_device_label = asr_device_label(
            effective.asr.device.as_deref(),
            hardware.gpu.as_ref().map(|gpu| gpu.vendor.as_str()),
        );
    }

    fn asr_model(&self) -> String {
        self.asr_model_label.clone()
    }

    pub fn asr_model_label(&self) -> String {
        self.asr_model()
    }

    pub fn asr_runtime_label(&self) -> String {
        format!(
            "{} · {}",
            crate::asr::engine_of(&self.asr_model_label).label(),
            self.asr_device_label
        )
    }

    pub fn asr_diarize(&self) -> bool {
        self.campaign
            .as_ref()
            .map(|campaign| crate::config::effective(&self.global, campaign).asr.diarize)
            .unwrap_or(self.global.asr.diarize)
    }

    /// If re-running `stem` could change the transcript — i.e. the source audio
    /// is still available *and* the current ASR model differs from the one that
    /// produced the existing transcript — return `(old_model, new_model)` so the
    /// caller can prompt the user. Returns `None` when re-transcribing isn't
    /// possible or wouldn't change anything.
    pub(super) fn rerun_model_change(&self, stem: &str) -> Option<(String, String)> {
        let cfg = self.campaign.as_ref()?;
        let meta = crate::meta::load(&cfg.transcripts_dir(), stem);
        let has_audio = meta
            .as_ref()
            .and_then(|m| m.source_audio.clone())
            .map(|p| p.exists())
            .unwrap_or(false)
            || find_audio_by_stem(&crate::config::audio_dir(), stem).is_some();
        if !has_audio {
            return None;
        }
        let new_model = self.asr_model();
        let old_model = meta.map(|m| m.model).filter(|s| !s.is_empty());
        if old_model.as_deref() == Some(new_model.as_str()) {
            return None;
        }
        Some((old_model.unwrap_or_else(|| "unknown".into()), new_model))
    }

    pub fn backend_summary(&self) -> String {
        let model = self
            .global
            .backend
            .model
            .clone()
            .unwrap_or_else(|| "(not set)".into());
        format!("{} / {}", self.global.backend.kind, model)
    }

    // ---- viewer ----------------------------------------------------------

    pub fn refresh_viewer(&mut self) {
        self.viewer_lines.clear();
        self.viewer_scroll = 0;
        self.quote_idx = 0;

        // Campaign log view.
        if self.viewing_log {
            let Some(cfg) = &self.campaign else { return };
            let path = cfg.notes_dir().join("_campaign-log.md");
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    // Hide any legacy machine markers from the reader (files
                    // written before the clean-render change).
                    self.viewer_lines = text
                        .lines()
                        .filter(|l| !l.trim_start().starts_with("<!-- ss:"))
                        .map(|l| l.to_string())
                        .collect();
                    if self.viewer_lines.is_empty() {
                        self.viewer_lines.push("(empty)".into());
                    }
                }
                Err(_) => {
                    self.viewer_lines =
                        vec!["No campaign log yet — generate notes or run `log rebuild`.".into()];
                }
            }
            return;
        }

        let Some(si) = self.open_session else { return };
        let Some(sess) = self.sessions.get(si) else {
            return;
        };
        let Some(cfg) = &self.campaign else { return };
        let art = ALL_ARTIFACTS[self.artifact_tab];
        let base = cfg.notes_dir().join(&sess.stem);
        // Show the candidate version when toggled and one exists.
        let path = if self.viewing_candidate {
            let c = base.join(crate::pipeline::artifact_file(art, true));
            if c.exists() {
                c
            } else {
                base.join(art.filename())
            }
        } else {
            base.join(art.filename())
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.viewer_lines = text.lines().map(|l| l.to_string()).collect();
                if self.viewer_lines.is_empty() {
                    self.viewer_lines.push("(empty)".into());
                }
            }
            Err(_) => {
                self.viewer_lines = vec![format!("Not generated yet: {}", art.filename())];
            }
        }
    }

    pub(super) fn current_artifact_path(&self) -> Option<PathBuf> {
        if self.viewing_log {
            return self
                .campaign
                .as_ref()
                .map(|c| c.notes_dir().join("_campaign-log.md"));
        }
        let si = self.open_session?;
        let sess = self.sessions.get(si)?;
        let cfg = self.campaign.as_ref()?;
        let art = ALL_ARTIFACTS[self.artifact_tab];
        Some(cfg.notes_dir().join(&sess.stem).join(art.filename()))
    }

    // ---- audio player ----------------------------------------------------

    /// Whether the currently-viewed artifact is the Quotes list (where the
    /// player is available).
    pub(super) fn viewing_quotes(&self) -> bool {
        !self.viewing_log
            && self.open_session.is_some()
            && ALL_ARTIFACTS[self.artifact_tab] == Artifact::Quotes
    }

    /// Source audio for the open session: the file recorded in metadata, or a
    /// best-effort match by stem in the audio dir (for sessions transcribed
    /// before metadata existed).
    fn session_source_audio(&self) -> Option<PathBuf> {
        let si = self.open_session?;
        let sess = self.sessions.get(si)?;
        let cfg = self.campaign.as_ref()?;
        // 1) Recorded metadata.
        if let Some(meta) = crate::meta::load(&cfg.transcripts_dir(), &sess.stem) {
            if let Some(audio) = meta.source_audio {
                if audio.exists() {
                    return Some(audio);
                }
            }
        }
        // 2) Fallback: an audio file in the audio dir whose stem matches.
        find_audio_by_stem(&crate::config::audio_dir(), &sess.stem)
    }

    /// All quote positions in the current view as `(line_index, seconds)`,
    /// in document order.
    pub(super) fn quote_positions(&self) -> Vec<(usize, f64)> {
        self.viewer_lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| parse_hms_bracket(l).map(|s| (i, s)))
            .collect()
    }

    /// Move the selected quote by `delta` (Quotes tab), scrolling it into view.
    /// Returns false if there are no quotes to navigate.
    pub(super) fn move_quote(&mut self, delta: i32) -> bool {
        let positions = self.quote_positions();
        if positions.is_empty() {
            return false;
        }
        let n = positions.len() as i32;
        let idx = (self.quote_idx as i32 + delta).clamp(0, n - 1);
        self.quote_idx = idx as usize;
        // Scroll so the selected quote (its text line, just above the timestamp)
        // sits near the top of the viewer.
        let ts_line = positions[self.quote_idx].0;
        let top = ts_line.saturating_sub(1);
        self.viewer_scroll = top as u16;
        self.status = format!(
            "quote {}/{}  ·  p to play",
            self.quote_idx + 1,
            positions.len()
        );
        true
    }

    /// The raw line-index range (inclusive) of the selected quote, for
    /// highlighting: the quote text line and its attribution/timestamp line.
    pub(super) fn selected_quote_range(&self) -> Option<(usize, usize)> {
        let positions = self.quote_positions();
        let (ts_line, _) = positions.get(self.quote_idx).copied()?;
        Some((ts_line.saturating_sub(1), ts_line))
    }

    /// The timestamp (seconds) of the currently-selected quote.
    fn current_quote_timestamp(&self) -> Option<f64> {
        let positions = self.quote_positions();
        positions
            .get(self.quote_idx)
            .or_else(|| positions.first())
            .map(|(_, s)| *s)
    }

    /// `p` in the Quotes view: play the source audio from the timestamp of the
    /// selected quote. Gives clear feedback on failure.
    pub(super) fn play_quote_here(&mut self) {
        let Some(secs) = self.current_quote_timestamp() else {
            self.status =
                "No [timestamp] in view — re-run Quotes to add timestamps, then try again".into();
            return;
        };
        let Some(audio) = self.session_source_audio() else {
            self.status =
                "No source audio found for this session (looked in metadata and audio/)".into();
            return;
        };
        let stem = self
            .open_session
            .and_then(|si| self.sessions.get(si))
            .map(|s| s.stem.clone())
            .unwrap_or_default();
        self.start_player(&audio, &stem, secs);
    }

    /// `p` in the Audio pane: play the highlighted audio file from the start,
    /// with scrubbing.
    pub(super) fn play_selected_audio(&mut self) {
        let Some(f) = self.audio.get(self.audio_idx) else {
            self.status = "No audio file selected".into();
            return;
        };
        let path = f.path.clone();
        let label = f.stem();
        self.start_player(&path, &label, 0.0);
    }

    /// Start (or restart) the player on `file` at `offset`.
    pub(super) fn start_player(&mut self, file: &std::path::Path, label: &str, offset: f64) {
        let vol = self.global.ui.player_volume;
        match super::player::Player::start(file, label, offset, vol) {
            Ok(p) => {
                self.status = format!(
                    "▶ {} from {} — space pause · , . seek · - + vol · S stop",
                    label,
                    super::player::fmt_time(offset)
                );
                self.player = Some(p);
            }
            Err(e) => self.status = format!("audio player unavailable: {e}"),
        }
    }

    /// Play one diarized source-audio cue, stopping at the cue boundary.
    pub(super) fn start_player_sample(
        &mut self,
        file: &std::path::Path,
        label: &str,
        start: f64,
        end: f64,
    ) {
        let vol = self.global.ui.player_volume;
        let display_label = format!("{label} sample");
        match super::player::Player::start_clip(file, &display_label, start, end, vol) {
            Ok(player) => {
                self.status = format!(
                    "▶ {display_label} {}–{} · space pause · S stop",
                    super::player::fmt_time(start),
                    super::player::fmt_time(end)
                );
                self.player = Some(player);
            }
            Err(error) => self.status = format!("audio sample unavailable: {error}"),
        }
    }

    /// Adjust the player volume by `delta` (percent), persisting the new value.
    pub(super) fn player_volume_change(&mut self, delta: i32) {
        let cur = self.global.ui.player_volume as i32;
        let vol = (cur + delta).clamp(0, 100) as u8;
        self.global.ui.player_volume = vol;
        self.global.save().ok();
        if let Some(p) = &mut self.player {
            p.set_volume(vol);
        }
        self.status = format!("🔊 volume {vol}%");
    }

    /// Pause/resume the active player.
    pub(super) fn player_toggle_pause(&mut self) {
        if let Some(p) = &mut self.player {
            p.toggle_pause();
            self.status = if p.paused {
                "⏸ paused".into()
            } else {
                "▶ playing".into()
            };
        }
    }

    /// Seek the active player by `delta` seconds.
    pub(super) fn player_seek(&mut self, delta: f64) {
        if let Some(p) = &mut self.player {
            p.seek(delta);
            self.status = format!("⏩ {}", super::player::fmt_time(p.position()));
        }
    }

    /// Seek the active player to an absolute position in seconds.
    pub(super) fn player_seek_to(&mut self, position: f64) {
        if let Some(p) = &mut self.player {
            p.seek_to(position);
            self.status = format!("⏩ {}", super::player::fmt_time(p.position()));
        }
    }

    /// Stop and drop the active player.
    pub(super) fn stop_audio(&mut self) {
        if self.player.take().is_some() {
            self.status = "⏹ stopped".into();
        }
    }

    /// `p` dispatcher: play the selected audio (Audio pane), the quote under the
    /// cursor (Quotes tab), or toggle pause if a player is already running.
    pub(super) fn play_context(&mut self) {
        if matches!(self.pane, Pane::Audio) {
            self.play_selected_audio();
        } else if self.viewing_quotes() {
            self.play_quote_here();
        } else if self.player.is_some() {
            self.player_toggle_pause();
        } else {
            self.status =
                "Nothing to play here — select an audio file, or open a session's Quotes tab"
                    .into();
        }
    }

    /// Drop the player once playback has finished on its own (called each tick).
    pub(super) fn tick_player(&mut self) {
        if let Some(p) = &mut self.player {
            if p.finished() {
                self.player = None;
            }
        }
    }

    // ---- candidate (keep-both compare) -----------------------------------

    /// Path of the `.candidate` file for the current artifact, if a session is
    /// open (regardless of whether it exists).
    pub(super) fn candidate_path(&self) -> Option<PathBuf> {
        if self.viewing_log {
            return None;
        }
        let si = self.open_session?;
        let sess = self.sessions.get(si)?;
        let cfg = self.campaign.as_ref()?;
        let art = ALL_ARTIFACTS[self.artifact_tab];
        Some(
            cfg.notes_dir()
                .join(&sess.stem)
                .join(crate::pipeline::artifact_file(art, true)),
        )
    }

    /// Whether a candidate exists for the current artifact.
    pub(super) fn has_candidate(&self) -> bool {
        self.candidate_path().map(|p| p.exists()).unwrap_or(false)
    }

    /// Toggle between the CURRENT (kept) artifact and the NEW candidate.
    pub(super) fn toggle_candidate_view(&mut self) {
        if !self.has_candidate() {
            return;
        }
        self.viewing_candidate = !self.viewing_candidate;
        self.refresh_viewer();
        self.status = if self.viewing_candidate {
            "showing the NEW version — press a to keep it, or c to compare".into()
        } else {
            "showing the CURRENT version — press a to keep it, or c to compare".into()
        };
    }

    /// Keep whichever version is currently on screen and remove the other.
    /// Viewing the NEW candidate → promote it over the current file; viewing the
    /// CURRENT version → discard the candidate.
    pub(super) fn keep_shown_version(&mut self) {
        if !self.has_candidate() {
            return;
        }
        let (Some(cand), Some(real)) = (self.candidate_path(), self.current_artifact_path()) else {
            return;
        };
        if self.viewing_candidate {
            // Keep the NEW version: replace the current file with the candidate.
            if let Err(e) = std::fs::rename(&cand, &real) {
                self.status = format!("keep failed: {e}");
                return;
            }
            if let (Some(si), tab) = (self.open_session, self.artifact_tab) {
                if let Some(sess) = self.sessions.get_mut(si) {
                    if let Some(flag) = sess.artifacts.get_mut(tab) {
                        *flag = true;
                    }
                }
            }
            self.status = "kept the NEW version".into();
        } else {
            // Keep the CURRENT version: throw away the candidate.
            let _ = std::fs::remove_file(&cand);
            self.status = "kept the CURRENT version".into();
        }
        self.viewing_candidate = false;
        self.refresh_viewer();
    }

    // ---- job events ------------------------------------------------------

    pub fn drain_job_events(&mut self) {
        self.drain_audio_duration_probes();
        // Absorb the background Ollama-installed query, if it has arrived.
        if let Some(mut rx) = self.model_refresh_rx.take() {
            match rx.try_recv() {
                Ok(list) => self.apply_ollama_installed(list),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    self.model_refresh_rx = Some(rx);
                }
                Err(_) => {}
            }
        }

        let mut done: Option<Result<String, String>> = None;
        if let Some(rx) = self.job_rx.as_mut() {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    UiEvent::Header(m) => self.job_log.push((LogLevel::Info, m)),
                    UiEvent::Step { n, total, msg } => self
                        .job_log
                        .push((LogLevel::Step, format!("[{n}/{total}] {msg}"))),
                    UiEvent::Ok(m) => self.job_log.push((LogLevel::Ok, m)),
                    UiEvent::Warn(m) => self.job_log.push((LogLevel::Warn, m)),
                    UiEvent::Error(m) => self.job_log.push((LogLevel::Error, m)),
                    UiEvent::Info(m) => self.job_log.push((LogLevel::Info, m)),
                    UiEvent::Progress {
                        label,
                        pos,
                        total,
                        rate,
                    } => {
                        self.job_progress = Some((label, pos, total, rate));
                    }
                    UiEvent::Phase(name) => {
                        if self
                            .job_stages
                            .last()
                            .map(|(stage, _)| stage != &name)
                            .unwrap_or(true)
                        {
                            self.job_stages
                                .push((name.clone(), std::time::Instant::now()));
                        }
                        self.job_log.push((LogLevel::Step, name));
                        self.job_progress = None;
                    }
                    UiEvent::JobDone(res) => done = Some(res),
                }
            }
        }
        if let Some(res) = done {
            self.job_running = false;
            self.job_rx = None;
            self.job_progress = None;
            self.job_elapsed = self.job_started.map(|s| s.elapsed());
            self.job_finished_at = Some(std::time::Instant::now());
            if self.job_elapsed.unwrap_or_default() >= std::time::Duration::from_secs(60) {
                print!("\x07");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                if self.global.ui.notify {
                    notify_job_done(&self.job_title, res.is_ok());
                }
            }
            match &res {
                Ok(summary) => {
                    self.job_log.push((LogLevel::Ok, summary.clone()));
                    self.status = format!("Done: {summary}");
                }
                Err(e) => {
                    self.job_log.push((LogLevel::Error, e.clone()));
                    self.status = format!("Failed: {e}");
                }
            }
            // Refresh data so new transcripts/artifacts appear, while keeping
            // the user's current document open when it still exists.
            let open_stem = self
                .open_session
                .and_then(|index| self.sessions.get(index))
                .map(|session| session.stem.clone());
            let was_log = self.viewing_log;
            let tab = self.artifact_tab;
            let was_candidate = self.viewing_candidate;
            let scroll = self.viewer_scroll;
            let camp_idx = self.campaign_idx;
            self.load_campaign_data();
            self.campaign_idx = camp_idx;
            self.artifact_tab = tab;
            self.viewing_candidate = was_candidate;
            if was_log {
                self.viewing_log = true;
                self.refresh_viewer();
                self.viewer_scroll = scroll.min(
                    self.viewer_lines
                        .len()
                        .saturating_sub(1)
                        .min(u16::MAX as usize) as u16,
                );
            } else if let Some(stem) = open_stem {
                if let Some(index) = self
                    .sessions
                    .iter()
                    .position(|session| session.stem == stem)
                {
                    self.open_session = Some(index);
                    self.session_idx = index;
                    self.log_selected = false;
                    self.sess_state.select(Some(index + 1));
                    self.refresh_viewer();
                    self.viewer_scroll = scroll.min(
                        self.viewer_lines
                            .len()
                            .saturating_sub(1)
                            .min(u16::MAX as usize) as u16,
                    );
                }
            }
            if self.models.is_some() {
                self.refresh_model_rows();
                self.spawn_model_refresh();
            }
            // Start the next queued model job, if any.
            if let Some((job, title)) = self.job_queue.pop_front() {
                self.start_model_job(job, title);
            }
        }
    }

    fn start_audio_duration_probes(&mut self) {
        self.audio_probe_generation = self.audio_probe_generation.wrapping_add(1);
        let generation = self.audio_probe_generation;
        let pending: Vec<(usize, PathBuf)> = self
            .audio
            .iter()
            .enumerate()
            .filter(|(_, file)| file.duration_secs.is_none())
            .map(|(index, file)| (index, file.path.clone()))
            .collect();
        if pending.is_empty() {
            self.audio_probe_rx = None;
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.audio_probe_rx = Some(rx);
        std::thread::spawn(move || {
            for (index, path) in pending {
                let duration = audio::probe_duration(&path);
                if tx.send((generation, index, duration)).is_err() {
                    break;
                }
            }
        });
    }

    fn drain_audio_duration_probes(&mut self) {
        let Some(rx) = &self.audio_probe_rx else {
            return;
        };
        while let Ok((generation, index, duration)) = rx.try_recv() {
            if generation == self.audio_probe_generation {
                if let Some(file) = self.audio.get_mut(index) {
                    file.duration_secs = duration;
                }
            }
        }
    }

    // ---- starting jobs ---------------------------------------------------

    fn require_ready(&mut self) -> Option<(CampaignConfig, Preset)> {
        if self.job_running {
            self.message("Busy", "A job is already running.", false);
            return None;
        }
        let (Some(cfg), Some(preset)) = (self.campaign.clone(), self.preset.clone()) else {
            self.message(
                "No campaign",
                "No campaign is loaded. Run `sessionsmith init` first.",
                true,
            );
            return None;
        };
        Some((cfg, preset))
    }

    pub(super) fn start_job(&mut self, mut req_kind: JobRequestBuilder) {
        let Some((cfg, preset)) = self.require_ready() else {
            return;
        };
        let g = crate::config::effective(&self.global, &cfg);
        let asr_model = g.asr.model.clone().unwrap_or_else(|| {
            crate::hardware::recommend(&crate::hardware::detect())
                .whisper_model
                .to_string()
        });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.job_rx = Some(rx);
        self.job_running = true;
        self.job_log.clear();
        self.job_scroll = 0;
        self.job_follow = true;
        self.job_progress = None;
        self.job_stages.clear();
        self.job_started = Some(std::time::Instant::now());
        self.job_elapsed = None;
        self.job_finished_at = None;
        self.job_title = req_kind.title.clone();
        self.status = format!("Running: {}", req_kind.title);

        let req = JobRequest {
            kind: req_kind.kind,
            g,
            campaign: cfg,
            preset,
            asr_model,
            sessions: std::mem::take(&mut req_kind.sessions),
            transcripts: std::mem::take(&mut req_kind.transcripts),
            artifacts: std::mem::take(&mut req_kind.artifacts),
            force: false,
            force_transcribe: false,
            resume: true,
            update_log: true,
            candidate: false,
            model_override: None,
        };
        jobs::spawn(&self.handle, tx, req);
    }

    /// Re-run the pipeline for one existing session. When `retranscribe` is
    /// true the source audio (metadata or a stem match in the audio dir) is
    /// re-transcribed with the current model; otherwise the existing transcript
    /// is reused and only notes are regenerated. Regenerates the selected
    /// `artifacts`; in `candidate` mode they are written as `.candidate` files
    /// for comparison and the campaign log is left untouched until kept.
    pub(super) fn start_rerun(
        &mut self,
        transcript: PathBuf,
        artifacts: Vec<Artifact>,
        candidate: bool,
        retranscribe: bool,
    ) {
        let Some((cfg, preset)) = self.require_ready() else {
            return;
        };
        let g = crate::config::effective(&self.global, &cfg);
        let asr_model = g.asr.model.clone().unwrap_or_else(|| {
            crate::hardware::recommend(&crate::hardware::detect())
                .whisper_model
                .to_string()
        });
        let stem = transcript
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        // When re-transcribing, resolve the source audio (recorded metadata or
        // a stem match in the audio dir). Otherwise reuse the transcript.
        let audio = if retranscribe {
            crate::meta::load(&cfg.transcripts_dir(), &stem)
                .and_then(|m| m.source_audio)
                .filter(|p| p.exists())
                .or_else(|| find_audio_by_stem(&crate::config::audio_dir(), &stem))
        } else {
            None
        };
        let (kind, sessions, transcripts, force_transcribe) = match audio {
            Some(a) => (
                JobKind::Run,
                vec![SessionInput {
                    files: vec![a],
                    name: stem.clone(),
                }],
                Vec::new(),
                true,
            ),
            None => (JobKind::Notes, Vec::new(), vec![transcript], false),
        };

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.job_rx = Some(rx);
        self.job_running = true;
        self.job_log.clear();
        self.job_scroll = 0;
        self.job_follow = true;
        self.job_progress = None;
        self.job_stages.clear();
        self.job_started = Some(std::time::Instant::now());
        self.job_elapsed = None;
        self.job_finished_at = None;
        let title = if candidate {
            format!("Re-run {stem} (compare)")
        } else {
            format!("Re-run {stem} (replace)")
        };
        self.job_title = title.clone();
        self.status = format!("Running: {title}");

        let req = JobRequest {
            kind,
            g,
            campaign: cfg,
            preset,
            asr_model,
            sessions,
            transcripts,
            artifacts,
            force: true,      // regenerate the selected artifacts
            force_transcribe, // re-transcribe only when the user chose to
            resume: false,
            update_log: !candidate,
            candidate,
            model_override: None,
        };
        jobs::spawn(&self.handle, tx, req);
    }

    pub(super) fn message(&mut self, title: &str, body: &str, error: bool) {
        self.overlay = Overlay::Message {
            title: title.into(),
            body: body.into(),
            error,
        };
    }

    pub(super) fn default_artifacts(&self) -> Vec<Artifact> {
        let defaults = self
            .campaign
            .as_ref()
            .map(|c| c.outputs.default.clone())
            .unwrap_or_default();
        let mut out: Vec<Artifact> = defaults
            .iter()
            .filter_map(|s| Artifact::from_id(s))
            .collect();
        if out.is_empty() {
            out = ALL_ARTIFACTS.to_vec();
        }
        out
    }

    // ---- model management ------------------------------------------------

    /// Open the interactive model manager in the content area (no network, so it
    /// opens instantly). It stays visible while install/delete jobs run in the
    /// Working pane, so several can be queued and monitored at once.
    pub(super) fn open_models(&mut self) {
        self.viewing_log = false;
        self.open_session = None;
        let expanded = std::collections::HashSet::new();
        let installed = std::collections::HashMap::new();
        let rows = self.build_model_rows(&expanded, &installed);
        let cursor = rows.iter().position(|r| r.selectable()).unwrap_or(0);
        self.models = Some(ModelsState {
            rows,
            cursor,
            scroll: 0,
            expanded,
            installed,
        });
        self.pane = Pane::Content;
        self.spawn_model_refresh();
    }

    /// Query Ollama for locally-installed models (name + size) in the background
    /// so the manager can mark them installed and show real sizes.
    fn spawn_model_refresh(&mut self) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.model_refresh_rx = Some(rx);
        let base = self
            .global
            .backend
            .base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".into());
        self.handle.spawn(async move {
            let list = crate::models::ollama_local_models(&base).await;
            let _ = tx.send(list);
        });
    }

    /// Store the locally-installed Ollama models and rebuild rows so installed
    /// markers and real sizes appear (even for collapsed variants → family ✓).
    fn apply_ollama_installed(&mut self, list: Vec<(String, u64)>) {
        if let Some(s) = &mut self.models {
            s.installed = list.into_iter().collect();
        }
        self.refresh_model_rows();
    }

    fn build_model_rows(
        &self,
        expanded: &std::collections::HashSet<String>,
        installed: &std::collections::HashMap<String, u64>,
    ) -> Vec<ModelRow> {
        use crate::models;
        let mut rows: Vec<ModelRow> = Vec::new();

        // --- Whisper (flat) ---
        let cache = models::whisper_cache_dir(self.global.asr.model_dir.as_deref()).ok();
        let asr_default = self.global.asr.model.clone().unwrap_or_default();
        rows.push(header_row("Whisper (built-in, offline) · speech-to-text"));
        rows.push(col_header_row());
        for m in models::WHISPER_MODELS {
            let inst = cache
                .as_ref()
                .and_then(|c| models::whisper_path(m.id, c).ok())
                .map(|p| p.exists())
                .unwrap_or(false);
            let size = cache
                .as_ref()
                .and_then(|c| std::fs::metadata(c.join(m.filename)).ok())
                .map(|md| md.len())
                .unwrap_or_else(|| models::whisper_approx_size(m.id));
            rows.push(ModelRow {
                kind: ModelKind::Whisper,
                display: m.id.to_string(),
                id: m.id.to_string(),
                installed: inst,
                is_default: m.id == asr_default,
                size,
                released: models::whisper_released(m.id).to_string(),
                ..Default::default()
            });
        }

        // --- Advanced ASR engines (via uv/local bridge; select as default only) ---
        rows.push(header_row("Advanced ASR engines (uv/local bridge)"));
        rows.push(col_header_row());
        for m in crate::asr::ASR_CATALOG {
            if m.engine == crate::asr::AsrEngine::WhisperCpp {
                continue; // whisper.cpp ggml models are listed above
            }
            rows.push(ModelRow {
                kind: ModelKind::Asr,
                display: m.display.to_string(),
                id: m.id.to_string(),
                installed: if m.engine == crate::asr::AsrEngine::TranscribeCpp {
                    models::gguf_asr_cache_dir()
                        .ok()
                        .and_then(|cache| models::gguf_asr_path(m.id, &cache).ok())
                        .map(|path| path.exists())
                        .unwrap_or(false)
                } else {
                    crate::asr::is_prepared(m.id)
                },
                is_default: m.id == asr_default,
                size: m.size,
                released: m.released.to_string(),
                ..Default::default()
            });
        }

        // --- Ollama (expandable families) ---
        let llm_default = self.global.backend.model.clone().unwrap_or_default();
        rows.push(header_row("Ollama · language models"));
        rows.push(col_header_row());
        let mut known: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for model in models::OLLAMA_CATALOG {
            for o in model.options {
                known.insert(o.pull);
            }
            if model.options.len() == 1 {
                // Single option → flat leaf row.
                let o = &model.options[0];
                let real = installed.get(o.pull).copied();
                rows.push(ModelRow {
                    kind: ModelKind::Ollama,
                    display: model.display.to_string(),
                    id: o.pull.to_string(),
                    installed: real.is_some(),
                    is_default: o.pull == llm_default,
                    size: real.filter(|s| *s > 0).unwrap_or(o.size),
                    released: model.released.to_string(),
                    ..Default::default()
                });
            } else {
                // Multi-variant → expandable family, with children when expanded.
                let key = model.display.to_string();
                let is_exp = expanded.contains(&key);
                let any_inst = model.options.iter().any(|o| installed.contains_key(o.pull));
                let any_def = model.options.iter().any(|o| o.pull == llm_default);
                rows.push(ModelRow {
                    family: true,
                    expanded: is_exp,
                    kind: ModelKind::Ollama,
                    display: model.display.to_string(),
                    variant_count: model.options.len(),
                    expand_key: key,
                    installed: any_inst,
                    is_default: any_def,
                    released: model.released.to_string(),
                    ..Default::default()
                });
                if is_exp {
                    for o in model.options {
                        let real = installed.get(o.pull).copied();
                        rows.push(ModelRow {
                            indent: 1,
                            kind: ModelKind::Ollama,
                            display: o.label.to_string(),
                            id: o.pull.to_string(),
                            installed: real.is_some(),
                            is_default: o.pull == llm_default,
                            size: real.filter(|s| *s > 0).unwrap_or(o.size),
                            released: model.released.to_string(),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        // Installed models that aren't in the catalog (e.g. pulled elsewhere).
        let mut extras: Vec<(&String, &u64)> = installed
            .iter()
            .filter(|(n, _)| !known.contains(n.as_str()))
            .collect();
        extras.sort_by(|a, b| a.0.cmp(b.0));
        for (name, size) in extras {
            rows.push(ModelRow {
                kind: ModelKind::Ollama,
                display: strip_org(name),
                id: name.clone(),
                installed: true,
                is_default: *name == llm_default,
                size: *size,
                released: models::ollama_released(name).to_string(),
                ..Default::default()
            });
        }
        rows
    }

    /// Move the model-manager cursor by `delta`, skipping non-selectable rows.
    pub(super) fn models_move(&mut self, delta: i32) {
        let Some(s) = &mut self.models else { return };
        let n = s.rows.len() as i32;
        let mut i = s.cursor as i32;
        loop {
            i += delta;
            if i < 0 || i >= n {
                return;
            }
            if s.rows[i as usize].selectable() {
                s.cursor = i as usize;
                return;
            }
        }
    }

    /// The (kind, id) of the highlighted installable model row, if any.
    fn selected_model(&self) -> Option<(ModelKind, String)> {
        let s = self.models.as_ref()?;
        s.rows
            .get(s.cursor)
            .filter(|r| r.selectable() && !r.family)
            .map(|r| (r.kind, r.id.clone()))
    }

    /// Set the highlighted model as the default (ASR or LLM) and persist it.
    pub(super) fn set_model_default(&mut self) {
        let Some((kind, id)) = self.selected_model() else {
            return;
        };
        match kind {
            ModelKind::Whisper => self.global.asr.model = Some(id.clone()),
            ModelKind::Asr => self.global.asr.model = Some(id.clone()),
            ModelKind::Ollama => self.global.backend.model = Some(id.clone()),
        }
        if matches!(kind, ModelKind::Whisper | ModelKind::Asr) {
            self.refresh_asr_labels();
        }
        self.global.save().ok();
        self.status = format!("Default set: {id} (saved)");
        self.refresh_model_rows();
    }

    /// Expand/collapse the highlighted family row.
    pub(super) fn toggle_models_expand(&mut self) {
        let key = match &self.models {
            Some(s) => s
                .rows
                .get(s.cursor)
                .filter(|r| r.family)
                .map(|r| r.expand_key.clone()),
            None => None,
        };
        let Some(key) = key else { return };
        if let Some(s) = &mut self.models {
            if !s.expanded.remove(&key) {
                s.expanded.insert(key.clone());
            }
        }
        self.refresh_model_rows();
        if let Some(s) = &mut self.models {
            if let Some(i) = s.rows.iter().position(|r| r.family && r.expand_key == key) {
                s.cursor = i;
            }
        }
    }

    /// True if the highlighted row is a family (needs expand, not install).
    pub(super) fn selected_is_family(&self) -> bool {
        self.models
            .as_ref()
            .and_then(|s| s.rows.get(s.cursor))
            .map(|r| r.family)
            .unwrap_or(false)
    }

    /// Rebuild rows in place (preserving cursor/scroll) after a change.
    pub(super) fn refresh_model_rows(&mut self) {
        let (exp, inst) = match &self.models {
            Some(s) => (s.expanded.clone(), s.installed.clone()),
            None => return,
        };
        let rows = self.build_model_rows(&exp, &inst);
        if let Some(s) = &mut self.models {
            s.cursor = s.cursor.min(rows.len().saturating_sub(1));
            s.rows = rows;
        }
    }

    /// Toggle speaker diarization on/off and persist it. Warns when enabling
    /// without a Hugging Face token (diarization needs one).
    pub(super) fn toggle_diarize(&mut self) {
        let on = !self.global.asr.diarize;
        self.global.asr.diarize = on;
        self.global.save().ok();
        if on {
            if self.global.resolved_hf_token().is_none() {
                self.status = "Speaker diarization: ON — but set [asr] hf_token (Hugging Face) \
                     and accept the pyannote community-1 terms, or it will fail"
                    .into();
            } else {
                self.status = "Speaker diarization: ON (whisperX + pyannote community-1)".into();
            }
        } else {
            self.status = "Speaker diarization: OFF".into();
        }
    }

    /// Queue the Ollama updater to run with the TUI suspended (so it can prompt
    /// for sudo and show output). No-op with a hint on unsupported platforms.
    pub(super) fn request_ollama_update(&mut self) {
        match ollama_update_command() {
            Some(cmd) => {
                self.pending_shell = Some(("Update Ollama".to_string(), cmd));
            }
            None => self.message(
                "Not supported here",
                "Automatic update isn't available on this OS.\nDownload Ollama from https://ollama.com/download",
                true,
            ),
        }
    }

    pub(super) fn request_cuda_toolkit_install(&mut self) {
        match cuda_toolkit_command() {
            Some(cmd) => {
                self.pending_shell = Some(("Install CUDA toolkit".to_string(), cmd));
            }
            None => self.message(
                "Not supported here",
                "Automatic CUDA toolkit installation is only scripted for Linux package managers. Install CUDA toolkit/nvcc, then run models pull for Cohere again.",
                true,
            ),
        }
    }

    /// Queue an install/update (`install = true`) or delete of a specific row.
    pub(super) fn model_action_at(&mut self, row_idx: usize, install: bool) {
        if let Some(s) = &mut self.models {
            if row_idx < s.rows.len() && s.rows[row_idx].selectable() && !s.rows[row_idx].family {
                s.cursor = row_idx;
            }
        }
        self.model_action(install);
    }

    /// Queue an install/update or delete of the highlighted model. Multiple
    /// actions can be queued; they run one at a time in the Working pane.
    pub(super) fn model_action(&mut self, install: bool) {
        let Some((kind, id)) = self.selected_model() else {
            return;
        };
        let already_installed = self
            .models
            .as_ref()
            .and_then(|s| s.rows.get(s.cursor))
            .map(|r| r.installed)
            .unwrap_or(false);
        if !install && !already_installed {
            self.status = format!("{id} is not installed");
            return;
        }
        let job = match (kind, install) {
            (ModelKind::Whisper, true) => ModelJob::PullWhisper(id.clone()),
            (ModelKind::Whisper, false) => ModelJob::DeleteWhisper(id.clone()),
            (ModelKind::Ollama, true) => ModelJob::PullOllama(id.clone()),
            (ModelKind::Ollama, false) => ModelJob::DeleteOllama(id.clone()),
            (ModelKind::Asr, true) => ModelJob::PrepareAsr(id.clone()),
            (ModelKind::Asr, false) => ModelJob::DeleteAsr(id.clone()),
        };
        let title = match (kind, install) {
            (ModelKind::Asr, true) if already_installed => format!("Check {id}"),
            (ModelKind::Asr, _) => format!("Prepare {id}"),
            (_, true) if already_installed => format!("Check {id}"),
            (_, true) => format!("Install {id}"),
            (_, false) => format!("Delete {id}"),
        };
        self.enqueue_model_job(job, title);
    }

    fn enqueue_model_job(&mut self, job: ModelJob, title: String) {
        if self.job_running {
            self.job_queue.push_back((job, title.clone()));
            self.status = format!("Queued: {title} ({} in queue)", self.job_queue.len());
        } else {
            // Fresh batch — start with a clean log.
            self.job_log.clear();
            self.start_model_job(job, title);
        }
    }

    fn start_model_job(&mut self, job: ModelJob, title: String) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.job_rx = Some(rx);
        self.job_running = true;
        self.job_scroll = 0;
        self.job_follow = true;
        self.job_progress = None;
        self.job_stages.clear();
        self.job_started = Some(std::time::Instant::now());
        self.job_elapsed = None;
        self.job_finished_at = None;
        self.job_title = title.clone();
        self.status = format!("Running: {title}");
        jobs::spawn_model(&self.handle, tx, self.global.clone(), job);
    }
}

fn asr_device_label(configured: Option<&str>, detected_vendor: Option<&str>) -> String {
    configured
        .filter(|device| !device.eq_ignore_ascii_case("auto"))
        .map(str::to_string)
        .or_else(|| detected_vendor.map(str::to_string))
        .unwrap_or_else(|| "CPU".to_string())
}

fn notify_job_done(title: &str, succeeded: bool) {
    let body = if succeeded {
        "SessionSmith job completed"
    } else {
        "SessionSmith job failed"
    };
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .args([title, body])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification {} with title {}",
            applescript_string(body),
            applescript_string(title),
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn();
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = (title, body);
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('\"', "\\\""))
}

/// Small builder used to hand a job over to [`App::start_job`].
pub(super) struct JobRequestBuilder {
    pub(super) title: String,
    pub(super) kind: JobKind,
    pub(super) sessions: Vec<SessionInput>,
    pub(super) transcripts: Vec<PathBuf>,
    pub(super) artifacts: Vec<Artifact>,
}

/// Parse a leading/inline `[HH:MM:SS]` (or `[M:SS]`) timestamp from a line,
/// returning seconds. Only numeric `:`-separated content counts, so markdown
/// links like `[text]` are ignored.
fn parse_hms_bracket(line: &str) -> Option<f64> {
    let a = line.find('[')?;
    let rest = &line[a + 1..];
    let b = rest.find(']')?;
    let inner = &rest[..b];
    let parts: Vec<&str> = inner.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let nums: Option<Vec<f64>> = parts.iter().map(|p| p.trim().parse::<f64>().ok()).collect();
    let nums = nums?;
    let secs = match nums.len() {
        3 => nums[0] * 3600.0 + nums[1] * 60.0 + nums[2],
        2 => nums[0] * 60.0 + nums[1],
        _ => return None,
    };
    Some(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_continue_to_parse_alongside_markdown_links() {
        assert_eq!(
            parse_hms_bracket("[01:23] [guide](https://example.test)"),
            Some(83.0)
        );
        assert_eq!(parse_hms_bracket("[guide](https://example.test)"), None);
    }

    #[test]
    fn campaign_asr_overrides_drive_header_labels() {
        let mut global = GlobalConfig::default();
        global.asr.model = Some("base".into());
        global.asr.diarize = false;
        let mut campaign = CampaignConfig::default();
        campaign.asr.model = Some("parakeet-tdt-0.6b-v3".into());
        campaign.asr.diarize = Some(true);
        let effective = crate::config::effective(&global, &campaign);
        assert_eq!(effective.asr.model.as_deref(), Some("parakeet-tdt-0.6b-v3"));
        assert!(effective.asr.diarize);
    }
}

/// Find an audio file in `audio_dir` whose file stem matches `stem` (used to
/// locate the source recording for sessions transcribed before metadata
/// existed). Searches common audio extensions.
fn find_audio_by_stem(audio_dir: &std::path::Path, stem: &str) -> Option<PathBuf> {
    audio::find_by_stem(audio_dir, stem)
}

#[cfg(test)]
mod audio_probe_tests {
    use super::*;

    #[test]
    fn stale_audio_duration_results_cannot_mutate_current_campaign() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut app = App::new(runtime.handle().clone());
        app.audio = vec![AudioFile {
            path: PathBuf::from("current.wav"),
            mtime: SystemTime::UNIX_EPOCH,
            duration_secs: None,
            size_bytes: 1,
            already_transcribed: false,
        }];
        app.audio_probe_generation = 2;
        let (tx, rx) = std::sync::mpsc::channel();
        app.audio_probe_rx = Some(rx);
        tx.send((1, 0, Some(99.0))).unwrap();
        app.drain_audio_duration_probes();
        assert_eq!(app.audio[0].duration_secs, None);
    }
}

fn roster_names(campaign: &CampaignConfig) -> Vec<String> {
    let mut names = Vec::new();
    if !campaign.campaign.gm.trim().is_empty() {
        names.push(campaign.campaign.gm.clone());
    }
    for player in &campaign.players {
        let name = player.player.trim();
        if !name.is_empty() && !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
    }
    names
}

#[cfg(test)]
mod roster_tests {
    use super::*;

    #[test]
    fn speaker_mapping_choices_use_people_and_include_the_gm() {
        let mut campaign = CampaignConfig::default();
        campaign.campaign.gm = "Michael".into();
        campaign.players = vec![
            crate::config::Player {
                player: "Ravn".into(),
                character: "Jan Simen".into(),
                ..Default::default()
            },
            crate::config::Player {
                player: "Emilie".into(),
                character: "Fatethrial".into(),
                ..Default::default()
            },
            crate::config::Player {
                player: "Ravn".into(),
                character: "A different character".into(),
                ..Default::default()
            },
        ];

        assert_eq!(roster_names(&campaign), vec!["Michael", "Ravn", "Emilie"]);
    }
}

/// The platform command that installs/updates Ollama, or `None` if we can't
/// script it (e.g. Windows). Runs the official installer.
fn ollama_update_command() -> Option<String> {
    if cfg!(target_os = "linux") {
        Some("curl -fsSL https://ollama.com/install.sh | sh".to_string())
    } else if cfg!(target_os = "macos") {
        // Prefer Homebrew when present; otherwise fall back to the install script.
        Some(
            "if command -v brew >/dev/null 2>&1; then brew upgrade ollama; \
             else curl -fsSL https://ollama.com/install.sh | sh; fi"
                .to_string(),
        )
    } else {
        None
    }
}

/// The campaign's config file stem, used as its stable id for ordering.
fn campaign_stem(path: &std::path::Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn shell_quote(s: &str) -> String {
    #[cfg(windows)]
    return format!("\"{}\"", s.replace('"', "\\\""));
    #[cfg(not(windows))]
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn cuda_toolkit_command() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let exe = shell_quote(&exe.display().to_string());
    Some(format!(
                "set -e; \
                 if command -v nvcc >/dev/null 2>&1; then \
                     echo 'CUDA toolkit already installed.'; \
                 elif command -v apt-get >/dev/null 2>&1; then \
                     sudo apt-get update && sudo apt-get install -y nvidia-cuda-toolkit; \
                 elif command -v dnf >/dev/null 2>&1; then \
                     sudo dnf install -y cuda-toolkit || sudo dnf install -y nvidia-cuda-toolkit; \
                 elif command -v pacman >/dev/null 2>&1; then \
                     sudo pacman -S --needed cuda; \
                 else \
                     echo 'No supported package manager found. Install CUDA toolkit/nvcc manually.'; exit 1; \
                 fi; \
                 {exe} models pull cohere-transcribe-03-2026"
        ))
}

fn header_row(title: &str) -> ModelRow {
    ModelRow {
        header: true,
        id: title.to_string(),
        ..Default::default()
    }
}

fn col_header_row() -> ModelRow {
    ModelRow {
        header: true,
        col_header: true,
        ..Default::default()
    }
}

/// Strip an `org/` prefix (and any GGUF/quant suffix) from a model id for
/// display, while the full id is still used for pulling.
fn strip_org(name: &str) -> String {
    let mut s = name.rsplit('/').next().unwrap_or(name).to_string();
    if let Some(p) = s.strip_suffix("-GGUF").or_else(|| s.strip_suffix("-gguf")) {
        s = p.to_string();
    }
    // Drop a trailing quant tag like `-Q4_K_M`.
    if let Some(idx) = s.rfind("-Q") {
        if s[idx + 2..]
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            s.truncate(idx);
        }
    }
    s
}
