use anyhow::Result;
use inquire::{MultiSelect, Select};
use std::path::{Path, PathBuf};

use crate::cli::NotesArgs;
use crate::config::GlobalConfig;
use crate::pipeline::{self, PipelineOpts, Session};
use crate::presets;
use crate::prompts::{self, Artifact};
use crate::{commands, deps, ui};

pub async fn run(args: NotesArgs) -> Result<()> {
    ui::header("SessionSmith · notes");
    deps::ensure_dirs()?;

    let camp_path = commands::resolve_campaign(None)?;
    let campaign = commands::load_campaign_or_die(&camp_path)?;
    let preset = presets::load(&campaign.system.preset)?;

    let mut g = GlobalConfig::load_or_default()?;
    if let Some(b) = args.backend { g.backend.kind = b; }
    if let Some(m) = &args.model { g.backend.model = Some(m.clone()); }

    let transcript = match args.transcript {
        Some(p) => p,
        None => pick_transcript_interactively(&campaign.transcripts_dir())?,
    };
    if !transcript.exists() {
        anyhow::bail!("transcript not found: {}", transcript.display());
    }

    let artifacts = resolve_artifacts(&args.artifacts, &campaign.outputs.default)?;
    let session = Session::new(&transcript, &campaign.notes_dir())?;

    let opts = PipelineOpts {
        artifacts,
        resume: args.resume,
        force: args.force,
        update_log: !args.no_log,
        model_override: args.model,
    };
    pipeline::run_notes(&session, &g, &campaign, &preset, &opts).await?;
    ui::ok(&format!("artifacts in {}", session.notes_dir.display()));
    Ok(())
}

pub fn pick_transcript_interactively(transcripts_dir: &Path) -> Result<PathBuf> {
    if !transcripts_dir.exists() {
        anyhow::bail!("no transcripts/ directory yet — run `sessionsmith transcribe` first.");
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(transcripts_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("txt"))
        .collect();
    files.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).and_then(|m| m.modified()).ok()));
    if files.is_empty() { anyhow::bail!("no .txt transcripts in transcripts/"); }
    let labels: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
    let chosen = Select::new("Pick a transcript:", labels.clone()).prompt()?;
    Ok(files[labels.iter().position(|l| *l == chosen).unwrap()].clone())
}

pub fn resolve_artifacts(cli: &Option<String>, defaults: &[String]) -> Result<Vec<Artifact>> {
    if let Some(spec) = cli {
        return pipeline::parse_artifacts(spec);
    }
    // Use campaign defaults; if none, interactively pick.
    if !defaults.is_empty() {
        let joined = defaults.join(",");
        return pipeline::parse_artifacts(&joined);
    }
    let labels: Vec<String> = prompts::ALL_ARTIFACTS.iter().map(|a| a.id().to_string()).collect();
    let chosen = MultiSelect::new("Pick artifacts to generate:", labels.clone())
        .with_default(&(0..labels.len()).collect::<Vec<_>>())
        .prompt()?;
    let joined = chosen.join(",");
    pipeline::parse_artifacts(&joined)
}
