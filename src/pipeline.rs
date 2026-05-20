//! Orchestrates the LLM passes: bullets first, then derived artifacts.
//! Also handles the rolling campaign log merge.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{CampaignConfig, GlobalConfig};
use crate::llm::{self, ChatMessage, ChatOptions, LlmBackend, Role};
use crate::presets::Preset;
use crate::prompts::{self, Artifact};

#[derive(Debug, Clone)]
pub struct PipelineOpts {
    pub artifacts: Vec<Artifact>,
    pub resume: bool,
    pub force: bool,
    pub update_log: bool,
    pub model_override: Option<String>,
}

pub struct Session {
    pub stem: String,
    pub transcript_path: PathBuf,
    pub notes_dir: PathBuf,
}

impl Session {
    pub fn new(transcript: &Path, notes_root: &Path) -> Result<Self> {
        let stem = transcript.file_stem()
            .ok_or_else(|| anyhow!("no stem for {}", transcript.display()))?
            .to_string_lossy().to_string();
        let notes_dir = notes_root.join(&stem);
        std::fs::create_dir_all(&notes_dir)?;
        Ok(Self { stem, transcript_path: transcript.to_path_buf(), notes_dir })
    }
}

pub async fn run_notes(
    session: &Session,
    g: &GlobalConfig,
    campaign: &CampaignConfig,
    preset: &Preset,
    opts: &PipelineOpts,
) -> Result<()> {
    let backend = llm::build(g)?;
    let model = opts.model_override.clone()
        .or_else(|| g.backend.model.clone())
        .ok_or_else(|| anyhow!("no LLM model configured (set in ~/.config/sessionsmith/config.toml or pass --model)"))?;

    let chat_opts = ChatOptions {
        model: model.clone(),
        temperature: Some(0.4),
        max_tokens: None,
        timeout: Duration::from_secs(g.runtime.timeout_secs),
        think: g.runtime.think,
    };

    let transcript = std::fs::read_to_string(&session.transcript_path)
        .with_context(|| format!("reading {}", session.transcript_path.display()))?;

    // --- Pass A: bullets (always; everything else derives from it) ---
    let needs_derived = opts.artifacts.iter().any(|a| !matches!(a, Artifact::Bullets));
    let want_bullets = opts.artifacts.contains(&Artifact::Bullets) || needs_derived;

    let bullets_path = session.notes_dir.join(Artifact::Bullets.filename());
    let bullets = if want_bullets {
        if opts.resume && bullets_path.exists() && !opts.force {
            crate::ui::ok(&format!("bullets: reuse {}", bullets_path.display()));
            std::fs::read_to_string(&bullets_path)?
        } else {
            let sys = prompts::system_for(Artifact::Bullets, campaign, preset);
            let user = prompts::user_bullets_from_transcript(&transcript);
            let text = call_one(backend.as_ref(), &chat_opts, Artifact::Bullets, sys, user).await?;
            std::fs::write(&bullets_path, &text)?;
            crate::ui::ok(&format!("wrote {}", bullets_path.display()));
            text
        }
    } else {
        String::new()
    };

    // --- Derived passes ---
    let derived: Vec<Artifact> = opts.artifacts.iter().copied()
        .filter(|a| !matches!(a, Artifact::Bullets))
        .collect();

    if !derived.is_empty() {
        let parallel = g.runtime.parallel_passes && backend.name() != "ollama";
        if parallel {
            let mut handles = Vec::new();
            for a in derived {
                let out = session.notes_dir.join(a.filename());
                if opts.resume && out.exists() && !opts.force {
                    crate::ui::ok(&format!("{}: reuse {}", a.label(), out.display()));
                    continue;
                }
                let sys = prompts::system_for(a, campaign, preset);
                let user = prompts::user_from_bullets(&bullets);
                let opts2 = chat_opts.clone();
                let g2 = g.clone();
                handles.push(tokio::spawn(async move {
                    let b = llm::build(&g2)?;
                    let text = call_one(b.as_ref(), &opts2, a, sys, user).await?;
                    std::fs::write(&out, &text)?;
                    crate::ui::ok(&format!("wrote {}", out.display()));
                    Ok::<(), anyhow::Error>(())
                }));
            }
            for h in handles {
                if let Err(e) = h.await? {
                    crate::ui::warn(&format!("artifact failed — {e:#}"));
                }
            }
        } else {
            for a in derived {
                let out = session.notes_dir.join(a.filename());
                if opts.resume && out.exists() && !opts.force {
                    crate::ui::ok(&format!("{}: reuse {}", a.label(), out.display()));
                    continue;
                }
                let sys = prompts::system_for(a, campaign, preset);
                let user = prompts::user_from_bullets(&bullets);
                match call_one(backend.as_ref(), &chat_opts, a, sys, user).await {
                    Ok(text) => {
                        std::fs::write(&out, &text)?;
                        crate::ui::ok(&format!("wrote {}", out.display()));
                    }
                    Err(e) => {
                        crate::ui::warn(&format!("{}: failed — {e:#}", a.label()));
                    }
                }
            }
        }
    }

    // --- Campaign log merge ---
    if opts.update_log {
        let summary_path = session.notes_dir.join(Artifact::Summary.filename());
        if summary_path.exists() {
            let summary = std::fs::read_to_string(&summary_path)?;
            if let Err(e) = update_campaign_log(g, campaign, preset, &session.stem, &summary, &chat_opts).await {
                crate::ui::warn(&format!("campaign log: {e:#}"));
                crate::ui::warn("  run `sessionsmith log rebuild` to retry");
            }
        } else {
            crate::ui::warn("no summary.md present; skipping campaign log merge");
        }
    }

    Ok(())
}

