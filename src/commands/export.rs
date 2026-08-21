//! Offline campaign exports for sharing notes outside SessionSmith.

use anyhow::{bail, Context, Result};
use pulldown_cmark::{html, Options, Parser};
use std::path::{Path, PathBuf};

use crate::cli::{ExportArgs, ExportFormat};
use crate::{commands, ui};

const HTML_CSS: &str = include_str!("export.css");

pub async fn run(args: ExportArgs) -> Result<()> {
    let campaign = commands::load_campaign_or_die(&commands::resolve_campaign(None)?)?;
    let sessions = session_dirs(&campaign.notes_dir(), args.stem.as_deref(), args.all)?;
    let output = args.out.unwrap_or_else(|| PathBuf::from("exports").join(campaign.slug()));

    match args.format {
        ExportFormat::Html => export_html(&campaign.campaign.name, &sessions, &output, args.player_safe)?,
        ExportFormat::Obsidian => export_obsidian(&campaign.campaign.name, &sessions, &output, args.player_safe)?,
    }
    ui::ok(&format!("exported {} session(s) to {}", sessions.len(), output.display()));
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

fn artifact_files(session: &Path, player_safe: bool) -> Result<Vec<PathBuf>> {
    let mut files: Vec<_> = std::fs::read_dir(session)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .filter(|path| {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            !player_safe || (!name.starts_with("dm-notes") && !name.starts_with("bullets"))
        })
        .collect();
    files.sort();
    Ok(files)
}

fn export_html(campaign: &str, sessions: &[PathBuf], output: &Path, player_safe: bool) -> Result<()> {
    std::fs::create_dir_all(output)?;
    let mut index = String::new();
    for session in sessions {
        let stem = session.file_name().and_then(|name| name.to_str()).unwrap_or("session");
        let mut body = String::new();
        for artifact in artifact_files(session, player_safe)? {
            let markdown = std::fs::read_to_string(&artifact)?;
            let title = artifact.file_stem().and_then(|name| name.to_str()).unwrap_or("notes");
            body.push_str(&format!("<section><h2>{}</h2>", escape_html(title)));
            let parser = Parser::new_ext(&markdown, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH);
            html::push_html(&mut body, parser);
            body.push_str("</section>");
        }
        std::fs::write(output.join(format!("{stem}.html")), html_page(campaign, stem, &body))?;
        index.push_str(&format!("<li><a href=\"{}.html\">{}</a></li>", escape_html(stem), escape_html(stem)));
    }
    std::fs::write(output.join("index.html"), html_page(campaign, campaign, &format!("<ul>{index}</ul>")))?;
    Ok(())
}

fn export_obsidian(campaign: &str, sessions: &[PathBuf], output: &Path, player_safe: bool) -> Result<()> {
    let root = output.join(sanitize_name(campaign));
    for session in sessions {
        let stem = session.file_name().and_then(|name| name.to_str()).unwrap_or("session");
        let destination = root.join("Sessions").join(stem);
        std::fs::create_dir_all(&destination)?;
        for artifact in artifact_files(session, player_safe)? {
            let source = std::fs::read_to_string(&artifact)?;
            let kind = artifact.file_stem().and_then(|name| name.to_str()).unwrap_or("notes");
            let front_matter = format!("---\ncampaign: \"{}\"\nsession: \"{}\"\nartifact: \"{}\"\n---\n\n", campaign, stem, kind);
            std::fs::write(destination.join(artifact.file_name().unwrap()), format!("{front_matter}{source}"))?;
        }
    }
    Ok(())
}

fn html_page(campaign: &str, title: &str, body: &str) -> String {
    format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title><style>{HTML_CSS}</style></head><body><main><p class=\"campaign\">{}</p><h1>{}</h1>{body}</main></body></html>", escape_html(title), escape_html(campaign), escape_html(title))
}

fn escape_html(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn sanitize_name(value: &str) -> String {
    value.chars().map(|ch| if ch.is_alphanumeric() { ch } else { '-' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn player_safe_excludes_gm_artifacts() {
        let temp = tempdir().unwrap();
        let session = temp.path().join("session");
        std::fs::create_dir(&session).unwrap();
        for name in ["recap.md", "story.md", "dm-notes.md", "bullets.md"] {
            std::fs::write(session.join(name), "notes").unwrap();
        }
        let names: Vec<_> = artifact_files(&session, true).unwrap().iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, ["recap.md", "story.md"]);
    }
}