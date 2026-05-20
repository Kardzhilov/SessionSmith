//! Bundled game-system presets. Each preset is a TOML document embedded at
//! compile time via `include_str!` so the binary ships self-contained.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub terminology: String,
    #[serde(default)]
    pub capture: String,
    #[serde(default)]
    pub extra_sections: String,
    #[serde(default)]
    pub forbidden_phrases: Vec<String>,
}

struct Bundled {
    id: &'static str,
    toml: &'static str,
}

const BUNDLED: &[Bundled] = &[
    Bundled { id: "generic",     toml: include_str!("../presets/generic.toml") },
    Bundled { id: "dnd5e",       toml: include_str!("../presets/dnd5e.toml") },
    Bundled { id: "pf2e",        toml: include_str!("../presets/pf2e.toml") },
    Bundled { id: "coc",         toml: include_str!("../presets/coc.toml") },
    Bundled { id: "blades",      toml: include_str!("../presets/blades.toml") },
    Bundled { id: "daggerheart", toml: include_str!("../presets/daggerheart.toml") },
    Bundled { id: "wordsmith",   toml: include_str!("../presets/wordsmith.toml") },
];

pub fn list_ids() -> Vec<&'static str> {
    BUNDLED.iter().map(|b| b.id).collect()
}

pub fn load(id: &str) -> Result<Preset> {
    let raw = BUNDLED.iter()
        .find(|b| b.id == id)
        .ok_or_else(|| anyhow!("unknown system preset '{id}'. Available: {}", list_ids().join(", ")))?;
    let preset: Preset = toml::from_str(raw.toml)
        .map_err(|e| anyhow!("parsing bundled preset '{id}': {e}"))?;
    Ok(preset)
}

pub fn raw_toml(id: &str) -> Result<&'static str> {
    BUNDLED.iter()
        .find(|b| b.id == id)
        .map(|b| b.toml)
        .ok_or_else(|| anyhow!("unknown preset '{id}'"))
}

impl Preset {
    /// Render the preset's contribution to a system prompt.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("Game system: {}\n", self.name));
        if !self.terminology.trim().is_empty() {
            s.push_str("\nTerminology:\n");
            s.push_str(self.terminology.trim_end());
            s.push('\n');
        }
        if !self.capture.trim().is_empty() {
            s.push_str("\nAlways capture:\n");
            s.push_str(self.capture.trim_end());
            s.push('\n');
        }
        s
    }

    pub fn render_extra_sections(&self) -> String {
        self.extra_sections.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_bundled_parse() {
        for id in list_ids() {
            let p = load(id).expect(id);
            assert!(!p.name.is_empty(), "{id} name");
        }
    }

    #[test]
    fn render_contains_terminology() {
        let p = load("dnd5e").unwrap();
        let rendered = p.render();
        assert!(rendered.contains("HP"));
        assert!(rendered.contains("Game system"));
    }
}