async fn call_one(backend: &dyn LlmBackend, chat_opts: &ChatOptions, a: Artifact, sys: String, user: String) -> Result<String> {
    let pb = crate::ui::spinner(&format!("{}: {} via {}", a.label(), chat_opts.model, backend.name()));
    let messages = vec![
        ChatMessage { role: Role::System, content: sys },
        ChatMessage { role: Role::User, content: user },
    ];
    let res = llm::collect(backend, messages, chat_opts.clone(), Some(&pb)).await;
    pb.finish_and_clear();
    res
}

async fn update_campaign_log(
    g: &GlobalConfig,
    campaign: &CampaignConfig,
    preset: &Preset,
    session_stem: &str,
    summary: &str,
    chat_opts: &ChatOptions,
) -> Result<()> {
    let log_path = campaign.notes_dir().join("_campaign-log.md");
    let existing = if log_path.exists() { std::fs::read_to_string(&log_path)? } else { String::new() };

    let date = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date()
        .to_string();

    let sys = prompts::campaign_log_system(campaign, preset);
    let user = prompts::user_campaign_log_merge(&existing, summary, &date, session_stem);
    let backend = llm::build(g)?;
    let pb = crate::ui::spinner("campaign log: merging");
    let messages = vec![
        ChatMessage { role: Role::System, content: sys },
        ChatMessage { role: Role::User, content: user },
    ];
    let merged = llm::collect(backend.as_ref(), messages, chat_opts.clone(), Some(&pb)).await?;
    pb.finish_and_clear();

    let tmp = log_path.with_extension("md.tmp");
    std::fs::write(&tmp, merged)?;
    std::fs::rename(&tmp, &log_path)?;
    crate::ui::ok(&format!("updated {}", log_path.display()));
    Ok(())
}

pub fn parse_artifacts(spec: &str) -> Result<Vec<Artifact>> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let p = part.trim().to_lowercase();
        if p.is_empty() { continue; }
        if p == "all" {
            return Ok(prompts::ALL_ARTIFACTS.to_vec());
        }
        match Artifact::from_id(&p) {
            Some(a) => if !out.contains(&a) { out.push(a); },
            None => return Err(anyhow!("unknown artifact '{p}'. Valid: bullets, dm-notes, recap, summary, story, quotes")),
        }
    }
    Ok(out)
}
