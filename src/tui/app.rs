//! TUI application state and behaviour. Rendering lives in [`super::draw`];
//! background work lives in [`super::jobs`].

use std::path::PathBuf;
use std::collections::VecDeque;
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
    RunPipeline,
    Transcribe,
    GenerateNotes,
    OpenSession,
    OpenInEditor,
    Search,
    NextCampaign,
    CycleTheme,
    ManageModels,
    RebuildLog,
    SystemCheck,
    Quit,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::RunPipeline => "Run pipeline — transcribe + notes",
            Action::Transcribe => "Transcribe audio — audio → transcript",
            Action::GenerateNotes => "Generate notes — transcript → notes",
            Action::OpenSession => "Open session in viewer",
            Action::OpenInEditor => "Open current artifact in $EDITOR",
            Action::Search => "Search notes",
            Action::NextCampaign => "Switch campaign",
            Action::CycleTheme => "Change theme",
            Action::ManageModels => "Manage models — install / update / delete",
            Action::RebuildLog => "Rebuild campaign log",
            Action::SystemCheck => "System check (doctor)",
            Action::Quit => "Quit",
        }
    }
    pub fn all() -> &'static [Action] {
        &[
            Action::RunPipeline,
            Action::Transcribe,
            Action::GenerateNotes,
            Action::OpenSession,
            Action::OpenInEditor,
            Action::Search,
            Action::NextCampaign,
            Action::CycleTheme,
            Action::ManageModels,
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
    pub cursor: usize,
}

#[derive(Clone, Copy, PartialEq)]
pub enum PickerKind {
    AudioRun,
    AudioTranscribe,
    Artifacts,
    /// Artifact selection shown after choosing audio for a full run.
    RunArtifacts,
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

pub enum Overlay {
    None,
    Help,
    Palette(PaletteState),
    Search(SearchState),
    Picker(PickerState),
    /// Theme chooser with live preview. `original` is restored on Esc.
    ThemePicker { cursor: usize, original: usize },
    Message { title: String, body: String, error: bool },
}

/// Which family a model row belongs to.
#[derive(Clone, Copy, PartialEq)]
pub enum ModelKind {
    Whisper,
    Ollama,
}

pub struct ModelRow {
    /// A non-selectable section header when true.
    pub header: bool,
    pub kind: ModelKind,
    pub id: String,
    pub installed: bool,
    pub is_default: bool,
    pub size: u64,
}

pub struct ModelsState {
    pub rows: Vec<ModelRow>,
    pub cursor: usize,
    pub scroll: usize,
}

#[derive(Default, Clone)]
pub struct Rects {
    pub campaigns: Rect,
    pub sessions: Rect,
    pub audio: Rect,
    pub tabs: Rect,
    pub viewer: Rect,
    pub overlay_list: Rect,
    /// Per-artifact-tab horizontal ranges `(start_x, end_x)` for mouse hits.
    pub tab_ranges: Vec<(u16, u16)>,
    /// The footer keybar row.
    pub footer: Rect,
    /// Clickable footer entries: `(start_x, end_x, row_y, command)`.
    pub footer_hits: Vec<(u16, u16, u16, FooterCmd)>,
    /// The live-job log pane.
    pub job: Rect,
    /// The interactive model-manager content pane (inner area).
    pub models_pane: Rect,
    /// Clickable model-row buttons: `(x0, x1, row_y, row_index, install)`.
    pub model_buttons: Vec<(u16, u16, u16, usize, bool)>,
}

/// A clickable footer shortcut.
#[derive(Clone, Copy)]
pub enum FooterCmd {
    Palette,
    Search,
    Help,
    Quit,
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

    pub campaigns: Vec<CampaignEntry>,
    pub campaign_idx: usize,
    pub campaign: Option<CampaignConfig>,
    pub preset: Option<Preset>,
    pub preset_name: String,
    pub global: GlobalConfig,

    pub audio: Vec<AudioFile>,
    pub audio_idx: usize,

