//! TUI application state and behaviour. Rendering lives in [`super::draw`];
//! background work lives in [`super::jobs`].

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

use super::jobs::{self, JobKind, JobRequest};
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
    /// Clickable footer entries: `(start_x, end_x, command)`.
    pub footer_hits: Vec<(u16, u16, FooterCmd)>,
    /// The live-job log pane.
    pub job: Rect,
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
            viewer_lines: Vec::new(),
            viewer_scroll: 0,
            job_running: false,
            job_title: String::new(),
            job_log: Vec::new(),
            job_rx: None,
            job_scroll: 0,
            job_follow: true,
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
        self.campaigns = entries;
        if self.campaign_idx >= self.campaigns.len() {
            self.campaign_idx = 0;
        }
        self.camp_state.select(if self.campaigns.is_empty() { None } else { Some(self.campaign_idx) });
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
                    UiEvent::JobDone(res) => done = Some(res),
                }
            }
        }
        if let Some(res) = done {
            self.job_running = false;
            self.job_rx = None;
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
            if let Some(si) = self.open_session {
                if si < self.sessions.len() {
                    self.refresh_viewer();
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
}

/// Small builder used to hand a job over to [`App::start_job`].
pub(super) struct JobRequestBuilder {
    pub(super) title: String,
    pub(super) kind: JobKind,
    pub(super) sessions: Vec<SessionInput>,
    pub(super) transcripts: Vec<PathBuf>,
    pub(super) artifacts: Vec<Artifact>,
}
