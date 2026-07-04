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
    UpdateOllama,
    RerunReplace,
    RerunKeepBoth,
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
            Action::UpdateOllama => "Update Ollama — run the official installer",
            Action::RerunReplace => "Re-run session — regenerate & replace artifacts",
            Action::RerunKeepBoth => "Re-run session — keep both to compare",
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
            Action::UpdateOllama,
            Action::RerunReplace,
            Action::RerunKeepBoth,
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
#[derive(Clone, Copy, PartialEq, Default)]
pub enum ModelKind {
    #[default]
    Whisper,
    Ollama,
    /// A modern ASR engine run via the uv bridge (faster-whisper, Parakeet,
    /// Canary, Voxtral). Selectable as the default; prepared lazily on first use.
    Asr,
}

#[derive(Default)]
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
    /// A shell command to run with the TUI suspended (e.g. the Ollama updater):
    /// `(title, command)`.
    pub pending_shell: Option<(String, String)>,
    /// Background audio player process (ffplay/mpv) for quote playback.
    pub audio_child: Option<std::process::Child>,
    /// Human status of the audio player while active (e.g. `▶ 12:34`).
    pub audio_status: Option<String>,

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
    /// Ordered high-level pipeline phases seen during the current job (for the
    /// animated stage timeline). The last entry is the active phase.
    pub job_stages: Vec<String>,
    /// When the current job started, for the elapsed-time readout.
    pub job_started: Option<std::time::Instant>,
    /// Frozen elapsed time of the last-finished job (so the readout stops
    /// counting once the job is done).
    pub job_elapsed: Option<std::time::Duration>,
    /// Animation frame counter (advances each draw) for the working spinner.
    pub tick: u64,
    /// Pending model jobs waiting for the current one to finish.
    pub job_queue: VecDeque<(ModelJob, String)>,
    /// When set, the content area shows the interactive model manager.
    pub models: Option<ModelsState>,
    /// Receives locally-installed Ollama models `(name, size)` to enrich the list.
    pub model_refresh_rx: Option<UnboundedReceiver<Vec<(String, u64)>>>,

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
            pending_shell: None,
            audio_child: None,
            audio_status: None,
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
            viewing_candidate: false,
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
            job_stages: Vec::new(),
            job_started: None,
            job_elapsed: None,
            tick: 0,
            job_queue: VecDeque::new(),
            models: None,
            model_refresh_rx: None,
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
        let base = cfg.notes_dir().join(&sess.stem);
        // Show the candidate version when toggled and one exists.
        let path = if self.viewing_candidate {
            let c = base.join(crate::pipeline::artifact_file(art, true));
            if c.exists() { c } else { base.join(art.filename()) }
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
            return self.campaign.as_ref().map(|c| c.notes_dir().join("_campaign-log.md"));
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

    /// Source audio file recorded for the open session, if it still exists.
    fn session_source_audio(&self) -> Option<PathBuf> {
        let si = self.open_session?;
        let sess = self.sessions.get(si)?;
        let cfg = self.campaign.as_ref()?;
        let meta = crate::meta::load(&cfg.transcripts_dir(), &sess.stem)?;
        let audio = meta.source_audio?;
        audio.exists().then_some(audio)
    }

    /// The first `[HH:MM:SS]` timestamp at or below the current scroll position
    /// (wrapping to the top), in seconds.
    fn current_quote_timestamp(&self) -> Option<f64> {
        let start = (self.viewer_scroll as usize).min(self.viewer_lines.len());
        self.viewer_lines[start..]
            .iter()
            .chain(self.viewer_lines[..start].iter())
            .find_map(|l| parse_hms_bracket(l))
    }

    /// Toggle audio playback: stop if playing, else play the source audio from
    /// the timestamp of the quote nearest the top of the view.
    pub(super) fn toggle_play_quote(&mut self) {
        if self.audio_child.is_some() {
            self.stop_audio();
            return;
        }
        let Some(secs) = self.current_quote_timestamp() else {
            self.status = "No timestamp in view to play from".into();
            return;
        };
        let Some(audio) = self.session_source_audio() else {
            self.status = "No source audio recorded for this session (re-transcribe to enable playback)".into();
            return;
        };
        match spawn_player(&audio, secs) {
            Ok(child) => {
                self.audio_child = Some(child);
                self.audio_status = Some(format!("▶ {}", fmt_hms(secs)));
                self.status = format!("▶ playing from {} — press p to stop", fmt_hms(secs));
            }
            Err(e) => self.status = format!("audio player unavailable: {e}"),
        }
    }

    /// Stop any active playback.
    pub(super) fn stop_audio(&mut self) {
        if let Some(mut c) = self.audio_child.take() {
            let _ = c.kill();
            let _ = c.wait();
            self.status = "⏹ stopped".into();
        }
        self.audio_status = None;
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

    /// Toggle between the kept artifact and its candidate.
    pub(super) fn toggle_candidate_view(&mut self) {
        if !self.has_candidate() {
            return;
        }
        self.viewing_candidate = !self.viewing_candidate;
        self.refresh_viewer();
        self.status = if self.viewing_candidate {
            "showing NEW candidate — a keep · x discard · c compare".into()
        } else {
            "showing current version — c compare".into()
        };
    }

    /// Accept the candidate: replace the kept artifact with it.
    pub(super) fn accept_candidate(&mut self) {
        let (Some(cand), Some(real)) = (self.candidate_path(), self.current_artifact_path()) else {
            return;
        };
        if !cand.exists() {
            return;
        }
        if let Err(e) = std::fs::rename(&cand, &real) {
            self.status = format!("keep failed: {e}");
            return;
        }
        self.viewing_candidate = false;
        if let (Some(si), tab) = (self.open_session, self.artifact_tab) {
            if let Some(sess) = self.sessions.get_mut(si) {
                if let Some(flag) = sess.artifacts.get_mut(tab) {
                    *flag = true;
                }
            }
        }
        self.status = "kept new version (candidate promoted)".into();
        self.refresh_viewer();
    }

    /// Discard the candidate, keeping the current artifact.
    pub(super) fn discard_candidate(&mut self) {
        if !self.has_candidate() {
            return;
        }
        if let Some(cand) = self.candidate_path() {
            let _ = std::fs::remove_file(&cand);
        }
        self.viewing_candidate = false;
        self.status = "discarded new version".into();
        self.refresh_viewer();
    }

    // ---- job events ------------------------------------------------------

    pub fn drain_job_events(&mut self) {
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
                    UiEvent::Phase(name) => {
                        if self.job_stages.last().map(|s| s != &name).unwrap_or(true) {
                            self.job_stages.push(name.clone());
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
            // Refresh data so new transcripts/artifacts appear.
            let camp_idx = self.campaign_idx;
            self.load_campaign_data();
            self.campaign_idx = camp_idx;
            if self.models.is_some() {
                self.refresh_model_rows();
                self.spawn_model_refresh();
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
        self.job_stages.clear();
        self.job_started = Some(std::time::Instant::now());
        self.job_elapsed = None;
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
            force_transcribe: false,
            resume: true,
            update_log: true,
            candidate: false,
            model_override: None,
        };
        jobs::spawn(&self.handle, tx, req);
    }

    /// Re-run the pipeline for one existing session. Transcription is reused
    /// when the ASR model is unchanged (only re-runs if the model differs or
    /// no audio metadata exists). Regenerates the selected `artifacts`; in
    /// `candidate` mode they are written as `.candidate` files for comparison
    /// and the campaign log is left untouched until kept.
    pub(super) fn start_rerun(
        &mut self,
        transcript: PathBuf,
        artifacts: Vec<Artifact>,
        candidate: bool,
    ) {
        let Some((cfg, preset)) = self.require_ready() else { return };
        let stem = transcript
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        // Source audio (if recorded and still present) lets us re-transcribe
        // when the model changed; otherwise we work from the existing transcript.
        let audio = crate::meta::load(&cfg.transcripts_dir(), &stem)
            .and_then(|m| m.source_audio)
            .filter(|p| p.exists());
        let (kind, sessions, transcripts) = match audio {
            Some(a) => (
                JobKind::Run,
                vec![SessionInput { files: vec![a], name: stem.clone() }],
                Vec::new(),
            ),
            None => (JobKind::Notes, Vec::new(), vec![transcript]),
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
        let title = if candidate {
            format!("Re-run {stem} (compare)")
        } else {
            format!("Re-run {stem} (replace)")
        };
        self.job_title = title.clone();
        self.status = format!("Running: {title}");

        let req = JobRequest {
            kind,
            g: self.global.clone(),
            campaign: cfg,
            preset,
            asr_model: self.asr_model(),
            sessions,
            transcripts,
            artifacts,
            force: true,          // regenerate the selected artifacts
            force_transcribe: false, // reuse transcript when the model matches
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
        self.models = Some(ModelsState { rows, cursor, scroll: 0, expanded, installed });
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
        rows.push(header_row("Whisper · speech-to-text"));
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

        // --- Advanced ASR engines (via uv bridge; select as default only) ---
        rows.push(header_row("Transcription (ASR) · advanced engines"));
        rows.push(col_header_row());
        for m in crate::asr::ASR_CATALOG {
            if !m.engine.is_bridge() {
                continue; // whisper.cpp ggml models are listed above
            }
            rows.push(ModelRow {
                kind: ModelKind::Asr,
                display: m.display.to_string(),
                id: m.id.to_string(),
                installed: false,
                is_default: m.id == asr_default,
                size: m.size,
                released: m.released.to_string(),
                ..Default::default()
            });
        }

        // --- Ollama (expandable families) ---
        let llm_default = self.global.backend.model.clone().unwrap_or_default();
        rows.push(header_row("Ollama · language model"));
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
        let Some((kind, id)) = self.selected_model() else { return };
        match kind {
            ModelKind::Whisper => self.global.asr.model = Some(id.clone()),
            ModelKind::Asr => self.global.asr.model = Some(id.clone()),
            ModelKind::Ollama => self.global.backend.model = Some(id.clone()),
        }
        self.global.save().ok();
        self.status = format!("Default set: {id} (saved)");
        self.refresh_model_rows();
    }

    /// Expand/collapse the highlighted family row.
    pub(super) fn toggle_models_expand(&mut self) {
        let key = match &self.models {
            Some(s) => s.rows.get(s.cursor).filter(|r| r.family).map(|r| r.expand_key.clone()),
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
        let Some((kind, id)) = self.selected_model() else { return };
        let job = match (kind, install) {
            (ModelKind::Whisper, true) => ModelJob::PullWhisper(id.clone()),
            (ModelKind::Whisper, false) => ModelJob::DeleteWhisper(id.clone()),
            (ModelKind::Ollama, true) => ModelJob::PullOllama(id.clone()),
            (ModelKind::Ollama, false) => ModelJob::DeleteOllama(id.clone()),
            (ModelKind::Asr, _) => {
                self.status = format!(
                    "{id}: prepared automatically on first transcription — press ⏎ to set as default"
                );
                return;
            }
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
        self.job_stages.clear();
        self.job_started = Some(std::time::Instant::now());
        self.job_elapsed = None;
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

/// Format seconds as `H:MM:SS` (or `M:SS` under an hour).
fn fmt_hms(secs: f64) -> String {
    let s = secs as u64;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

/// Spawn a detached, headless audio player seeking to `secs`. Prefers `ffplay`
/// (ships with ffmpeg); falls back to `mpv`.
fn spawn_player(audio: &std::path::Path, secs: f64) -> std::io::Result<std::process::Child> {
    use std::process::{Command, Stdio};
    let bin_exists = |b: &str| {
        Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {b}"))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    let seek = format!("{secs:.3}");
    if bin_exists("ffplay") {
        return Command::new("ffplay")
            .args(["-nodisp", "-autoexit", "-loglevel", "quiet", "-ss", &seek])
            .arg(audio)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    if bin_exists("mpv") {
        return Command::new("mpv")
            .args(["--no-video", "--really-quiet", &format!("--start={seek}")])
            .arg(audio)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no audio player found (install ffmpeg for ffplay, or mpv)",
    ))
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