    pub sessions: Vec<SessionEntry>,
    pub session_idx: usize,

    pub pane: Pane,

    /// True when the synthetic "Campaign Log" row is highlighted in Sessions.
    pub log_selected: bool,

    pub open_session: Option<usize>,
    /// True when the viewer is showing the rolling campaign log.
    pub viewing_log: bool,
    pub artifact_tab: usize,
    /// Audio selected for a run, held while the artifact picker is shown.
    pub pending_run_sessions: Vec<SessionInput>,
    pub viewer_lines: Vec<String>,
    pub viewer_scroll: u16,

    pub job_running: bool,
    pub job_title: String,
    pub job_log: Vec<(LogLevel, String)>,
    pub job_rx: Option<UnboundedReceiver<UiEvent>>,
    /// Scroll offset (in wrapped rows) into the job log pane.
    pub job_scroll: u16,
    /// When true, the job pane stays pinned to the newest output.
    pub job_follow: bool,
    /// Latest progress update `(label, pos, total)`; `total == 0` = indeterminate.
    pub job_progress: Option<(String, u64, u64)>,
    /// Animation frame counter (advances each draw) for the working spinner.
    pub tick: u64,
    /// Pending model jobs waiting for the current one to finish.
    pub job_queue: VecDeque<(ModelJob, String)>,
    /// When set, the content area shows the interactive model manager.
    pub models: Option<ModelsState>,

    /// Last known mouse position, for hover highlighting.
    pub hover_col: u16,
    pub hover_row: u16,
    /// Whether mouse capture is currently on. Toggled for native text
    /// selection ("select mode").
    pub mouse_enabled: bool,
    /// Set when the event loop should toggle terminal mouse capture.
    pub mouse_toggle_pending: bool,
    /// Text queued to be copied to the system clipboard (via OSC 52).
    pub copy_pending: Option<String>,

    pub overlay: Overlay,
    pub status: String,

    pub camp_state: ListState,
    pub sess_state: ListState,
    pub audio_state: ListState,
    pub overlay_state: ListState,

    pub rects: Rects,
}

impl App {
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        let global = GlobalConfig::load_or_default().unwrap_or_default();
        let (themes, theme_idx) = theme::resolve(&global.ui.theme);

        let mut app = Self {
            handle,
            themes,
            theme_idx,
            should_quit: false,
            pending_editor: None,
            campaigns: Vec::new(),
            campaign_idx: 0,
            campaign: None,
            preset: None,
            preset_name: "—".into(),
            global,
            audio: Vec::new(),
            audio_idx: 0,
            sessions: Vec::new(),
            session_idx: 0,
            pane: Pane::Campaigns,
            log_selected: false,
            open_session: None,
            viewing_log: false,
            artifact_tab: 0,
            pending_run_sessions: Vec::new(),
            viewer_lines: Vec::new(),
            viewer_scroll: 0,
            job_running: false,
            job_title: String::new(),
            job_log: Vec::new(),
            job_rx: None,
            job_scroll: 0,
            job_follow: true,
            job_progress: None,
            tick: 0,
            job_queue: VecDeque::new(),
            models: None,
            hover_col: 0,
            hover_row: 0,
            mouse_enabled: true,
            mouse_toggle_pending: false,
            copy_pending: None,
            overlay: Overlay::None,
            status: "Welcome to SessionSmith. Press : for the command palette, ? for help.".into(),
            camp_state: ListState::default(),
            sess_state: ListState::default(),
            audio_state: ListState::default(),
            overlay_state: ListState::default(),
            rects: Rects::default(),
        };
        app.load_campaigns();
        app.load_campaign_data();
        app
    }

