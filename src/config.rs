//! Configuration: global (`~/.config/sessionsmith/config.toml`) + per-repo
//! `campaign.toml`. CLI flags override campaign which overrides preset defaults.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
    pub hardware: Option<HardwareProfile>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrConfig {
    /// Path to the whisper.cpp `whisper-cli` (or compatible) binary.
    pub binary: Option<PathBuf>,
    /// Default model name (e.g. "large-v3", "medium", "base").
    pub model: Option<String>,
    /// Directory to cache downloaded ggml models.
    pub model_dir: Option<PathBuf>,
    /// Threads passed to whisper-cli.
    pub threads: Option<u32>,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self { binary: None, model: None, model_dir: None, threads: None }
    }
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
}

fn default_timeout() -> u64 { 1800 }

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self { parallel_passes: false, timeout_secs: default_timeout(), think: false }
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
        if p.exists() {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("reading {}", p.display()))?;
            Ok(toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path()?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&p, text).with_context(|| format!("writing {}", p.display()))?;
        Ok(())
    }

    /// Resolve API key, expanding `${VAR}` shell-style.
    pub fn resolved_api_key(&self) -> Option<String> {
        self.backend.api_key.as_ref().map(|raw| expand_env(raw))
    }
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
                if n == '}' { break; }
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
    pub campaign: Campaign,
    #[serde(default)]
    pub players: Vec<Player>,
    #[serde(default)]
    pub system: SystemRef,
    #[serde(default)]
    pub outputs: OutputsConfig,
    #[serde(default)]
    pub prompts: PromptOverrides,
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
        Self { preset: "generic".into(), overrides: String::new() }
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
    fn default() -> Self { Self { default: default_artifacts() } }
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
    #[serde(default)]
    pub campaign_log: Option<String>,
}

impl CampaignConfig {
    pub fn default_path() -> PathBuf {
        PathBuf::from("campaign.toml")
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading campaign config: {}", path.display()))?;
        let cfg: Self = toml::from_str(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Filesystem-safe slug derived from the campaign name.
    /// e.g. "My Game" → "my-game", "Curse of Strahd" → "curse-of-strahd"
    pub fn slug(&self) -> String {
        let s: String = self.campaign.name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        let parts: Vec<&str> = s.split('-').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() { "campaign".into() } else { parts.join("-") }
    }

    /// Root output directory for this campaign: `output/<slug>/`
    pub fn output_root(&self) -> PathBuf {
        PathBuf::from("output").join(self.slug())
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
                let detail = if bits.is_empty() { String::new() } else { format!(" ({})", bits.join(" ")) };
                s.push_str(&format!("  - {} plays {}{}\n", p.player, p.character, detail));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_campaign() {
        let cfg = CampaignConfig {
            campaign: Campaign {
                name: "Test".into(),
                gm: "Alice".into(),
                setting: "Forgotten Realms".into(),
                notes: String::new(),
            },
            players: vec![Player {
                player: "Bob".into(),
                character: "Drokel".into(),
                ancestry: "Dwarf".into(),
                class: "Fighter".into(),
            }],
            system: SystemRef { preset: "dnd5e".into(), overrides: String::new() },
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
    fn env_expansion() {
        std::env::set_var("SS_TEST_KEY", "secret");
        assert_eq!(expand_env("${SS_TEST_KEY}"), "secret");
        assert_eq!(expand_env("prefix-${SS_TEST_KEY}-suffix"), "prefix-secret-suffix");
        assert_eq!(expand_env("plain"), "plain");
    }
}
