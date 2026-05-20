//! Subcommand handlers.

pub mod doctor;
pub mod home;
pub mod init;
pub mod log_cmd;
pub mod models;
pub mod notes;
pub mod run;
pub mod systems;
pub mod transcribe;

use anyhow::{anyhow, Result};
use std::path::PathBuf;

use crate::config::CampaignConfig;

/// Resolve the campaign config path interactively:
///
/// 1. Explicit `--campaign` / `SESSIONSMITH_CAMPAIGN` env var.
/// 2. Scan `campaigns/*.toml` (skip dotfiles); auto-pick if one exists,
///    prompt if multiple.
/// 3. `campaign.toml` in cwd (backward compat).
/// 4. Error with actionable hint.
pub fn resolve_campaign(override_path: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(p) = override_path { return Ok(p.clone()); }
    if let Ok(p) = std::env::var("SESSIONSMITH_CAMPAIGN") { return Ok(PathBuf::from(p)); }

    let campaigns_dir = PathBuf::from("campaigns");
    if campaigns_dir.is_dir() {
        let mut options: Vec<PathBuf> = std::fs::read_dir(&campaigns_dir)
            .into_iter().flatten().flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()) == Some("toml")
                    && !p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with('.'))
                        .unwrap_or(true)
            })
            .collect();
        options.sort();
        match options.len() {
            0 => { /* fall through */ }
            1 => return Ok(options.remove(0)),
            _ => {
                let labels: Vec<String> = options.iter()
                    .map(|p| p.file_stem().unwrap_or_default().to_string_lossy().to_string())
                    .collect();
                let chosen = inquire::Select::new("Select campaign:", labels.clone())
                    .prompt()
                    .map_err(|e| anyhow!("campaign selection: {e}"))?;
                let idx = labels.iter().position(|l| *l == chosen).unwrap();
                return Ok(options[idx].clone());
            }
        }
    }

    // Backward compat: root campaign.toml
    let root = PathBuf::from("campaign.toml");
    if root.exists() { return Ok(root); }

    Err(anyhow!(
        "no campaign config found.\n  \
         Run `sessionsmith init` to create one, or pass --campaign <path>."
    ))
}

pub fn load_campaign_or_die(path: &std::path::Path) -> Result<CampaignConfig> {
    if !path.exists() {
        anyhow::bail!(
            "campaign config not found at {}. Run `sessionsmith init` to create one.",
            path.display()
        );
    }
    CampaignConfig::load(path)
}

/// Derive a filesystem-safe slug from a campaign name.
pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
