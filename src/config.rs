//! Configuration: global (`~/.config/sessionsmith/config.toml`) + per-repo
//! `campaign.toml`. CLI flags override campaign which overrides preset defaults.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table};

use crate::hardware::HardwareProfile;

// ---------------------------------------------------------------------------
// Global config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalConfig {
    #[serde(default)]
    pub backend: BackendConfig,
    #[serde(default)]
    pub asr: AsrConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub desktop: DesktopConfig,
    #[serde(default)]
    pub hardware: Option<HardwareProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopConfig {
    #[serde(default = "default_date_format")]
    pub date_format: String,
    #[serde(default = "default_appearance")]
    pub appearance: String,
    #[serde(default = "default_desktop_theme")]
    pub theme: String,
    #[serde(default = "default_volume")]
    pub player_volume: u8,
    #[serde(default)]
    pub onboarding_completed_version: u32,
    #[serde(default)]
    pub onboarding_outcome: String,
}

fn default_date_format() -> String {
    "dmy".into()
}

fn default_appearance() -> String {
    "system".into()
}

fn default_desktop_theme() -> String {
    "default".into()
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            date_format: default_date_format(),
            appearance: default_appearance(),
            theme: default_desktop_theme(),
            player_volume: default_volume(),
            onboarding_completed_version: 0,
            onboarding_outcome: String::new(),
        }
    }
}

/// Full-screen TUI preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Name of the active theme (built-in or a user theme in
    /// `~/.config/sessionsmith/themes/*.toml`).
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Use the legacy line-based menu instead of the full-screen TUI when
    /// launched with no subcommand.
    #[serde(default)]
    pub legacy_menu: bool,
    /// Persisted campaign ordering for the TUI sidebar, by campaign file stem
    /// (e.g. `["Emberfall", "DnDThursday"]`). Campaigns not listed here are
    /// appended in alphabetical order.
    #[serde(default)]
    pub campaign_order: Vec<String>,
    /// Last audio-player volume (0–100). Starts at 50 and persists across runs.
    #[serde(default = "default_volume")]
    pub player_volume: u8,
    /// Enable terminal mouse capture for the full-screen TUI by default.
    #[serde(default = "default_mouse")]
    pub mouse: bool,
    /// Show a desktop notification after a TUI job that ran for at least a minute.
    #[serde(default)]
    pub notify: bool,
}

fn default_theme() -> String {
    "midnight".into()
}

fn default_volume() -> u8 {
    50
}

fn default_mouse() -> bool {
    true
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            legacy_menu: false,
            campaign_order: Vec::new(),
            player_volume: default_volume(),
            mouse: default_mouse(),
            notify: false,
        }
    }
}

/// Base input/output directories. Relative paths are resolved against the
/// current working directory; `~` is expanded to the user's home.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    /// Directory scanned for input recordings.
    #[serde(default = "default_audio_dir")]
    pub audio_dir: PathBuf,
    /// Directory containing per-campaign TOML definitions.
    #[serde(default = "default_campaigns_dir")]
    pub campaigns_dir: PathBuf,
    /// Root directory for all generated output (per-campaign subdirs live here).
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,
}

fn default_audio_dir() -> PathBuf {
    PathBuf::from("audio")
}
fn default_campaigns_dir() -> PathBuf {
    PathBuf::from("campaigns")
}
fn default_output_dir() -> PathBuf {
    PathBuf::from("output")
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            audio_dir: default_audio_dir(),
            campaigns_dir: default_campaigns_dir(),
            output_dir: default_output_dir(),
        }
    }
}

// ---------------------------------------------------------------------------
// Process-global resolved paths. Set whenever the global config is loaded, so
// `CampaignConfig` path helpers and the audio scanner honour user overrides
// without threading the config through every call site.
// ---------------------------------------------------------------------------

static PATHS: once_cell::sync::Lazy<std::sync::RwLock<PathsConfig>> =
    once_cell::sync::Lazy::new(|| std::sync::RwLock::new(PathsConfig::default()));

#[cfg(unix)]
static PERMISSIVE_SECRET_WARNING_EMITTED: AtomicBool = AtomicBool::new(false);

fn set_global_paths(paths: &PathsConfig) {
    let resolved = PathsConfig {
        audio_dir: expand_tilde(&paths.audio_dir),
        campaigns_dir: expand_tilde(&paths.campaigns_dir),
        output_dir: expand_tilde(&paths.output_dir),
    };
    if let Ok(mut guard) = PATHS.write() {
        *guard = resolved;
    }
}

/// The configured audio input directory (default `audio/`).
pub fn audio_dir() -> PathBuf {
    PATHS
        .read()
        .map(|p| p.audio_dir.clone())
        .unwrap_or_else(|_| default_audio_dir())
}

/// The configured campaign definition directory (default `campaigns/`).
pub fn campaigns_dir() -> PathBuf {
    PATHS
        .read()
        .map(|paths| paths.campaigns_dir.clone())
        .unwrap_or_else(|_| default_campaigns_dir())
}

/// The configured output root directory (default `output/`).
pub fn output_dir() -> PathBuf {
    PATHS
        .read()
        .map(|p| p.output_dir.clone())
        .unwrap_or_else(|_| default_output_dir())
}

/// Expand a leading `~` to the user's home directory.
fn expand_tilde(p: &Path) -> PathBuf {
    if let Ok(stripped) = p.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    p.to_path_buf()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// One of: "ollama", "openai", "anthropic".
    pub kind: String,
    /// Base URL override (e.g. for OpenRouter / vLLM / local Ollama).
    pub base_url: Option<String>,
    /// API key. Supports `${ENV_VAR}` interpolation.
    pub api_key: Option<String>,
    /// Default model id for this backend.
    pub model: Option<String>,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            kind: "ollama".into(),
            base_url: Some("http://localhost:11434".into()),
            api_key: None,
            model: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AsrConfig {
    /// Path to the whisper.cpp `whisper-cli` (or compatible) binary.
    pub binary: Option<PathBuf>,
    /// Default model name (e.g. "large-v3", "medium", "base").
    pub model: Option<String>,
    /// Directory to cache downloaded ggml models.
    pub model_dir: Option<PathBuf>,
    /// Threads passed to whisper-cli.
    pub threads: Option<u32>,
    /// Enable speaker diarization (whisperX only). Off by default: current local
    /// models frequently mis-attribute the GM and players to one another, which
    /// causes more harm than help. Kept as an opt-in for future, better models.
    #[serde(default)]
    pub diarize: bool,
    /// Hugging Face token for the pyannote diarization models (required by
    /// whisperX diarization). Supports `${ENV_VAR}` interpolation.
    #[serde(default)]
    pub hf_token: Option<String>,
    /// Run an ffmpeg silence-removal (VAD) pre-pass before ASR to skip long
    /// gaps. Off by default. Speeds up long, gap-heavy recordings.
    #[serde(default)]
    pub vad: bool,
    /// Force the whisperX compute device: `"cuda"`, `"cpu"`, or unset for
    /// auto-detect (CUDA when ≥4 GB VRAM is free, else CPU). Lets non-NVIDIA
    /// users override the `nvidia-smi`-based default.
    #[serde(default)]
    pub device: Option<String>,
    /// ASR engine preference: `"local"` (in-process whisper-rs), `"whisper-cli"`
    /// (whisper.cpp binary), `"whisperx"`, or unset/`"auto"`. Auto prefers the
    /// built-in local engine when compiled with the `local-whisper` feature,
    /// then falls back to whichever external binary is found.
    #[serde(default)]
    pub engine: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// If true, run derived LLM passes concurrently (recommended for API
    /// backends, off by default for local Ollama).
    #[serde(default)]
    pub parallel_passes: bool,
    /// Per-request timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// If true, allow thinking/reasoning models (e.g. Qwen3) to use their
    /// internal chain-of-thought. Off by default because it makes generation
    /// 10-50× slower for minimal quality gain on extraction tasks.
    #[serde(default)]
    pub think: bool,
    /// Override the LLM context window (Ollama `num_ctx`). When `None`, it is
    /// derived from the detected hardware tier's recommended context hint.
    /// Without this, Ollama silently falls back to a small default context and
    /// truncates long transcripts.
    #[serde(default)]
    pub num_ctx: Option<u32>,
    /// Split long transcripts into overlapping windows for the bullets pass so
    /// they never exceed the model context. On by default.
    #[serde(default = "default_true")]
    pub chunk: bool,
    /// Character overlap between consecutive transcript chunks.
    #[serde(default = "default_chunk_overlap")]
    pub chunk_overlap_chars: usize,
    /// Also emit machine-readable JSON companions (e.g. `dm-notes.json`) using
    /// the backend's structured-output mode. Off by default.
    #[serde(default)]
    pub structured: bool,
    /// Maintain a per-campaign SQLite index of sessions and artifacts for
    /// cross-session search (`sessionsmith search`). On by default.
    #[serde(default = "default_true")]
    pub index: bool,
    /// Automatically free GPU VRAM held by the LLM backend (unload Ollama
    /// models) before transcription and after notes generation, so the ASR and
    /// LLM stages don't fight over VRAM. On by default; harmless no-op for
    /// remote API backends.
    #[serde(default = "default_true")]
    pub auto_free_vram: bool,
}

fn default_timeout() -> u64 {
    1800
}
fn default_true() -> bool {
    true
}
fn default_chunk_overlap() -> usize {
    1000
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            parallel_passes: false,
            timeout_secs: default_timeout(),
            think: false,
            num_ctx: None,
            chunk: true,
            chunk_overlap_chars: default_chunk_overlap(),
            structured: false,
            index: true,
            auto_free_vram: true,
        }
    }
}