    pub fn theme(&self) -> &Theme {
        &self.themes[self.theme_idx]
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
                            p.file_stem().unwrap_or_default().to_string_lossy().to_string()
                        });
                    entries.push(CampaignEntry { name, path: p });
                }
            }
        }
        // Backward compat: root campaign.toml.
        let root = PathBuf::from("campaign.toml");
        if entries.is_empty() && root.exists() {
            if let Ok(c) = CampaignConfig::load(&root) {
                entries.push(CampaignEntry { name: c.campaign.name, path: root });
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
        self.camp_state.select(if self.campaigns.is_empty() { None } else { Some(self.campaign_idx) });
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
        self.global.ui.campaign_order =
            self.campaigns.iter().map(|c| campaign_stem(&c.path)).collect();
        self.global.save().ok();
        self.status = format!(
            "Moved '{}' — order saved",
            self.campaigns.get(to).map(|c| c.name.as_str()).unwrap_or("")
        );
    }

    /// Load the selected campaign's config, preset, audio and sessions, and pin
    /// it in the environment so any dispatched command resolves the same one.
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
        if let Ok(abs) = std::fs::canonicalize(&entry.path) {
            std::env::set_var("SESSIONSMITH_CAMPAIGN", abs);
        }
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
        self.audio_idx = 0;
        self.audio_state.select(if self.audio.is_empty() { None } else { Some(0) });

        // Sessions = existing transcripts + which artifacts exist.
        let notes_dir = cfg.notes_dir();
        let mut sessions = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&tx_dir) {
            let mut txts: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("txt"))
                .collect();
            txts.sort_by_key(|p| {
                std::cmp::Reverse(std::fs::metadata(p).and_then(|m| m.modified()).ok())
            });
            for t in txts {
                let stem = t.file_stem().unwrap_or_default().to_string_lossy().to_string();
                let modified = std::fs::metadata(&t)
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                let artifacts = ALL_ARTIFACTS
                    .iter()
                    .map(|a| notes_dir.join(&stem).join(a.filename()).exists())
                    .collect();
                sessions.push(SessionEntry { stem, transcript: t, modified, artifacts });
            }
        }
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
    }

    fn asr_model(&self) -> String {
        self.global
            .asr
            .model
            .clone()
            .unwrap_or_else(|| crate::hardware::recommend(&crate::hardware::detect()).whisper_model.to_string())
    }

    pub fn asr_model_label(&self) -> String {
        self.asr_model()
    }

    pub fn backend_summary(&self) -> String {
        let model = self.global.backend.model.clone().unwrap_or_else(|| "(not set)".into());
        format!("{} / {}", self.global.backend.kind, model)
    }

    // ---- viewer ----------------------------------------------------------

    pub fn refresh_viewer(&mut self) {
        self.viewer_lines.clear();
        self.viewer_scroll = 0;

        // Campaign log view.
        if self.viewing_log {
            let Some(cfg) = &self.campaign else { return };
            let path = cfg.notes_dir().join("_campaign-log.md");
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    self.viewer_lines = text.lines().map(|l| l.to_string()).collect();
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
        let Some(sess) = self.sessions.get(si) else { return };
        let Some(cfg) = &self.campaign else { return };
        let art = ALL_ARTIFACTS[self.artifact_tab];
        let path = cfg.notes_dir().join(&sess.stem).join(art.filename());
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
            return self.campaign.as_ref().map(|c| c.notes_dir().join("_campaign-log.md"));
        }
        let si = self.open_session?;
        let sess = self.sessions.get(si)?;
        let cfg = self.campaign.as_ref()?;
        let art = ALL_ARTIFACTS[self.artifact_tab];
        Some(cfg.notes_dir().join(&sess.stem).join(art.filename()))
    }

    // ---- job events ------------------------------------------------------

    pub fn drain_job_events(&mut self) {
        let mut done: Option<Result<String, String>> = None;
        if let Some(rx) = self.job_rx.as_mut() {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    UiEvent::Header(m) => self.job_log.push((LogLevel::Info, m)),
                    UiEvent::Step { n, total, msg } => {
                        self.job_log.push((LogLevel::Step, format!("[{n}/{total}] {msg}")))
                    }
                    UiEvent::Ok(m) => self.job_log.push((LogLevel::Ok, m)),
                    UiEvent::Warn(m) => self.job_log.push((LogLevel::Warn, m)),
                    UiEvent::Error(m) => self.job_log.push((LogLevel::Error, m)),
                    UiEvent::Info(m) => self.job_log.push((LogLevel::Info, m)),
                    UiEvent::Progress { label, pos, total } => {
                        self.job_progress = Some((label, pos, total));
                    }
                    UiEvent::JobDone(res) => done = Some(res),
                }
            }
        }
        if let Some(res) = done {
            self.job_running = false;
            self.job_rx = None;
            self.job_progress = None;
            match &res {
                Ok(summary) => {
                    self.job_log.push((LogLevel::Ok, format!("✓ {summary}")));
                    self.status = format!("Done: {summary}");
                }
                Err(e) => {
                    self.job_log.push((LogLevel::Error, format!("✗ {e}")));
                    self.status = format!("Failed: {e}");
                }
            }
            // Refresh data so new transcripts/artifacts appear.
            let camp_idx = self.campaign_idx;
            self.load_campaign_data();
            self.campaign_idx = camp_idx;
            if self.models.is_some() {
                self.refresh_model_rows();
            }
            if let Some(si) = self.open_session {
                if si < self.sessions.len() {
                    self.refresh_viewer();
                }
            }
            // Start the next queued model job, if any.
            if let Some((job, title)) = self.job_queue.pop_front() {
                self.start_model_job(job, title);
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
            self.message("No campaign", "No campaign is loaded. Run `sessionsmith init` first.", true);
            return None;
        };
        Some((cfg, preset))
    }

    pub(super) fn start_job(&mut self, mut req_kind: JobRequestBuilder) {
        let Some((cfg, preset)) = self.require_ready() else { return };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.job_rx = Some(rx);
        self.job_running = true;
        self.job_log.clear();
        self.job_scroll = 0;
        self.job_follow = true;
        self.job_progress = None;
        self.job_title = req_kind.title.clone();
        self.status = format!("Running: {}", req_kind.title);

        let req = JobRequest {
            kind: req_kind.kind,
            g: self.global.clone(),
            campaign: cfg,
            preset,
            asr_model: self.asr_model(),
            sessions: std::mem::take(&mut req_kind.sessions),
            transcripts: std::mem::take(&mut req_kind.transcripts),
            artifacts: std::mem::take(&mut req_kind.artifacts),
            force: false,
            resume: true,
            update_log: true,
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
        let cursor = self
            .build_model_rows()
            .iter()
            .position(|r| !r.header)
            .unwrap_or(0);
        self.models = Some(ModelsState {
            rows: self.build_model_rows(),
            cursor,
            scroll: 0,
        });
        self.pane = Pane::Content;
    }

    fn build_model_rows(&self) -> Vec<ModelRow> {
        let mut rows: Vec<ModelRow> = Vec::new();
        let cache = crate::models::whisper_cache_dir(self.global.asr.model_dir.as_deref()).ok();
        let asr_default = self.global.asr.model.clone().unwrap_or_default();

        rows.push(header_row("Whisper · speech-to-text"));
        for m in crate::models::WHISPER_MODELS {
            let installed = cache
                .as_ref()
                .and_then(|c| crate::models::whisper_path(m.id, c).ok())
                .map(|p| p.exists())
                .unwrap_or(false);
            let size = cache
                .as_ref()
                .and_then(|c| std::fs::metadata(c.join(m.filename)).ok())
                .map(|md| md.len())
                .unwrap_or_else(|| crate::models::whisper_approx_size(m.id));
            rows.push(ModelRow {
                header: false,
                kind: ModelKind::Whisper,
                id: m.id.to_string(),
                installed,
                is_default: m.id == asr_default,
                size,
            });
        }

        let llm_default = self.global.backend.model.clone().unwrap_or_default();
        rows.push(header_row("Ollama · language model"));
        let mut seen = std::collections::HashSet::new();
        for (name, size) in crate::models::OLLAMA_KNOWN_SIZES {
            seen.insert(*name);
            rows.push(ModelRow {
                header: false,
                kind: ModelKind::Ollama,
                id: (*name).to_string(),
                installed: false,
                is_default: *name == llm_default,
                size: *size,
            });
        }
        if !llm_default.is_empty() && !seen.contains(llm_default.as_str()) {
            rows.push(ModelRow {
                header: false,
                kind: ModelKind::Ollama,
                id: llm_default,
                installed: true,
                is_default: true,
                size: 0,
            });
        }
        rows
    }

    /// Move the model-manager cursor by `delta`, skipping section headers.
    pub(super) fn models_move(&mut self, delta: i32) {
        let Some(s) = &mut self.models else { return };
        let n = s.rows.len() as i32;
        let mut i = s.cursor as i32;
        loop {
            i += delta;
            if i < 0 || i >= n {
                return;
            }
            if !s.rows[i as usize].header {
                s.cursor = i as usize;
                return;
            }
        }
    }

    /// The (kind, id) of the highlighted model row, if any.
    fn selected_model(&self) -> Option<(ModelKind, String)> {
        let s = self.models.as_ref()?;
        s.rows.get(s.cursor).filter(|r| !r.header).map(|r| (r.kind, r.id.clone()))
    }

    /// Set the highlighted model as the default (ASR or LLM) and persist it.
    pub(super) fn set_model_default(&mut self) {
        let Some((kind, id)) = self.selected_model() else { return };
        match kind {
            ModelKind::Whisper => self.global.asr.model = Some(id.clone()),
            ModelKind::Ollama => self.global.backend.model = Some(id.clone()),
        }
        self.global.save().ok();
        self.status = format!("Default set: {id} (saved)");
        self.refresh_model_rows();
    }

    /// Rebuild rows in place (preserving cursor/scroll) after a change.
    pub(super) fn refresh_model_rows(&mut self) {
        let rows = self.build_model_rows();
        if let Some(s) = &mut self.models {
            s.cursor = s.cursor.min(rows.len().saturating_sub(1));
            s.rows = rows;
        }
    }

    /// Queue an install/update (`install = true`) or delete of a specific row.
    pub(super) fn model_action_at(&mut self, row_idx: usize, install: bool) {
        if let Some(s) = &mut self.models {
            if row_idx < s.rows.len() && !s.rows[row_idx].header {
                s.cursor = row_idx;
            }
        }
        self.model_action(install);
    }

    /// Queue an install/update or delete of the highlighted model. Multiple
    /// actions can be queued; they run one at a time in the Working pane.
    pub(super) fn model_action(&mut self, install: bool) {
        let Some((kind, id)) = self.selected_model() else { return };
        let job = match (kind, install) {
            (ModelKind::Whisper, true) => ModelJob::PullWhisper(id.clone()),
            (ModelKind::Whisper, false) => ModelJob::DeleteWhisper(id.clone()),
            (ModelKind::Ollama, true) => ModelJob::PullOllama(id.clone()),
            (ModelKind::Ollama, false) => ModelJob::DeleteOllama(id.clone()),
        };
        let title = format!("{} {id}", if install { "Install" } else { "Delete" });
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
        self.job_title = title.clone();
        self.status = format!("Running: {title}");
        jobs::spawn_model(&self.handle, tx, self.global.clone(), job);
    }
}

/// Small builder used to hand a job over to [`App::start_job`].
pub(super) struct JobRequestBuilder {
    pub(super) title: String,
    pub(super) kind: JobKind,
    pub(super) sessions: Vec<SessionInput>,
    pub(super) transcripts: Vec<PathBuf>,
    pub(super) artifacts: Vec<Artifact>,
}

/// The campaign's config file stem, used as its stable id for ordering.
fn campaign_stem(path: &std::path::Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn header_row(title: &str) -> ModelRow {
    ModelRow {
        header: true,
        kind: ModelKind::Whisper,
        id: title.to_string(),
        installed: false,
        is_default: false,
        size: 0,
    }
}
