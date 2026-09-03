//! CLI wrapper for offline campaign exports.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use crate::cli::{ExportArgs, ExportFormat as CliExportFormat};
use crate::export::{export, ExportFormat, ExportRequest};
use crate::{commands, ui};

pub async fn run(args: ExportArgs) -> Result<()> {
    let campaign = commands::load_campaign_or_die(&commands::resolve_campaign(None)?)?;
    let sessions = session_dirs(&campaign.notes_dir(), args.stem.as_deref(), args.all)?;
    let output = args
        .out
        .unwrap_or_else(|| PathBuf::from("exports").join(campaign.slug()));
    let format = match args.format {
        CliExportFormat::Html => ExportFormat::Html,
        CliExportFormat::Obsidian => ExportFormat::Obsidian,
    };
    let result = export(ExportRequest {
        campaign_name: campaign.campaign.name,
        session_dirs: sessions,
        output_dir: output,
        format,
        player_safe: args.player_safe,
    })?;
    ui::ok(&format!(
        "exported {} session(s) to {}",
        result.session_count,
        result.output_dir.display()
    ));
    Ok(())
}

fn session_dirs(notes_dir: &Path, stem: Option<&str>, all: bool) -> Result<Vec<PathBuf>> {
    if let Some(stem) = stem {
        let path = notes_dir.join(stem);
        if !path.is_dir() {
            bail!("session notes not found: {}", path.display());
        }
        return Ok(vec![path]);
    }
    let mut sessions: Vec<_> = std::fs::read_dir(notes_dir)
        .with_context(|| format!("reading {}", notes_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    sessions.sort();
    if all && sessions.is_empty() {
        bail!("no session notes found in {}", notes_dir.display());
    }
    Ok(sessions)
}