impl GlobalConfig {
    pub fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .ok_or_else(|| anyhow!("could not resolve XDG config dir"))?
            .join("sessionsmith");
        Ok(dir.join("config.toml"))
    }

    pub fn load_or_default() -> Result<Self> {
        let p = Self::path()?;
        let cfg = if p.exists() {
            let text =
                std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
            let config =
                toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
            warn_if_permissive_secret_file(&p, &config);
            config
        } else {
            Self::default()
        };
        // Publish resolved input/output paths for the rest of the process.
        set_global_paths(&cfg.paths);
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path()?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&p, text).with_context(|| format!("writing {}", p.display()))?;
        set_private_permissions(&p)?;
        Ok(())
    }

    /// Resolve API key, expanding `${VAR}` shell-style.
    pub fn resolved_api_key(&self) -> Option<String> {
        self.backend.api_key.as_ref().map(|raw| expand_env(raw))
    }

    /// Resolve the Hugging Face token for diarization, expanding `${VAR}`.
    pub fn resolved_hf_token(&self) -> Option<String> {
        self.asr
            .hf_token
            .as_ref()
            .map(|raw| expand_env(raw))
            .filter(|s| !s.is_empty())
    }

    /// Effective LLM context window: explicit `[runtime] num_ctx` override, else
    /// the detected hardware tier's recommended context hint, else `None`.
    pub fn effective_num_ctx(&self) -> Option<u32> {
        self.runtime.num_ctx.or_else(|| {
            self.hardware
                .as_ref()
                .map(|hw| crate::hardware::recommend(hw).llm_context_hint)
        })
    }
}

#[cfg(unix)]
fn has_inline_secret(value: Option<&String>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty() && !value.trim_start().starts_with("${"))
}

#[cfg(unix)]
fn warn_if_permissive_secret_file(path: &Path, config: &GlobalConfig) {
    use std::os::unix::fs::PermissionsExt;

    if !has_inline_secret(config.backend.api_key.as_ref())
        && !has_inline_secret(config.asr.hf_token.as_ref())
    {
        return;
    }
    let is_permissive = std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o077 != 0)
        .unwrap_or(false);
    if is_permissive && !PERMISSIVE_SECRET_WARNING_EMITTED.swap(true, Ordering::Relaxed) {
        crate::ui::warn(&format!(
            "{} contains inline secrets and is readable by other users; run chmod 600 {}",
            path.display(),
            path.display()
        ));
    }
}

#[cfg(not(unix))]
fn warn_if_permissive_secret_file(_: &Path, _: &GlobalConfig) {}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_permissions(_: &Path) -> Result<()> {
    Ok(())
}

fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next();
            let mut name = String::new();
            while let Some(&n) = chars.peek() {
                chars.next();
                if n == '}' {
                    break;
                }
                name.push(n);
            }
            if let Ok(v) = std::env::var(&name) {
                out.push_str(&v);
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Campaign config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CampaignConfig {
    /// Source file used only for local cache keys; never serialized into TOML.
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
    pub campaign: Campaign,
    /// Per-campaign backend settings. Specified fields override global config.
    #[serde(default)]
    pub backend: CampaignBackendConfig,
    /// Per-campaign ASR settings. Specified fields override global config.
    #[serde(default)]
    pub asr: CampaignAsrConfig,
    #[serde(default)]
    pub players: Vec<Player>,
    #[serde(default)]
    pub transcription: TranscriptionConfig,
    #[serde(default)]
    pub system: SystemRef,
    #[serde(default)]
    pub outputs: OutputsConfig,
    #[serde(default)]
    pub prompts: PromptOverrides,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CampaignBackendConfig {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CampaignAsrConfig {
    #[serde(default)]
    pub binary: Option<PathBuf>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub model_dir: Option<PathBuf>,
    #[serde(default)]
    pub threads: Option<u32>,
    #[serde(default)]
    pub diarize: Option<bool>,
    #[serde(default)]
    pub hf_token: Option<String>,
    #[serde(default)]
    pub vad: Option<bool>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub engine: Option<String>,
}

/// Merge global defaults with per-campaign overrides. Command-line flags are
/// applied by callers afterwards and therefore remain highest precedence.
pub fn effective(global: &GlobalConfig, campaign: &CampaignConfig) -> GlobalConfig {
    let mut resolved = global.clone();
    let backend = &campaign.backend;
    if let Some(value) = &backend.kind {
        resolved.backend.kind = value.clone();
    }
    if let Some(value) = &backend.base_url {
        resolved.backend.base_url = Some(value.clone());
    }
    if let Some(value) = &backend.api_key {
        resolved.backend.api_key = Some(value.clone());
    }
    if let Some(value) = &backend.model {
        resolved.backend.model = Some(value.clone());
    }

    let asr = &campaign.asr;
    if let Some(value) = &asr.binary {
        resolved.asr.binary = Some(value.clone());
    }
    if let Some(value) = &asr.model {
        resolved.asr.model = Some(value.clone());
    }
    if let Some(value) = &asr.model_dir {
        resolved.asr.model_dir = Some(value.clone());
    }
    if let Some(value) = asr.threads {
        resolved.asr.threads = Some(value);
    }
    if let Some(value) = asr.diarize {
        resolved.asr.diarize = value;
    }
    if let Some(value) = &asr.hf_token {
        resolved.asr.hf_token = Some(value.clone());
    }
    if let Some(value) = asr.vad {
        resolved.asr.vad = value;
    }
    if let Some(value) = &asr.device {
        resolved.asr.device = Some(value.clone());
    }
    if let Some(value) = &asr.engine {
        resolved.asr.engine = Some(value.clone());
    }
    resolved
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Campaign {
    pub name: String,
    #[serde(default)]
    pub gm: String,
    #[serde(default)]
    pub setting: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Player {
    pub player: String,
    pub character: String,
    #[serde(default)]
    pub ancestry: String,
    #[serde(default)]
    pub class: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignTextReplacement {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone)]
pub struct CampaignEditableSettings {
    pub players: Vec<Player>,
    pub vocabulary: Vec<String>,
    pub replacements: Vec<CampaignTextReplacement>,
}

#[derive(Debug, Clone)]
pub struct CampaignEditableSettingsResult {
    pub config: CampaignConfig,
    pub revision: String,
}

#[derive(Debug, Clone)]
pub struct CampaignEditableIdentity {
    pub gm: String,
    pub setting: String,
    pub notes: String,
}

#[derive(Debug, Clone)]
pub struct CampaignEditableSpeaker {
    pub label: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct CampaignEditableOutputs {
    pub default: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CampaignEditableSystem {
    pub preset: String,
    pub overrides: String,
}

#[derive(Debug, Clone)]
pub struct CampaignEditableBackend {
    pub kind: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CampaignEditableAsr {
    pub model: Option<String>,
    pub threads: Option<u32>,
    pub diarize: Option<bool>,
    pub vad: Option<bool>,
    pub device: Option<String>,
    pub engine: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CampaignEditablePrompts {
    pub bullets: Option<String>,
    pub dm_notes: Option<String>,
    pub recap: Option<String>,
    pub summary: Option<String>,
    pub story: Option<String>,
    pub quotes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DesktopSettingsResult {
    pub settings: DesktopConfig,
    pub paths: PathsConfig,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionConfig {
    /// Literal, case-sensitive corrections applied longest-first to transcript
    /// TXT and SRT output. Useful for fictional names unsupported by an ASR
    /// engine's vocabulary prompting interface.
    #[serde(default)]
    pub replacements: BTreeMap<String, String>,
    /// Extra proper nouns and terms used to bias supported ASR engines.
    #[serde(default)]
    pub vocabulary: Vec<String>,
    /// Whether to build an initial ASR prompt from campaign vocabulary.
    #[serde(default = "default_vocab_prompt")]
    pub vocab_prompt: bool,
    /// Default names for diarization labels, confirmed or overridden per session.
    #[serde(default)]
    pub speakers: BTreeMap<String, String>,
}

fn default_vocab_prompt() -> bool {
    true
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            replacements: BTreeMap::new(),
            vocabulary: Vec::new(),
            vocab_prompt: default_vocab_prompt(),
            speakers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemRef {
    /// Preset name (must match a bundled preset).
    pub preset: String,
    /// Free-form overrides appended after the preset block in prompts.
    #[serde(default)]
    pub overrides: String,
}

impl Default for SystemRef {
    fn default() -> Self {
        Self {
            preset: "generic".into(),
            overrides: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputsConfig {
    #[serde(default = "default_artifacts")]
    pub default: Vec<String>,
}

fn default_artifacts() -> Vec<String> {
    vec![
        "bullets".into(),
        "dm-notes".into(),
        "recap".into(),
        "summary".into(),
        "story".into(),
        "quotes".into(),
    ]
}

impl Default for OutputsConfig {
    fn default() -> Self {
        Self {
            default: default_artifacts(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptOverrides {
    #[serde(default)]
    pub bullets: Option<String>,
    #[serde(default)]
    pub dm_notes: Option<String>,
    #[serde(default)]
    pub recap: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub story: Option<String>,
    #[serde(default)]
    pub quotes: Option<String>,
}

impl CampaignConfig {
    pub fn default_path() -> PathBuf {
        PathBuf::from("campaign.toml")
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with_revision(path).map(|(config, _revision)| config)
    }

    /// Load a campaign config and the exact content revision that produced it.
    /// Mutating callers can return the revision to a client and reject a later
    /// write when the file changed meanwhile.
    pub fn load_with_revision(path: &Path) -> Result<(Self, String)> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading campaign config: {}", path.display()))?;
        let text = std::str::from_utf8(&bytes)
            .with_context(|| format!("campaign config is not UTF-8: {}", path.display()))?;
        let mut cfg: Self =
            toml::from_str(text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.source_path = std::fs::canonicalize(path)
            .ok()
            .or_else(|| Some(path.to_path_buf()));
        Ok((cfg, crate::util::content_revision(&bytes)))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Filesystem-safe slug derived from the campaign name.
    /// e.g. "My Game" → "my-game", "Curse of Strahd" → "curse-of-strahd"
    pub fn slug(&self) -> String {
        crate::util::slugify(&self.campaign.name)
    }

    /// Root output directory for this campaign: `<output_dir>/<slug>/`
    pub fn output_root(&self) -> PathBuf {
        output_dir().join(self.slug())
    }

    /// Directory for transcripts: `output/<slug>/transcripts/`
    pub fn transcripts_dir(&self) -> PathBuf {
        self.output_root().join("transcripts")
    }

    /// Directory for LLM notes: `output/<slug>/notes/`
    pub fn notes_dir(&self) -> PathBuf {
        self.output_root().join("notes")
    }

    /// Render the campaign context block injected into prompts.
    pub fn render_context(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("Campaign: {}\n", self.campaign.name));
        if !self.campaign.gm.is_empty() {
            s.push_str(&format!("GM: {}\n", self.campaign.gm));
        }
        if !self.campaign.setting.is_empty() {
            s.push_str(&format!("Setting: {}\n", self.campaign.setting));
        }
        if !self.players.is_empty() {
            s.push_str("Players:\n");
            for p in &self.players {
                let bits: Vec<String> = [&p.ancestry, &p.class]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                let detail = if bits.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", bits.join(" "))
                };
                s.push_str(&format!(
                    "  - {} plays {}{}\n",
                    p.player, p.character, detail
                ));
            }
        }
        if !self.campaign.notes.is_empty() {
            s.push_str(&format!("Notes: {}\n", self.campaign.notes));
        }
        s
    }

    /// Like `render_context` but omits the player/character roster.
    /// Used when generating artifacts (e.g. the recap) that must not map
    /// transcript names onto campaign characters the LLM has not heard in this session.
    pub fn render_context_no_roster(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("Campaign: {}\n", self.campaign.name));
        if !self.campaign.gm.is_empty() {
            s.push_str(&format!("GM: {}\n", self.campaign.gm));
        }
        if !self.campaign.setting.is_empty() {
            s.push_str(&format!("Setting: {}\n", self.campaign.setting));
        }
        if !self.campaign.notes.is_empty() {
            s.push_str(&format!("Notes: {}\n", self.campaign.notes));
        }
        s
    }
}

const MAX_EDITABLE_PLAYERS: usize = 100;
const MAX_EDITABLE_VOCABULARY: usize = 250;
const MAX_EDITABLE_REPLACEMENTS: usize = 250;
const MAX_EDITABLE_TEXT_LENGTH: usize = 240;
const MAX_EDITABLE_NOTES_LENGTH: usize = 8_000;
const MAX_EDITABLE_SPEAKERS: usize = 40;
const MAX_EDITABLE_SPEAKER_NAME_LENGTH: usize = 100;

pub fn read_desktop_settings(path: &Path) -> Result<DesktopSettingsResult> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(&GlobalConfig::default())?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
        set_private_permissions(path)?;
    }
    let contents = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let text = std::str::from_utf8(&contents)
        .with_context(|| format!("global config is not UTF-8: {}", path.display()))?;
    let config: GlobalConfig =
        toml::from_str(text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(DesktopSettingsResult {
        settings: normalize_desktop_settings(config.desktop)?,
        paths: config.paths,
        revision: crate::util::content_revision(&contents),
    })
}

pub fn write_desktop_settings(
    path: &Path,
    expected_revision: &str,
    settings: DesktopConfig,
    paths: PathsConfig,
) -> Result<DesktopSettingsResult> {
    validate_revision(expected_revision)?;
    let original = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if crate::util::content_revision(&original) != expected_revision {
        bail!("app settings changed since they were read");
    }
    let original = std::str::from_utf8(&original)
        .with_context(|| format!("global config is not UTF-8: {}", path.display()))?;
    let mut document = original
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    let settings = normalize_desktop_settings(settings)?;
    let desktop = document
        .as_table_mut()
        .entry("desktop")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| anyhow!("[desktop] must be a TOML table to edit app settings"))?;
    desktop.insert("date_format", value(&settings.date_format));
    desktop.insert("appearance", value(&settings.appearance));
    desktop.insert("theme", value(&settings.theme));
    desktop.insert("player_volume", value(i64::from(settings.player_volume)));
    desktop.insert(
        "onboarding_completed_version",
        value(i64::from(settings.onboarding_completed_version)),
    );
    desktop.insert("onboarding_outcome", value(&settings.onboarding_outcome));

    let paths_table = document
        .as_table_mut()
        .entry("paths")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| anyhow!("[paths] must be a TOML table to edit app settings"))?;
    paths_table.insert(
        "audio_dir",
        value(paths.audio_dir.to_string_lossy().as_ref()),
    );
    paths_table.insert(
        "campaigns_dir",
        value(paths.campaigns_dir.to_string_lossy().as_ref()),
    );
    paths_table.insert(
        "output_dir",
        value(paths.output_dir.to_string_lossy().as_ref()),
    );

    let updated = document.to_string();
    let _: GlobalConfig = toml::from_str(&updated)
        .with_context(|| format!("validating updated {}", path.display()))?;
    match crate::util::atomic_replace_if_revision(path, expected_revision, updated.as_bytes()) {
        Ok(()) => {
            set_global_paths(&paths);
            Ok(DesktopSettingsResult {
                settings,
                paths,
                revision: crate::util::content_revision(updated.as_bytes()),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            bail!("app settings changed since they were read")
        }
        Err(error) => Err(error).with_context(|| format!("writing {}", path.display())),
    }
}

/// Replace a campaign file ID in persisted sidebar ordering without
/// reserializing unrelated global configuration.
pub fn replace_campaign_order_id(path: &Path, old_id: &str, new_id: &str) -> Result<bool> {
    if old_id == new_id || !path.is_file() {
        return Ok(false);
    }
    let original = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let revision = crate::util::content_revision(&original);
    let text = std::str::from_utf8(&original)
        .with_context(|| format!("global config is not UTF-8: {}", path.display()))?;
    let mut document = text
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    let Some(order) = document
        .as_table_mut()
        .get_mut("ui")
        .and_then(|item| item.as_table_mut())
        .and_then(|table| table.get_mut("campaign_order"))
        .and_then(|item| item.as_array_mut())
    else {
        return Ok(false);
    };
    let positions = (0..order.len())
        .filter(|index| order.get(*index).and_then(|value| value.as_str()) == Some(old_id))
        .collect::<Vec<_>>();
    if positions.is_empty() {
        return Ok(false);
    }
    for index in positions {
        order.replace(index, new_id);
    }
    let updated = document.to_string();
    let _: GlobalConfig = toml::from_str(&updated)
        .with_context(|| format!("validating updated {}", path.display()))?;
    crate::util::atomic_replace_if_revision(path, &revision, updated.as_bytes())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

fn normalize_desktop_settings(settings: DesktopConfig) -> Result<DesktopConfig> {
    if !matches!(settings.date_format.as_str(), "dmy" | "mdy" | "ymd" | "iso") {
        bail!("date format must be dmy, mdy, ymd, or iso");
    }
    if !matches!(settings.appearance.as_str(), "system" | "light" | "dark") {
        bail!("appearance must be system, light, or dark");
    }
    let theme = normalize_editable_text(&settings.theme, "theme", true)?;
    if settings.player_volume > 100 {
        bail!("player volume must be between 0 and 100");
    }
    if !matches!(
        settings.onboarding_outcome.as_str(),
        "" | "finished" | "skipped"
    ) {
        bail!("onboarding outcome must be empty, finished, or skipped");
    }
    if settings.onboarding_completed_version == 0 && !settings.onboarding_outcome.is_empty() {
        bail!("onboarding outcome requires a completed onboarding version");
    }
    Ok(DesktopConfig { theme, ..settings })
}

/// Apply the narrow editable-settings surface without reserializing unrelated
/// campaign TOML. The caller supplies the revision obtained while reading the
/// configuration so external edits cannot be overwritten silently.
pub fn write_campaign_editable_settings(
    path: &Path,
    expected_revision: &str,
    settings: CampaignEditableSettings,
) -> Result<CampaignEditableSettingsResult> {
    validate_revision(expected_revision)?;
    let original = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if crate::util::content_revision(&original) != expected_revision {
        bail!("campaign settings changed since they were read");
    }
    let original = std::str::from_utf8(&original)
        .with_context(|| format!("campaign config is not UTF-8: {}", path.display()))?;
    let mut document = original
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    let settings = normalize_editable_settings(settings)?;

    replace_players(&mut document, &settings.players);
    replace_transcription_settings(&mut document, &settings)?;

    let updated = document.to_string();
    let config: CampaignConfig = toml::from_str(&updated)
        .with_context(|| format!("validating updated {}", path.display()))?;
    match crate::util::atomic_replace_if_revision(path, expected_revision, updated.as_bytes()) {
        Ok(()) => Ok(CampaignEditableSettingsResult {
            config,
            revision: crate::util::content_revision(updated.as_bytes()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            bail!("campaign settings changed since they were read")
        }
        Err(error) => Err(error).with_context(|| format!("writing {}", path.display())),
    }
}

pub fn write_campaign_editable_identity(
    path: &Path,
    expected_revision: &str,
    identity: CampaignEditableIdentity,
) -> Result<CampaignEditableSettingsResult> {
    validate_revision(expected_revision)?;
    let original = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if crate::util::content_revision(&original) != expected_revision {
        bail!("campaign settings changed since they were read");
    }
    let original = std::str::from_utf8(&original)
        .with_context(|| format!("campaign config is not UTF-8: {}", path.display()))?;
    let mut document = original
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    let identity = CampaignEditableIdentity {
        gm: normalize_editable_text(&identity.gm, "game master", false)?,
        setting: normalize_editable_text(&identity.setting, "setting", false)?,
        notes: normalize_editable_notes(&identity.notes)?,
    };
    let campaign = document
        .as_table_mut()
        .entry("campaign")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| anyhow!("[campaign] must be a TOML table to edit identity settings"))?;
    campaign.insert("gm", value(&identity.gm));
    campaign.insert("setting", value(&identity.setting));
    campaign.insert("notes", value(&identity.notes));

    let updated = document.to_string();
    let config: CampaignConfig = toml::from_str(&updated)
        .with_context(|| format!("validating updated {}", path.display()))?;
    match crate::util::atomic_replace_if_revision(path, expected_revision, updated.as_bytes()) {
        Ok(()) => Ok(CampaignEditableSettingsResult {
            config,
            revision: crate::util::content_revision(updated.as_bytes()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            bail!("campaign settings changed since they were read")
        }
        Err(error) => Err(error).with_context(|| format!("writing {}", path.display())),
    }
}

pub fn write_campaign_editable_speakers(
    path: &Path,
    expected_revision: &str,
    speakers: Vec<CampaignEditableSpeaker>,
) -> Result<CampaignEditableSettingsResult> {
    validate_revision(expected_revision)?;
    if speakers.len() > MAX_EDITABLE_SPEAKERS {
        bail!("no more than {MAX_EDITABLE_SPEAKERS} speaker defaults may be saved at once");
    }
    let mut normalized = BTreeMap::new();
    for speaker in speakers {
        let name = normalize_editable_text(&speaker.name, "speaker name", true)?;
        if name.chars().count() > MAX_EDITABLE_SPEAKER_NAME_LENGTH {
            bail!("speaker name must be at most {MAX_EDITABLE_SPEAKER_NAME_LENGTH} characters");
        }
        let (label, _) = crate::speakers::parse_mapping(&format!("{}={name}", speaker.label))?;
        if normalized.insert(label, name).is_some() {
            bail!("speaker labels must be unique");
        }
    }

    let original = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if crate::util::content_revision(&original) != expected_revision {
        bail!("campaign settings changed since they were read");
    }
    let original = std::str::from_utf8(&original)
        .with_context(|| format!("campaign config is not UTF-8: {}", path.display()))?;
    let mut document = original
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    let transcription = document
        .as_table_mut()
        .entry("transcription")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| anyhow!("[transcription] must be a TOML table to edit speaker defaults"))?;
    if normalized.is_empty() {
        transcription.remove("speakers");
    } else {
        let mut table = Table::new();
        for (label, name) in normalized {
            table.insert(&label, value(name));
        }
        transcription.insert("speakers", Item::Table(table));
    }

    let updated = document.to_string();
    let config: CampaignConfig = toml::from_str(&updated)
        .with_context(|| format!("validating updated {}", path.display()))?;
    match crate::util::atomic_replace_if_revision(path, expected_revision, updated.as_bytes()) {
        Ok(()) => Ok(CampaignEditableSettingsResult {
            config,
            revision: crate::util::content_revision(updated.as_bytes()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            bail!("campaign settings changed since they were read")
        }
        Err(error) => Err(error).with_context(|| format!("writing {}", path.display())),
    }
}

pub fn write_campaign_editable_outputs(
    path: &Path,
    expected_revision: &str,
    outputs: CampaignEditableOutputs,
) -> Result<CampaignEditableSettingsResult> {
    validate_revision(expected_revision)?;
    let mut seen = BTreeSet::new();
    let mut defaults = Vec::with_capacity(outputs.default.len());
    for output in outputs.default {
        let output = normalize_editable_text(&output, "output artifact", true)?;
        let artifact = crate::prompts::ALL_ARTIFACTS
            .iter()
            .find(|artifact| artifact.id() == output)
            .ok_or_else(|| anyhow!("unknown output artifact '{output}'"))?;
        if !seen.insert(artifact.id()) {
            bail!("output artifacts must be unique");
        }
        defaults.push(artifact.id().to_string());
    }

    let original = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if crate::util::content_revision(&original) != expected_revision {
        bail!("campaign settings changed since they were read");
    }
    let original = std::str::from_utf8(&original)
        .with_context(|| format!("campaign config is not UTF-8: {}", path.display()))?;
    let mut document = original
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    let outputs = document
        .as_table_mut()
        .entry("outputs")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| anyhow!("[outputs] must be a TOML table to edit output defaults"))?;
    let mut values = Array::new();
    for output in defaults {
        values.push(output);
    }
    outputs.insert("default", value(values));

    let updated = document.to_string();
    let config: CampaignConfig = toml::from_str(&updated)
        .with_context(|| format!("validating updated {}", path.display()))?;
    match crate::util::atomic_replace_if_revision(path, expected_revision, updated.as_bytes()) {
        Ok(()) => Ok(CampaignEditableSettingsResult {
            config,
            revision: crate::util::content_revision(updated.as_bytes()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            bail!("campaign settings changed since they were read")
        }
        Err(error) => Err(error).with_context(|| format!("writing {}", path.display())),
    }
}

pub fn write_campaign_editable_system(
    path: &Path,
    expected_revision: &str,
    system: CampaignEditableSystem,
) -> Result<CampaignEditableSettingsResult> {
    validate_revision(expected_revision)?;
    let preset = normalize_editable_text(&system.preset, "system preset", true)?;
    crate::presets::load(&preset)?;
    let overrides = normalize_editable_multiline(
        &system.overrides,
        "system overrides",
        MAX_EDITABLE_NOTES_LENGTH,
    )?;

    let original = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if crate::util::content_revision(&original) != expected_revision {
        bail!("campaign settings changed since they were read");
    }
    let original = std::str::from_utf8(&original)
        .with_context(|| format!("campaign config is not UTF-8: {}", path.display()))?;
    let mut document = original
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    let system = document
        .as_table_mut()
        .entry("system")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| anyhow!("[system] must be a TOML table to edit game system settings"))?;
    system.insert("preset", value(preset));
    system.insert("overrides", value(overrides));

    let updated = document.to_string();
    let config: CampaignConfig = toml::from_str(&updated)
        .with_context(|| format!("validating updated {}", path.display()))?;
    match crate::util::atomic_replace_if_revision(path, expected_revision, updated.as_bytes()) {
        Ok(()) => Ok(CampaignEditableSettingsResult {
            config,
            revision: crate::util::content_revision(updated.as_bytes()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            bail!("campaign settings changed since they were read")
        }
        Err(error) => Err(error).with_context(|| format!("writing {}", path.display())),
    }
}

pub fn write_campaign_editable_backend(
    path: &Path,
    expected_revision: &str,
    backend: CampaignEditableBackend,
) -> Result<CampaignEditableSettingsResult> {
    validate_revision(expected_revision)?;
    let kind =
        normalize_optional_choice(backend.kind, "backend", &["ollama", "openai", "anthropic"])?;
    let model = normalize_optional_limited_text(backend.model, "backend model", 256)?;
    let base_url = normalize_optional_limited_text(backend.base_url, "backend URL", 2_048)?;
    if let Some(url) = &base_url {
        let parsed = reqwest::Url::parse(url)
            .with_context(|| "backend URL must be an absolute HTTP(S) URL")?;
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            bail!("backend URL must be HTTP(S) and cannot contain credentials");
        }
    }

    write_campaign_section(path, expected_revision, "backend", |table| {
        replace_optional_string(table, "kind", kind);
        replace_optional_string(table, "base_url", base_url);
        replace_optional_string(table, "model", model);
        Ok(())
    })
}

pub fn write_campaign_editable_asr(
    path: &Path,
    expected_revision: &str,
    asr: CampaignEditableAsr,
) -> Result<CampaignEditableSettingsResult> {
    validate_revision(expected_revision)?;
    let model = normalize_optional_limited_text(asr.model, "ASR model", 256)?;
    if let Some(model) = &model {
        if crate::asr::find(model).is_none() {
            bail!("unknown ASR model '{model}'");
        }
    }
    if matches!(asr.threads, Some(0 | 1025..)) {
        bail!("ASR threads must be between 1 and 1024");
    }
    let device = normalize_optional_choice(asr.device, "ASR device", &["auto", "cuda", "cpu"])?;
    let engine = normalize_optional_choice(
        asr.engine,
        "ASR engine",
        &["auto", "local", "whisper-cli", "whisperx"],
    )?;

    write_campaign_section(path, expected_revision, "asr", |table| {
        replace_optional_string(table, "model", model);
        replace_optional_integer(table, "threads", asr.threads.map(i64::from));
        replace_optional_bool(table, "diarize", asr.diarize);
        replace_optional_bool(table, "vad", asr.vad);
        replace_optional_string(table, "device", device);
        replace_optional_string(table, "engine", engine);
        Ok(())
    })
}

pub fn write_campaign_editable_prompts(
    path: &Path,
    expected_revision: &str,
    prompts: CampaignEditablePrompts,
) -> Result<CampaignEditableSettingsResult> {
    validate_revision(expected_revision)?;
    let prompts = CampaignEditablePrompts {
        bullets: normalize_optional_multiline(prompts.bullets, "bullets prompt")?,
        dm_notes: normalize_optional_multiline(prompts.dm_notes, "DM notes prompt")?,
        recap: normalize_optional_multiline(prompts.recap, "recap prompt")?,
        summary: normalize_optional_multiline(prompts.summary, "summary prompt")?,
        story: normalize_optional_multiline(prompts.story, "story prompt")?,
        quotes: normalize_optional_multiline(prompts.quotes, "quotes prompt")?,
    };
    write_campaign_section(path, expected_revision, "prompts", |table| {
        replace_optional_string(table, "bullets", prompts.bullets);
        replace_optional_string(table, "dm_notes", prompts.dm_notes);
        replace_optional_string(table, "recap", prompts.recap);
        replace_optional_string(table, "summary", prompts.summary);
        replace_optional_string(table, "story", prompts.story);
        replace_optional_string(table, "quotes", prompts.quotes);
        Ok(())
    })
}

fn write_campaign_section(
    path: &Path,
    expected_revision: &str,
    section: &str,
    update: impl FnOnce(&mut Table) -> Result<()>,
) -> Result<CampaignEditableSettingsResult> {
    let original = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if crate::util::content_revision(&original) != expected_revision {
        bail!("campaign settings changed since they were read");
    }
    let original = std::str::from_utf8(&original)
        .with_context(|| format!("campaign config is not UTF-8: {}", path.display()))?;
    let mut document = original
        .parse::<DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    let table = document
        .as_table_mut()
        .entry(section)
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| anyhow!("[{section}] must be a TOML table to edit these settings"))?;
    update(table)?;

    let updated = document.to_string();
    let config: CampaignConfig = toml::from_str(&updated)
        .with_context(|| format!("validating updated {}", path.display()))?;
    match crate::util::atomic_replace_if_revision(path, expected_revision, updated.as_bytes()) {
        Ok(()) => Ok(CampaignEditableSettingsResult {
            config,
            revision: crate::util::content_revision(updated.as_bytes()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            bail!("campaign settings changed since they were read")
        }
        Err(error) => Err(error).with_context(|| format!("writing {}", path.display())),
    }
}

fn validate_revision(revision: &str) -> Result<()> {
    if revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("campaign settings revision is invalid");
    }
    Ok(())
}

fn normalize_editable_settings(
    settings: CampaignEditableSettings,
) -> Result<CampaignEditableSettings> {
    if settings.players.len() > MAX_EDITABLE_PLAYERS {
        bail!("no more than {MAX_EDITABLE_PLAYERS} players may be saved at once");
    }
    if settings.vocabulary.len() > MAX_EDITABLE_VOCABULARY {
        bail!("no more than {MAX_EDITABLE_VOCABULARY} vocabulary terms may be saved at once");
    }
    if settings.replacements.len() > MAX_EDITABLE_REPLACEMENTS {
        bail!("no more than {MAX_EDITABLE_REPLACEMENTS} corrections may be saved at once");
    }

    let players = settings
        .players
        .into_iter()
        .map(|player| {
            Ok(Player {
                player: normalize_editable_text(&player.player, "player name", true)?,
                character: normalize_editable_text(&player.character, "character name", true)?,
                ancestry: normalize_editable_text(&player.ancestry, "ancestry", false)?,
                class: normalize_editable_text(&player.class, "class", false)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut vocabulary_seen = BTreeSet::new();
    let vocabulary = settings
        .vocabulary
        .into_iter()
        .map(|term| {
            let term = normalize_editable_text(&term, "vocabulary term", true)?;
            if !vocabulary_seen.insert(term.clone()) {
                bail!("vocabulary terms must be unique");
            }
            Ok(term)
        })
        .collect::<Result<Vec<_>>>()?;

    let mut replacement_keys = BTreeSet::new();
    let replacements = settings
        .replacements
        .into_iter()
        .map(|replacement| {
            let from = normalize_editable_text(&replacement.from, "correction source", true)?;
            let to = normalize_editable_text(&replacement.to, "correction value", true)?;
            if !replacement_keys.insert(from.clone()) {
                bail!("correction sources must be unique");
            }
            Ok(CampaignTextReplacement { from, to })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(CampaignEditableSettings {
        players,
        vocabulary,
        replacements,
    })
}

fn normalize_editable_text(value: &str, label: &str, required: bool) -> Result<String> {
    let value = value.trim();
    if required && value.is_empty() {
        bail!("{label} is required");
    }
    if value.chars().count() > MAX_EDITABLE_TEXT_LENGTH {
        bail!("{label} must be at most {MAX_EDITABLE_TEXT_LENGTH} characters");
    }
    if value.chars().any(char::is_control) {
        bail!("{label} cannot contain control characters");
    }
    Ok(value.into())
}

fn normalize_editable_notes(value: &str) -> Result<String> {
    normalize_editable_multiline(value, "campaign notes", MAX_EDITABLE_NOTES_LENGTH)
}

fn normalize_editable_multiline(value: &str, label: &str, max_length: usize) -> Result<String> {
    let value = value.trim();
    if value.chars().count() > max_length {
        bail!("{label} must be at most {max_length} characters");
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        bail!("{label} cannot contain unsupported control characters");
    }
    Ok(value.into())
}

fn normalize_optional_limited_text(
    value: Option<String>,
    label: &str,
    max_length: usize,
) -> Result<Option<String>> {
    value
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                return Ok(None);
            }
            if value.chars().count() > max_length {
                bail!("{label} must be at most {max_length} characters");
            }
            if value.chars().any(char::is_control) {
                bail!("{label} cannot contain control characters");
            }
            Ok(Some(value.into()))
        })
        .transpose()
        .map(Option::flatten)
}

fn normalize_optional_multiline(value: Option<String>, label: &str) -> Result<Option<String>> {
    value
        .map(|value| {
            if value.trim().is_empty() {
                Ok(None)
            } else {
                normalize_editable_multiline(&value, label, MAX_EDITABLE_NOTES_LENGTH).map(Some)
            }
        })
        .transpose()
        .map(Option::flatten)
}

fn normalize_optional_choice(
    value: Option<String>,
    label: &str,
    choices: &[&str],
) -> Result<Option<String>> {
    let value = normalize_optional_limited_text(value, label, MAX_EDITABLE_TEXT_LENGTH)?;
    if let Some(value) = &value {
        if !choices.contains(&value.as_str()) {
            bail!("{label} must be one of: {}", choices.join(", "));
        }
    }
    Ok(value)
}

fn replace_optional_string(table: &mut Table, key: &str, value_to_write: Option<String>) {
    match value_to_write {
        Some(value_to_write) => {
            table.insert(key, value(value_to_write));
        }
        None => {
            table.remove(key);
        }
    }
}

fn replace_optional_integer(table: &mut Table, key: &str, value_to_write: Option<i64>) {
    match value_to_write {
        Some(value_to_write) => {
            table.insert(key, value(value_to_write));
        }
        None => {
            table.remove(key);
        }
    }
}

fn replace_optional_bool(table: &mut Table, key: &str, value_to_write: Option<bool>) {
    match value_to_write {
        Some(value_to_write) => {
            table.insert(key, value(value_to_write));
        }
        None => {
            table.remove(key);
        }
    }
}

fn replace_players(document: &mut DocumentMut, players: &[Player]) {
    let root = document.as_table_mut();
    if players.is_empty() {
        root.remove("players");
        return;
    }

    let mut tables = ArrayOfTables::new();
    for player in players {
        let mut table = Table::new();
        table["player"] = value(&player.player);
        table["character"] = value(&player.character);
        if !player.ancestry.is_empty() {
            table["ancestry"] = value(&player.ancestry);
        }
        if !player.class.is_empty() {
            table["class"] = value(&player.class);
        }
        tables.push(table);
    }
    root.insert("players", Item::ArrayOfTables(tables));
}

fn replace_transcription_settings(
    document: &mut DocumentMut,
    settings: &CampaignEditableSettings,
) -> Result<()> {
    let transcription = document
        .as_table_mut()
        .entry("transcription")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| anyhow!("[transcription] must be a TOML table to edit these settings"))?;

    let mut vocabulary = Array::new();
    for term in &settings.vocabulary {
        vocabulary.push(term.as_str());
    }
    transcription.insert("vocabulary", value(vocabulary));

    if settings.replacements.is_empty() {
        transcription.remove("replacements");
        return Ok(());
    }

    let mut replacements = Table::new();
    for replacement in &settings.replacements {
        replacements.insert(&replacement.from, value(&replacement.to));
    }
    transcription.insert("replacements", Item::Table(replacements));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn private_permissions_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[backend]\nkind = 'ollama'\n").unwrap();
        set_private_permissions(&path).unwrap();
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn round_trip_campaign() {
        let cfg = CampaignConfig {
            source_path: None,
            campaign: Campaign {
                name: "Test".into(),
                gm: "Alice".into(),
                setting: "Forgotten Realms".into(),
                notes: String::new(),
            },
            backend: CampaignBackendConfig::default(),
            asr: CampaignAsrConfig::default(),
            players: vec![Player {
                player: "Bob".into(),
                character: "Drokel".into(),
                ancestry: "Dwarf".into(),
                class: "Fighter".into(),
            }],
            transcription: TranscriptionConfig::default(),
            system: SystemRef {
                preset: "dnd5e".into(),
                overrides: String::new(),
            },
            outputs: OutputsConfig::default(),
            prompts: PromptOverrides::default(),
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: CampaignConfig = toml::from_str(&text).unwrap();
        assert_eq!(back.campaign.name, "Test");
        assert_eq!(back.players[0].character, "Drokel");
        assert_eq!(back.system.preset, "dnd5e");
    }

    #[test]
    fn campaign_overrides_are_fieldwise() {
        let mut global = GlobalConfig::default();
        global.backend.kind = "ollama".into();
        global.backend.model = Some("global-model".into());
        global.asr.diarize = false;

        let campaign = CampaignConfig {
            source_path: None,
            campaign: Campaign::default(),
            backend: CampaignBackendConfig {
                model: Some("campaign-model".into()),
                ..Default::default()
            },
            asr: CampaignAsrConfig {
                diarize: Some(true),
                ..Default::default()
            },
            players: Vec::new(),
            transcription: TranscriptionConfig::default(),
            system: SystemRef::default(),
            outputs: OutputsConfig::default(),
            prompts: PromptOverrides::default(),
        };
        let merged = effective(&global, &campaign);
        assert_eq!(merged.backend.kind, "ollama");
        assert_eq!(merged.backend.model.as_deref(), Some("campaign-model"));
        assert!(merged.asr.diarize);
    }

    #[test]
    fn env_expansion() {
        std::env::set_var("SS_TEST_KEY", "secret");
        assert_eq!(expand_env("${SS_TEST_KEY}"), "secret");
        assert_eq!(
            expand_env("prefix-${SS_TEST_KEY}-suffix"),
            "prefix-secret-suffix"
        );
        assert_eq!(expand_env("plain"), "plain");
    }

    #[test]
    fn ui_mouse_defaults_on_and_can_be_disabled() {
        assert!(GlobalConfig::default().ui.mouse);
        let inherited: GlobalConfig = toml::from_str("[ui]\ntheme = 'mono'").unwrap();
        assert!(inherited.ui.mouse);
        let disabled: GlobalConfig = toml::from_str("[ui]\nmouse = false").unwrap();
        assert!(!disabled.ui.mouse);
    }

    #[test]
    fn desktop_settings_default_to_dmy_system_and_half_volume() {
        let config: GlobalConfig = toml::from_str("").unwrap();
        assert_eq!(config.desktop, DesktopConfig::default());
        assert_eq!(config.desktop.date_format, "dmy");
        assert_eq!(config.desktop.appearance, "system");
        assert_eq!(config.desktop.theme, "default");
        assert_eq!(config.desktop.player_volume, 50);
        assert_eq!(config.desktop.onboarding_completed_version, 0);
        assert!(config.desktop.onboarding_outcome.is_empty());
    }

    #[test]
    fn desktop_settings_writer_preserves_unowned_toml_and_rejects_invalid_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let source =
            "# Keep this comment.\n[backend]\nkind = \"ollama\"\n\n[future]\nenabled = true\n";
        std::fs::write(&path, source).unwrap();

        let paths = PathsConfig {
            audio_dir: "recordings".into(),
            campaigns_dir: "tables".into(),
            output_dir: "generated".into(),
        };
        let result = write_desktop_settings(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            DesktopConfig {
                date_format: "dmy".into(),
                appearance: "dark".into(),
                theme: "default".into(),
                player_volume: 64,
                onboarding_completed_version: 1,
                onboarding_outcome: "finished".into(),
            },
            paths,
        )
        .unwrap();
        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# Keep this comment."));
        assert!(updated.contains("[future]"));
        assert!(updated.contains("enabled = true"));
        assert_eq!(result.settings.player_volume, 64);
        assert_eq!(result.settings.onboarding_completed_version, 1);
        assert_eq!(result.settings.onboarding_outcome, "finished");
        assert_eq!(result.paths.audio_dir, PathBuf::from("recordings"));
        assert_eq!(result.paths.campaigns_dir, PathBuf::from("tables"));
        assert_eq!(result.paths.output_dir, PathBuf::from("generated"));
        assert!(updated.contains("audio_dir = \"recordings\""));
        assert!(updated.contains("campaigns_dir = \"tables\""));
        assert!(updated.contains("output_dir = \"generated\""));

        let invalid = write_desktop_settings(
            &path,
            &result.revision,
            DesktopConfig {
                appearance: "sepia".into(),
                ..result.settings
            },
            result.paths,
        )
        .unwrap_err();
        assert!(invalid.to_string().contains("appearance must be"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), updated);
    }

    #[test]
    fn desktop_settings_writer_rejects_stale_revision() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let source = "[desktop]\ndate_format = \"dmy\"\n";
        std::fs::write(&path, source).unwrap();

        let error = write_desktop_settings(
            &path,
            &crate::util::content_revision(b"stale"),
            DesktopConfig::default(),
            PathsConfig::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("changed since they were read"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_settings_preserve_unrelated_toml_comments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = r#"# Keep this campaign heading.
[campaign]
name = "Table Test"

[backend]
# Keep this backend comment.
kind = "ollama"

[[players]]
player = "Old Player"
character = "Old Character"

[transcription]
vocabulary = ["Old term"]

[transcription.replacements]
"Old term" = "New term"

[outputs]
# Keep this output comment.
default = ["summary"]
"#;
        std::fs::write(&path, source).unwrap();

        let result = write_campaign_editable_settings(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditableSettings {
                players: vec![Player {
                    player: "Mina".into(),
                    character: "Tamsin".into(),
                    ancestry: "Elf".into(),
                    class: "Wizard".into(),
                }],
                vocabulary: vec!["Damasus".into()],
                replacements: vec![CampaignTextReplacement {
                    from: "Mosses".into(),
                    to: "Damasus".into(),
                }],
            },
        )
        .unwrap();

        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# Keep this campaign heading."));
        assert!(updated.contains("# Keep this backend comment."));
        assert!(updated.contains("# Keep this output comment."));
        assert_eq!(result.config.players[0].player, "Mina");
        assert_eq!(result.config.transcription.vocabulary, ["Damasus"]);
        assert_eq!(
            result.config.transcription.replacements.get("Mosses"),
            Some(&"Damasus".to_string())
        );
        assert_eq!(
            result.revision,
            crate::util::content_revision(updated.as_bytes())
        );
    }

    #[test]
    fn editable_campaign_identity_preserves_name_and_unrelated_toml() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = r#"# Keep heading.
[campaign]
name = "Table Test"
gm = "Old GM"

[backend]
# Keep provider context.
kind = "ollama"
"#;
        std::fs::write(&path, source).unwrap();

        let result = write_campaign_editable_identity(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditableIdentity {
                gm: " Mina ".into(),
                setting: "The Shattered Coast".into(),
                notes: "First line\nSecond line".into(),
            },
        )
        .unwrap();

        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# Keep heading."));
        assert!(updated.contains("# Keep provider context."));
        assert_eq!(result.config.campaign.name, "Table Test");
        assert_eq!(result.config.campaign.gm, "Mina");
        assert_eq!(result.config.campaign.setting, "The Shattered Coast");
        assert_eq!(result.config.campaign.notes, "First line\nSecond line");
    }

    #[test]
    fn editable_campaign_identity_rejects_invalid_values_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();

        let error = write_campaign_editable_identity(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditableIdentity {
                gm: "Mina\u{7}".into(),
                setting: String::new(),
                notes: String::new(),
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("game master cannot contain control characters"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_speakers_round_trip_and_preserve_unrelated_toml() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = r#"[campaign]
name = "Table Test"

[transcription]
vocabulary = ["Damasus"]

[transcription.speakers]
SPEAKER_00 = "Old name"

[backend]
# Keep provider context.
kind = "ollama"
"#;
        std::fs::write(&path, source).unwrap();

        let result = write_campaign_editable_speakers(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            vec![
                CampaignEditableSpeaker {
                    label: "SPEAKER_01".into(),
                    name: " Tamsin ".into(),
                },
                CampaignEditableSpeaker {
                    label: "SPEAKER_00".into(),
                    name: "Mina".into(),
                },
            ],
        )
        .unwrap();

        let updated = std::fs::read_to_string(path).unwrap();
        assert!(updated.contains("# Keep provider context."));
        assert_eq!(result.config.transcription.vocabulary, ["Damasus"]);
        assert_eq!(result.config.transcription.speakers["SPEAKER_00"], "Mina");
        assert_eq!(result.config.transcription.speakers["SPEAKER_01"], "Tamsin");
    }

    #[test]
    fn editable_campaign_speakers_reject_duplicates_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();

        let error = write_campaign_editable_speakers(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            vec![
                CampaignEditableSpeaker {
                    label: "SPEAKER_00".into(),
                    name: "Mina".into(),
                },
                CampaignEditableSpeaker {
                    label: " SPEAKER_00 ".into(),
                    name: "Tamsin".into(),
                },
            ],
        )
        .unwrap_err();

        assert!(error.to_string().contains("speaker labels must be unique"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_outputs_preserve_order_and_unrelated_toml() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = r#"[campaign]
name = "Table Test"

[outputs]
default = ["summary"]

[backend]
# Keep provider context.
kind = "ollama"
"#;
        std::fs::write(&path, source).unwrap();

        let result = write_campaign_editable_outputs(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditableOutputs {
                default: vec!["recap".into(), "quotes".into(), "bullets".into()],
            },
        )
        .unwrap();

        let updated = std::fs::read_to_string(path).unwrap();
        assert!(updated.contains("# Keep provider context."));
        assert_eq!(
            result.config.outputs.default,
            ["recap", "quotes", "bullets"]
        );
    }

    #[test]
    fn editable_campaign_outputs_reject_unknown_and_duplicate_ids_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();
        let revision = crate::util::content_revision(source.as_bytes());

        let unknown = write_campaign_editable_outputs(
            &path,
            &revision,
            CampaignEditableOutputs {
                default: vec!["timeline".into()],
            },
        )
        .unwrap_err();
        assert!(unknown.to_string().contains("unknown output artifact"));

        let duplicate = write_campaign_editable_outputs(
            &path,
            &revision,
            CampaignEditableOutputs {
                default: vec!["summary".into(), "summary".into()],
            },
        )
        .unwrap_err();
        assert!(duplicate
            .to_string()
            .contains("output artifacts must be unique"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_system_validates_preset_and_preserves_comments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = r#"[campaign]
name = "Table Test"

[system]
preset = "generic"

[backend]
# Keep provider context.
kind = "ollama"
"#;
        std::fs::write(&path, source).unwrap();

        let result = write_campaign_editable_system(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditableSystem {
                preset: "dnd5e".into(),
                overrides: "Track faction clocks.\nAvoid spoilers.".into(),
            },
        )
        .unwrap();

        let updated = std::fs::read_to_string(path).unwrap();
        assert!(updated.contains("# Keep provider context."));
        assert_eq!(result.config.system.preset, "dnd5e");
        assert_eq!(
            result.config.system.overrides,
            "Track faction clocks.\nAvoid spoilers."
        );
    }

    #[test]
    fn editable_campaign_system_rejects_unknown_preset_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();

        let error = write_campaign_editable_system(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditableSystem {
                preset: "invented-system".into(),
                overrides: String::new(),
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown system preset"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_backend_and_asr_preserve_secrets_and_clear_owned_keys() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = r#"[campaign]
name = "Table Test"

[backend]
api_key = "keep-backend-secret"

[asr]
hf_token = "keep-asr-secret"
"#;
        std::fs::write(&path, source).unwrap();

        let backend = write_campaign_editable_backend(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditableBackend {
                kind: Some("openai".into()),
                base_url: Some("https://api.example.test/v1".into()),
                model: Some("notes-model".into()),
            },
        )
        .unwrap();
        assert_eq!(backend.config.backend.kind.as_deref(), Some("openai"));
        assert_eq!(
            backend.config.backend.api_key.as_deref(),
            Some("keep-backend-secret")
        );

        let asr = write_campaign_editable_asr(
            &path,
            &backend.revision,
            CampaignEditableAsr {
                model: Some("base".into()),
                threads: Some(8),
                diarize: Some(true),
                vad: Some(false),
                device: Some("cpu".into()),
                engine: Some("local".into()),
            },
        )
        .unwrap();
        assert_eq!(asr.config.asr.model.as_deref(), Some("base"));
        assert_eq!(asr.config.asr.threads, Some(8));
        assert_eq!(asr.config.asr.hf_token.as_deref(), Some("keep-asr-secret"));

        let cleared = write_campaign_editable_backend(
            &path,
            &asr.revision,
            CampaignEditableBackend {
                kind: None,
                base_url: None,
                model: None,
            },
        )
        .unwrap();
        assert_eq!(cleared.config.backend.kind, None);
        assert_eq!(cleared.config.backend.base_url, None);
        assert_eq!(cleared.config.backend.model, None);
        assert_eq!(
            cleared.config.backend.api_key.as_deref(),
            Some("keep-backend-secret")
        );
    }

    #[test]
    fn editable_campaign_overrides_reject_invalid_values_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();
        let revision = crate::util::content_revision(source.as_bytes());

        let backend_error = write_campaign_editable_backend(
            &path,
            &revision,
            CampaignEditableBackend {
                kind: Some("local".into()),
                base_url: Some("file:///tmp/socket".into()),
                model: None,
            },
        )
        .unwrap_err();
        assert!(backend_error.to_string().contains("backend must be one of"));

        let asr_error = write_campaign_editable_asr(
            &path,
            &revision,
            CampaignEditableAsr {
                model: Some("unknown-model".into()),
                threads: None,
                diarize: None,
                vad: None,
                device: None,
                engine: None,
            },
        )
        .unwrap_err();
        assert!(asr_error.to_string().contains("unknown ASR model"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_prompts_round_trip_clear_and_preserve_comments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = r#"[campaign]
name = "Table Test"

[prompts]
summary = "Old summary"
story = "Old story"

[backend]
# Keep provider context.
api_key = "keep-secret"
"#;
        std::fs::write(&path, source).unwrap();

        let result = write_campaign_editable_prompts(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditablePrompts {
                bullets: Some("Extract every event.\nKeep chronology.".into()),
                dm_notes: None,
                recap: Some("  ".into()),
                summary: Some("New summary".into()),
                story: None,
                quotes: Some("Capture exact quotations.".into()),
            },
        )
        .unwrap();

        let updated = std::fs::read_to_string(path).unwrap();
        assert!(updated.contains("# Keep provider context."));
        assert!(updated.contains("api_key = \"keep-secret\""));
        assert_eq!(
            result.config.prompts.bullets.as_deref(),
            Some("Extract every event.\nKeep chronology.")
        );
        assert_eq!(
            result.config.prompts.summary.as_deref(),
            Some("New summary")
        );
        assert_eq!(
            result.config.prompts.quotes.as_deref(),
            Some("Capture exact quotations.")
        );
        assert_eq!(result.config.prompts.recap, None);
        assert_eq!(result.config.prompts.story, None);
    }

    #[test]
    fn editable_campaign_prompts_reject_control_characters_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();

        let error = write_campaign_editable_prompts(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditablePrompts {
                bullets: Some("Bad\u{7}prompt".into()),
                dm_notes: None,
                recap: None,
                summary: None,
                story: None,
                quotes: None,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("unsupported control characters"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_settings_reject_stale_revisions_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();

        let error = write_campaign_editable_settings(
            &path,
            &crate::util::content_revision(b"different"),
            CampaignEditableSettings {
                players: Vec::new(),
                vocabulary: Vec::new(),
                replacements: Vec::new(),
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("changed since they were read"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_settings_reject_duplicate_and_control_character_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();
        let revision = crate::util::content_revision(source.as_bytes());

        let duplicate_error = write_campaign_editable_settings(
            &path,
            &revision,
            CampaignEditableSettings {
                players: Vec::new(),
                vocabulary: vec!["Damasus".into(), "Damasus".into()],
                replacements: Vec::new(),
            },
        )
        .unwrap_err();
        assert!(duplicate_error
            .to_string()
            .contains("vocabulary terms must be unique"));

        let control_error = write_campaign_editable_settings(
            &path,
            &revision,
            CampaignEditableSettings {
                players: vec![Player {
                    player: "Mina\nOther".into(),
                    character: "Tamsin".into(),
                    ancestry: String::new(),
                    class: String::new(),
                }],
                vocabulary: Vec::new(),
                replacements: Vec::new(),
            },
        )
        .unwrap_err();
        assert!(control_error
            .to_string()
            .contains("player name cannot contain control characters"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_settings_reject_non_table_transcription_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "transcription = \"invalid\"\n\n[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();

        let error = write_campaign_editable_settings(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditableSettings {
                players: Vec::new(),
                vocabulary: Vec::new(),
                replacements: Vec::new(),
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("[transcription] must be a TOML table"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }

    #[test]
    fn editable_campaign_settings_reject_duplicate_correction_sources_after_trimming() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("campaign.toml");
        let source = "[campaign]\nname = \"Table Test\"\n";
        std::fs::write(&path, source).unwrap();

        let error = write_campaign_editable_settings(
            &path,
            &crate::util::content_revision(source.as_bytes()),
            CampaignEditableSettings {
                players: Vec::new(),
                vocabulary: Vec::new(),
                replacements: vec![
                    CampaignTextReplacement {
                        from: "Mosses".into(),
                        to: "Damasus".into(),
                    },
                    CampaignTextReplacement {
                        from: " Mosses ".into(),
                        to: "Damasus".into(),
                    },
                ],
            },
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("correction sources must be unique"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), source);
    }
}
