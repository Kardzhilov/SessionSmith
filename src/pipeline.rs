//! Orchestrates the LLM passes: bullets first, then derived artifacts.
//! Also handles the rolling campaign log merge.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{CampaignConfig, GlobalConfig};
use crate::llm::{self, ChatMessage, ChatOptions, LlmBackend, LlmUsage, Role};
use crate::presets::Preset;
use crate::prompts::{self, Artifact};

#[derive(Debug, Clone)]
pub struct PipelineOpts {
    pub artifacts: Vec<Artifact>,
    pub resume: bool,
    pub force: bool,
    pub update_log: bool,
    pub model_override: Option<String>,
    /// Write artifacts to `<name>.candidate.md` instead of overwriting, so the
    /// user can compare against the existing version before keeping one.
    pub candidate: bool,
}

#[derive(Debug, Clone)]
pub struct ArtifactUsage {
    pub artifact: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct PipelineReport {
    pub model: String,
    pub usages: Vec<ArtifactUsage>,
}

struct CampaignLogCall {
    artifact: &'static str,
    input: String,
    output: String,
    usage: Option<LlmUsage>,
}

impl PipelineReport {
    fn record(
        &mut self,
        artifact: impl Into<String>,
        input: &str,
        output: &str,
        usage: Option<LlmUsage>,
    ) {
        let (input_tokens, output_tokens) = usage
            .map(|usage| (usage.input_tokens, usage.output_tokens))
            .unwrap_or_else(|| (approximate_tokens(input), approximate_tokens(output)));
        self.usages.push(ArtifactUsage {
            artifact: artifact.into(),
            input_tokens,
            output_tokens,
            estimated_cost_usd: estimated_cost(&self.model, input_tokens, output_tokens),
        });
    }

    fn emit(&self) {
        if self.usages.is_empty() {
            return;
        }
        let input: usize = self.usages.iter().map(|usage| usage.input_tokens).sum();
        let output: usize = self.usages.iter().map(|usage| usage.output_tokens).sum();
        let cost: Option<f64> = self.usages.iter().try_fold(0.0, |total, usage| {
            usage.estimated_cost_usd.map(|cost| total + cost)
        });
        let mut lines = vec![
            format!("model: {}", self.model),
            format!("tokens: ~{input} input / ~{output} output"),
        ];
        lines.push(match cost {
            Some(cost) => format!("estimated API cost: ~${cost:.4}"),
            None => "estimated API cost: unavailable for this model".into(),
        });
        crate::ui::panel("Generation usage", &lines);
    }
}

fn approximate_tokens(text: &str) -> usize {
    text.split_whitespace().count()
}

fn estimated_cost(model: &str, input_tokens: usize, output_tokens: usize) -> Option<f64> {
    let model = model.to_ascii_lowercase();
    let (_, input_per_million, output_per_million) = [
        ("gpt-4o-mini", 0.15, 0.60),
        ("gpt-4o", 2.50, 10.00),
        ("gpt-4.1-mini", 0.40, 1.60),
        ("gpt-4.1", 2.00, 8.00),
        ("claude-haiku", 0.80, 4.00),
        ("claude-sonnet", 3.00, 15.00),
        ("claude-opus", 15.00, 75.00),
    ]
    .into_iter()
    .find(|(prefix, _, _)| model.starts_with(prefix))?;
    Some(
        input_tokens as f64 * input_per_million / 1_000_000.0
            + output_tokens as f64 * output_per_million / 1_000_000.0,
    )
}

/// Output filename for an artifact, honouring candidate (compare) mode.
pub fn artifact_file(a: Artifact, candidate: bool) -> String {
    candidate_name(a.filename(), candidate)
}

/// Turn `bullets.md` into `bullets.candidate.md` (and `.json` similarly) when
/// `candidate` is set.
pub fn candidate_name(fname: &str, candidate: bool) -> String {
    if !candidate {
        return fname.to_string();
    }
    if let Some(stem) = fname.strip_suffix(".md") {
        format!("{stem}.candidate.md")
    } else if let Some(stem) = fname.strip_suffix(".json") {
        format!("{stem}.candidate.json")
    } else {
        format!("{fname}.candidate")
    }
}

pub struct Session {
    pub stem: String,
    pub transcript_path: PathBuf,
    pub notes_dir: PathBuf,
}

impl Session {
    pub fn new(transcript: &Path, notes_root: &Path) -> Result<Self> {
        let stem = transcript
            .file_stem()
            .ok_or_else(|| anyhow!("no stem for {}", transcript.display()))?
            .to_string_lossy()
            .to_string();
        let notes_dir = notes_root.join(&stem);
        std::fs::create_dir_all(&notes_dir)?;
        Ok(Self {
            stem,
            transcript_path: transcript.to_path_buf(),
            notes_dir,
        })
    }
}

pub async fn run_notes(
    session: &Session,
    g: &GlobalConfig,
    campaign: &CampaignConfig,
    preset: &Preset,
    opts: &PipelineOpts,
) -> Result<PipelineReport> {
    let backend = llm::build(g)?;
    run_notes_with_backend(session, g, campaign, preset, opts, backend.as_ref()).await
}

async fn run_notes_with_backend(
    session: &Session,
    g: &GlobalConfig,
    campaign: &CampaignConfig,
    preset: &Preset,
    opts: &PipelineOpts,
    backend: &dyn LlmBackend,
) -> Result<PipelineReport> {
    let model = opts.model_override.clone()
        .or_else(|| g.backend.model.clone())
        .ok_or_else(|| anyhow!("no LLM model configured (set in ~/.config/sessionsmith/config.toml or pass --model)"))?;

    let chat_opts = ChatOptions {
        model: model.clone(),
        temperature: Some(0.4),
        max_tokens: None,
        timeout: Duration::from_secs(g.runtime.timeout_secs),
        think: g.runtime.think,
        num_ctx: g.effective_num_ctx(),
        format: None,
    };

    let transcript = std::fs::read_to_string(&session.transcript_path)
        .with_context(|| format!("reading {}", session.transcript_path.display()))?;
    let mut report = PipelineReport {
        model: model.clone(),
        usages: Vec::new(),
    };

    // --- Pass A: bullets (everything except quotes derives from it) ---
    let needs_derived = opts
        .artifacts
        .iter()
        .any(|a| !matches!(a, Artifact::Bullets | Artifact::Quotes));
    let regen_bullets = opts.artifacts.contains(&Artifact::Bullets);
    let bullets_real = session.notes_dir.join(Artifact::Bullets.filename());
    let bullets_out = session
        .notes_dir
        .join(artifact_file(Artifact::Bullets, opts.candidate));

    let bullets = if regen_bullets {
        crate::ui::phase("Outline");
        if opts.resume && !opts.candidate && bullets_real.exists() && !opts.force {
            crate::ui::ok(&format!("bullets: reuse {}", bullets_real.display()));
            std::fs::read_to_string(&bullets_real)?
        } else {
            let sys = prompts::system_for(Artifact::Bullets, campaign, preset);
            let text = generate_bullets(backend, &chat_opts, sys, &transcript, g).await?;
            std::fs::write(&bullets_out, &text)?;
            report.record(Artifact::Bullets.label(), &transcript, &text, None);
            crate::ui::ok(&format!("wrote {}", bullets_out.display()));
            text
        }
    } else if needs_derived {
        // Bullets are needed as input but not being (re)generated: reuse the
        // existing outline, or create it once if none exists.
        if bullets_real.exists() {
            std::fs::read_to_string(&bullets_real)?
        } else {
            crate::ui::phase("Outline");
            let sys = prompts::system_for(Artifact::Bullets, campaign, preset);
            let text = generate_bullets(backend, &chat_opts, sys, &transcript, g).await?;
            std::fs::write(&bullets_real, &text)?;
            report.record(Artifact::Bullets.label(), &transcript, &text, None);
            crate::ui::ok(&format!("wrote {}", bullets_real.display()));
            text
        }
    } else {
        String::new()
    };

    // --- Derived passes ---
    let derived: Vec<Artifact> = opts
        .artifacts
        .iter()
        .copied()
        .filter(|a| !matches!(a, Artifact::Bullets | Artifact::Quotes))
        .collect();

    if !derived.is_empty() {
        crate::ui::phase("Notes");
        let parallel = g.runtime.parallel_passes && backend.name() != "ollama";
        if parallel {
            let mut handles = Vec::new();
            for a in derived {
                let out = session.notes_dir.join(artifact_file(a, opts.candidate));
                if opts.resume && !opts.candidate && out.exists() && !opts.force {
                    crate::ui::ok(&format!("{}: reuse {}", a.label(), out.display()));
                    continue;
                }
                let sys = prompts::system_for(a, campaign, preset);
                let user = prompts::user_from_bullets(&bullets);
                let usage_input = user.clone();
                let opts2 = chat_opts.clone();
                let g2 = g.clone();
                handles.push(tokio::spawn(async move {
                    let b = llm::build(&g2)?;
                    let response = call_one_with_usage(b.as_ref(), &opts2, a, sys, user).await?;
                    std::fs::write(&out, &response.text)?;
                    crate::ui::ok(&format!("wrote {}", out.display()));
                    Ok::<(Artifact, String, llm::CollectedResponse), anyhow::Error>((
                        a,
                        usage_input,
                        response,
                    ))
                }));
            }
            for h in handles {
                match h.await? {
                    Ok((artifact, input, response)) => {
                        report.record(artifact.label(), &input, &response.text, response.usage)
                    }
                    Err(e) => crate::ui::warn(&format!("artifact failed — {e:#}")),
                }
            }
        } else {
            for a in derived {
                let out = session.notes_dir.join(artifact_file(a, opts.candidate));
                if opts.resume && !opts.candidate && out.exists() && !opts.force {
                    crate::ui::ok(&format!("{}: reuse {}", a.label(), out.display()));
                    continue;
                }
                let sys = prompts::system_for(a, campaign, preset);
                let user = prompts::user_from_bullets(&bullets);
                let usage_input = user.clone();
                match call_one_with_usage(backend, &chat_opts, a, sys, user).await {
                    Ok(response) => {
                        std::fs::write(&out, &response.text)?;
                        report.record(a.label(), &usage_input, &response.text, response.usage);
                        crate::ui::ok(&format!("wrote {}", out.display()));
                    }
                    Err(e) => {
                        crate::ui::warn(&format!("{}: failed — {e:#}", a.label()));
                    }
                }
            }
        }
    }

    // --- Quotes (verbatim, timestamped — read from the transcript directly) ---
    if opts.artifacts.contains(&Artifact::Quotes) {
        let out = session
            .notes_dir
            .join(artifact_file(Artifact::Quotes, opts.candidate));
        if opts.resume && !opts.candidate && out.exists() && !opts.force {
            crate::ui::ok(&format!(
                "{}: reuse {}",
                Artifact::Quotes.label(),
                out.display()
            ));
        } else {
            let ts = timestamped_transcript(session);
            let sys = prompts::system_for(Artifact::Quotes, campaign, preset);
            match generate_quotes(backend, &chat_opts, sys, &ts, g).await {
                Ok(text) => {
                    // Grounding: drop any quote not found verbatim in the
                    // transcript, and pin each timestamp to where it occurs.
                    let grounded = ground_quotes(&text, &ts);
                    std::fs::write(&out, &grounded)?;
                    report.record(Artifact::Quotes.label(), &ts, &grounded, None);
                    crate::ui::ok(&format!("wrote {}", out.display()));
                }
                Err(e) => crate::ui::warn(&format!("{}: failed — {e:#}", Artifact::Quotes.label())),
            }
        }
    }

    // --- Structured JSON companion (opt-in) ---
    if g.runtime.structured && opts.artifacts.contains(&Artifact::DmNotes) {
        let out = session
            .notes_dir
            .join(candidate_name("dm-notes.json", opts.candidate));
        if opts.resume && !opts.candidate && out.exists() && !opts.force {
            crate::ui::ok(&format!("dm-notes.json: reuse {}", out.display()));
        } else {
            let mut sopts = chat_opts.clone();
            sopts.format = Some(prompts::dm_notes_schema());
            let sys = prompts::dm_notes_structured_system(campaign, preset);
            let user = prompts::user_from_bullets(&bullets);
            let usage_input = user.clone();
            match call_one_with_usage(backend, &sopts, Artifact::DmNotes, sys, user).await {
                Ok(response) => {
                    let text = response.text;
                    // Best-effort pretty-print; write raw if not valid JSON.
                    let pretty = serde_json::from_str::<serde_json::Value>(&text)
                        .ok()
                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                        .unwrap_or(text);
                    std::fs::write(&out, &pretty)?;
                    report.record("dm-notes.json", &usage_input, &pretty, response.usage);
                    crate::ui::ok(&format!("wrote {}", out.display()));
                }
                Err(e) => crate::ui::warn(&format!("dm-notes.json: failed — {e:#}")),
            }
        }
    }

    // --- Campaign log merge ---
    // Skipped in candidate mode: the log is only touched once the user keeps
    // the regenerated artifacts.
    if opts.update_log && !opts.candidate {
        crate::ui::phase("Campaign log");
        let summary_path = session.notes_dir.join(Artifact::Summary.filename());
        if summary_path.exists() {
            let summary = std::fs::read_to_string(&summary_path)?;
            if let Err(e) = update_campaign_log(
                g,
                campaign,
                preset,
                &session.stem,
                &summary,
                &chat_opts,
                &mut report,
            )
            .await
            {
                crate::ui::warn(&format!("campaign log: {e:#}"));
                crate::ui::warn("  run `sessionsmith log rebuild` to retry");
            }
        } else {
            crate::ui::warn("no summary.md present; skipping campaign log merge");
        }
    }

    // --- Local search index (opt-in, on by default) ---
    // Candidate artifacts aren't indexed until kept.
    if g.runtime.index && !opts.candidate {
        if let Err(e) = crate::index::record_session(campaign, &session.stem, &session.notes_dir) {
            crate::ui::warn(&format!("index: {e:#}"));
        }
    }

    // Notes generation is done — release the LLM model's VRAM so it doesn't sit
    // idle blocking the next transcription (or another GPU workload).
    crate::llm::free_vram(g).await;

    report.emit();
    Ok(report)
}

async fn call_one(
    backend: &dyn LlmBackend,
    chat_opts: &ChatOptions,
    a: Artifact,
    sys: String,
    user: String,
) -> Result<String> {
    Ok(call_one_with_usage(backend, chat_opts, a, sys, user)
        .await?
        .text)
}

async fn call_one_with_usage(
    backend: &dyn LlmBackend,
    chat_opts: &ChatOptions,
    a: Artifact,
    sys: String,
    user: String,
) -> Result<llm::CollectedResponse> {
    let pb = crate::ui::spinner(&format!(
        "{}: {} via {}",
        a.label(),
        chat_opts.model,
        backend.name()
    ));
    let messages = vec![
        ChatMessage {
            role: Role::System,
            content: sys,
        },
        ChatMessage {
            role: Role::User,
            content: user,
        },
    ];
    let res = llm::collect_with_usage(backend, messages, chat_opts.clone(), Some(&pb))
        .await
        .map_err(|error| anyhow!("{}: {error:#}", a.label()));
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
    report: &mut PipelineReport,
) -> Result<()> {
    let notes_dir = campaign.notes_dir();
    let md_path = crate::campaign_log::md_path(&notes_dir);

    // Load the structured state: prefer the JSON sidecar, else migrate a legacy
    // marker-based markdown, else (unknown/clean legacy md) rebuild from scratch.
    let mut log = if let Some(state) = crate::campaign_log::load_json(&notes_dir) {
        state
    } else if md_path.exists() {
        let text = std::fs::read_to_string(&md_path).unwrap_or_default();
        if text.trim().is_empty() {
            crate::campaign_log::CampaignLog::default()
        } else if crate::campaign_log::is_v2(&text) {
            crate::ui::info("campaign log: migrating markers into clean format");
            crate::campaign_log::parse(&text)
        } else {
            crate::ui::info("campaign log: rebuilding to deduplicated format");
            return rebuild_campaign_log_with_report(g, campaign, preset, chat_opts, Some(report))
                .await;
        }
    } else {
        crate::campaign_log::CampaignLog::default()
    };

    let existing_date = log
        .blocks
        .iter()
        .find(|block| block.id == session_stem)
        .map(|block| block.date.as_str());
    let summary_mtime = std::fs::metadata(notes_dir.join(session_stem).join("summary.md"))
        .and_then(|metadata| metadata.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now());
    let date = campaign_session_date(campaign, session_stem, existing_date, summary_mtime);

    let backend = llm::build(g)?;
    let pb = crate::ui::spinner("campaign log: writing session entry");
    let (title, body, entry_call) = generate_session_entry(
        backend.as_ref(),
        chat_opts,
        campaign,
        preset,
        summary,
        &date,
    )
    .await?;
    report.record(
        entry_call.artifact,
        &entry_call.input,
        &entry_call.output,
        entry_call.usage,
    );
    log.upsert(session_stem, &date, title, body);
    let latest = log.block_body(session_stem).unwrap_or("").to_string();
    let (threads, threads_call) = generate_threads(
        backend.as_ref(),
        chat_opts,
        campaign,
        preset,
        &log.threads,
        &latest,
    )
    .await?;
    report.record(
        threads_call.artifact,
        &threads_call.input,
        &threads_call.output,
        threads_call.usage,
    );
    log.threads = threads;
    pb.finish_and_clear();

    crate::campaign_log::persist(&notes_dir, &log)?;
    crate::ui::ok(&format!("updated {}", md_path.display()));
    Ok(())
}

fn campaign_session_date(
    campaign: &CampaignConfig,
    stem: &str,
    existing_date: Option<&str>,
    fallback: std::time::SystemTime,
) -> String {
    crate::meta::load(&campaign.transcripts_dir(), stem)
        .and_then(|meta| meta.session_date)
        .or_else(|| existing_date.map(ToOwned::to_owned))
        .unwrap_or_else(|| time::OffsetDateTime::from(fallback).date().to_string())
}

/// Rebuild the campaign log from scratch using every `notes/<stem>/summary.md`,
/// keyed by session stem so re-run duplicates collapse into one entry.
pub async fn rebuild_campaign_log(
    g: &GlobalConfig,
    campaign: &CampaignConfig,
    preset: &Preset,
    chat_opts: &ChatOptions,
) -> Result<()> {
    rebuild_campaign_log_with_report(g, campaign, preset, chat_opts, None).await
}

async fn rebuild_campaign_log_with_report(
    g: &GlobalConfig,
    campaign: &CampaignConfig,
    preset: &Preset,
    chat_opts: &ChatOptions,
    mut report: Option<&mut PipelineReport>,
) -> Result<()> {
    let notes_dir = campaign.notes_dir();
    let existing_log = crate::campaign_log::load_json(&notes_dir);

    // Collect summaries oldest-first so numbering/threads accumulate correctly.
    let mut sessions: Vec<(String, String, String)> = Vec::new(); // (stem, date, summary)
    if notes_dir.exists() {
        let mut dirs: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in std::fs::read_dir(&notes_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let summary = entry.path().join("summary.md");
            if !summary.exists() {
                continue;
            }
            let mtime = std::fs::metadata(&summary)?.modified()?;
            dirs.push((entry.path(), mtime));
        }
        dirs.sort_by_key(|d| d.1);
        for (dir, mtime) in dirs {
            let stem = dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let summary = std::fs::read_to_string(dir.join("summary.md")).unwrap_or_default();
            let existing_date = existing_log
                .as_ref()
                .and_then(|log| log.blocks.iter().find(|block| block.id == stem))
                .map(|block| block.date.as_str());
            let date = campaign_session_date(campaign, &stem, existing_date, mtime);
            sessions.push((stem, date, summary));
        }
    }
    if sessions.is_empty() {
        anyhow::bail!("no notes/*/summary.md files found to rebuild from");
    }

    let backend = llm::build(g)?;
    let mut log = crate::campaign_log::CampaignLog::default();
    let total = sessions.len();
    for (i, (stem, date, summary)) in sessions.into_iter().enumerate() {
        let pb = crate::ui::spinner(&format!("campaign log: {}/{} · {stem}", i + 1, total));
        let (title, body, entry_call) = generate_session_entry(
            backend.as_ref(),
            chat_opts,
            campaign,
            preset,
            &summary,
            &date,
        )
        .await?;
        if let Some(report) = report.as_deref_mut() {
            report.record(
                entry_call.artifact,
                &entry_call.input,
                &entry_call.output,
                entry_call.usage,
            );
        }
        log.upsert(&stem, &date, title, body);
        let latest = log.block_body(&stem).unwrap_or("").to_string();
        let (threads, threads_call) = generate_threads(
            backend.as_ref(),
            chat_opts,
            campaign,
            preset,
            &log.threads,
            &latest,
        )
        .await?;
        if let Some(report) = report.as_deref_mut() {
            report.record(
                threads_call.artifact,
                &threads_call.input,
                &threads_call.output,
                threads_call.usage,
            );
        }
        log.threads = threads;
        pb.finish_and_clear();
        crate::ui::ok(&format!("logged {stem}"));
    }

    crate::campaign_log::persist(&notes_dir, &log)?;
    crate::ui::ok(&format!(
        "rebuilt {}",
        crate::campaign_log::md_path(&notes_dir).display()
    ));
    Ok(())
}

/// Generate a session's log entry, returning `(title, body)`.
async fn generate_session_entry(
    backend: &dyn LlmBackend,
    chat_opts: &ChatOptions,
    campaign: &CampaignConfig,
    preset: &Preset,
    summary: &str,
    date: &str,
) -> Result<(String, String, CampaignLogCall)> {
    let sys = prompts::session_log_entry_system(campaign, preset);
    let user = prompts::user_session_log_entry(summary, date);
    let response = llm::collect_with_usage(
        backend,
        vec![
            ChatMessage {
                role: Role::System,
                content: sys,
            },
            ChatMessage {
                role: Role::User,
                content: user.clone(),
            },
        ],
        chat_opts.clone(),
        None,
    )
    .await?;
    // First non-empty line is the title; the rest is the body.
    let mut lines = response.text.lines();
    let mut title = lines
        .by_ref()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("Untitled Session")
        .trim()
        .trim_matches(['#', '*', '"', ' '])
        .to_string();
    // Defensively strip a leading "Title:" / "Session Title:" label some models
    // prepend despite instructions.
    let low = title.to_lowercase();
    for pfx in ["session title:", "title:"] {
        if low.starts_with(pfx) {
            title = title[pfx.len()..]
                .trim()
                .trim_matches(['*', '"', ' '])
                .to_string();
            break;
        }
    }
    if title.is_empty() {
        title = "Untitled Session".to_string();
    }
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    let body = if body.is_empty() {
        summary.trim().to_string()
    } else {
        body
    };
    Ok((
        title,
        body,
        CampaignLogCall {
            artifact: "campaign-log entry",
            input: user,
            output: response.text,
            usage: response.usage,
        },
    ))
}

/// (Re)generate the ongoing-threads section from the current threads + latest.
async fn generate_threads(
    backend: &dyn LlmBackend,
    chat_opts: &ChatOptions,
    campaign: &CampaignConfig,
    preset: &Preset,
    current: &str,
    latest_session: &str,
) -> Result<(String, CampaignLogCall)> {
    let sys = prompts::ongoing_threads_system(campaign, preset);
    let user = prompts::user_ongoing_threads(current, latest_session);
    let response = llm::collect_with_usage(
        backend,
        vec![
            ChatMessage {
                role: Role::System,
                content: sys,
            },
            ChatMessage {
                role: Role::User,
                content: user.clone(),
            },
        ],
        chat_opts.clone(),
        None,
    )
    .await?;
    Ok((
        response.text.trim().to_string(),
        CampaignLogCall {
            artifact: "campaign-log threads",
            input: user,
            output: response.text,
            usage: response.usage,
        },
    ))
}

pub fn parse_artifacts(spec: &str) -> Result<Vec<Artifact>> {
    let mut out = Vec::new();
    for part in spec.split(',') {
        let p = part.trim().to_lowercase();
        if p.is_empty() {
            continue;
        }
        if p == "all" {
            return Ok(prompts::ALL_ARTIFACTS.to_vec());
        }
        match Artifact::from_id(&p) {
            Some(a) => {
                if !out.contains(&a) {
                    out.push(a);
                }
            }
            None => {
                return Err(anyhow!(
                "unknown artifact '{p}'. Valid: bullets, dm-notes, recap, summary, story, quotes"
            ))
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Bullets generation with transcript chunking (map-reduce)
// ---------------------------------------------------------------------------

/// Build a timestamped transcript for quote extraction. Prefers the `.srt`
/// (each cue rendered as `[HH:MM:SS] text`); falls back to the plain `.txt`
/// when no SRT is available.
fn timestamped_transcript(session: &Session) -> String {
    let srt_path = session.transcript_path.with_extension("srt");
    if let Ok(srt) = std::fs::read_to_string(&srt_path) {
        let ts = srt_to_timestamped(&srt);
        if !ts.trim().is_empty() {
            return ts;
        }
    }
    std::fs::read_to_string(&session.transcript_path).unwrap_or_default()
}

/// Verify each extracted quote against the transcript, dropping any that are
/// not present verbatim (after light normalisation) and pinning the timestamp
/// to where the quote actually occurs. This is what guarantees quotes are never
/// fabricated, regardless of how well the LLM followed instructions.
fn ground_quotes(raw: &str, timestamped: &str) -> String {
    let grounder = QuoteGrounder::build(timestamped);
    let mut out = String::new();
    let mut kept = 0usize;

    let lines: Vec<&str> = raw.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if let Some(quote) = extract_quoted(lines[i]) {
            if quote.split_whitespace().count() >= 3 {
                // Speaker from a following attribution line, if any.
                let mut speaker = String::from("Unknown");
                for l in lines.iter().skip(i + 1).take(2) {
                    if let Some(s) = parse_speaker(l) {
                        speaker = s;
                        break;
                    }
                }
                if let Some(ts) = grounder.locate(&quote) {
                    out.push_str(&format!(
                        "> *\"{}\"*\n> — {speaker} — [{ts}]\n\n",
                        quote.trim()
                    ));
                    kept += 1;
                }
            }
        }
        i += 1;
    }

    if kept == 0 {
        return "_No verbatim quotes could be extracted from this session._\n".to_string();
    }
    out
}

/// Extract the quoted text from a line: prefers `*"…"*`, else the text between
/// the first and last double-quote on a `>`-prefixed line.
fn extract_quoted(line: &str) -> Option<String> {
    if let (Some(a), Some(b)) = (line.find("*\""), line.rfind("\"*")) {
        if b > a + 2 {
            return Some(line[a + 2..b].trim().to_string());
        }
    }
    let t = line.trim_start();
    if t.starts_with('>') {
        let first = t.find('"')?;
        let last = t.rfind('"')?;
        if last > first + 1 {
            return Some(t[first + 1..last].trim().to_string());
        }
    }
    None
}

/// Parse a speaker from an attribution line like `> — Player 1 — [00:12:34]`.
fn parse_speaker(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.split('—').collect();
    if parts.len() >= 2 {
        let s = parts[1].trim();
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    None
}

/// Normalised transcript with a per-character timestamp map, for verifying and
/// locating quotes.
struct QuoteGrounder {
    norm: String,
    ts_at: Vec<String>,
}

impl QuoteGrounder {
    fn build(timestamped: &str) -> Self {
        let mut norm = String::new();
        let mut ts_at: Vec<String> = Vec::new();
        for line in timestamped.lines() {
            let (ts, text) = split_ts_line(line);
            // Push one timestamp entry per *byte* so `norm.find` (a byte offset)
            // maps back correctly even with multi-byte characters.
            for ch in normalize(text).chars() {
                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                norm.push_str(s);
                for _ in 0..s.len() {
                    ts_at.push(ts.clone());
                }
            }
            // Separator space between lines (carries the same timestamp).
            norm.push(' ');
            ts_at.push(ts);
        }
        Self { norm, ts_at }
    }

    /// If `quote` occurs (normalised) in the transcript, return its timestamp.
    fn locate(&self, quote: &str) -> Option<String> {
        let q = normalize(quote);
        let q = q.trim();
        if q.is_empty() {
            return None;
        }
        let pos = self.norm.find(q)?;
        self.ts_at
            .get(pos)
            .cloned()
            .or_else(|| Some("00:00:00".to_string()))
    }
}

/// Split a `[HH:MM:SS] text` line into `(timestamp, text)`.
fn split_ts_line(line: &str) -> (String, &str) {
    if let Some(rest) = line.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let ts = rest[..end].trim().to_string();
            let text = rest[end + 1..].trim_start();
            return (ts, text);
        }
    }
    ("00:00:00".to_string(), line)
}

/// Lowercase, map every non-alphanumeric char to a space, and collapse runs of
/// whitespace to a single space — so punctuation / quote-style differences
/// don't defeat the verbatim check.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

/// Convert SRT text into `[HH:MM:SS] text` lines (one per cue).
fn srt_to_timestamped(srt: &str) -> String {
    let mut out = String::new();
    let normalized = srt.replace("\r\n", "\n");
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        // Find the "HH:MM:SS,mmm --> ..." timing line.
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let start = lines[ti]
            .split("-->")
            .next()
            .unwrap_or("")
            .trim()
            .split(',')
            .next()
            .unwrap_or("")
            .trim();
        if start.is_empty() {
            continue;
        }
        let text = lines[ti + 1..].join(" ").trim().to_string();
        if text.is_empty() {
            continue;
        }
        out.push_str(&format!("[{start}] {text}\n"));
    }
    out
}

/// Extract verbatim quotes from the timestamped transcript, chunking long
/// transcripts (each chunk's quotes are concatenated — no merge needed).
async fn generate_quotes(
    backend: &dyn LlmBackend,
    chat_opts: &ChatOptions,
    sys: String,
    timestamped: &str,
    g: &GlobalConfig,
) -> Result<String> {
    let budget = char_budget(chat_opts.num_ctx);
    if !g.runtime.chunk || timestamped.chars().count() <= budget {
        let user = prompts::user_quotes_from_transcript(timestamped);
        return call_one(backend, chat_opts, Artifact::Quotes, sys, user).await;
    }
    let chunks = chunk_text(timestamped, budget, g.runtime.chunk_overlap_chars);
    let mut all: Vec<String> = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let user = prompts::user_quotes_from_transcript(chunk);
        match call_one(backend, chat_opts, Artifact::Quotes, sys.clone(), user).await {
            Ok(t) => all.push(t.trim().to_string()),
            Err(e) => crate::ui::warn(&format!(
                "quotes chunk {}/{} failed — {e:#}",
                i + 1,
                chunks.len()
            )),
        }
    }
    if all.is_empty() {
        return Err(anyhow!("all quote chunks failed"));
    }
    Ok(all.join("\n\n"))
}

/// Generate the bullet outline from a transcript, chunking it into overlapping
/// windows when it would exceed the model's context budget. Each chunk is
/// summarised independently (map), then the partial outlines are merged into a
/// single chronological outline (reduce).
async fn generate_bullets(
    backend: &dyn LlmBackend,
    chat_opts: &ChatOptions,
    sys: String,
    transcript: &str,
    g: &GlobalConfig,
) -> Result<String> {
    let budget = char_budget(chat_opts.num_ctx);

    if !g.runtime.chunk || transcript.chars().count() <= budget {
        let user = prompts::user_bullets_from_transcript(transcript);
        return call_one(backend, chat_opts, Artifact::Bullets, sys, user).await;
    }

    let chunks = chunk_text(transcript, budget, g.runtime.chunk_overlap_chars);
    crate::ui::info(&format!(
        "transcript is long ({} chars) — chunking into {} windows (budget ~{} chars)",
        transcript.chars().count(),
        chunks.len(),
        budget,
    ));

    let mut partials: Vec<String> = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let user = prompts::user_bullets_chunk(chunk, i + 1, chunks.len());
        match call_one(backend, chat_opts, Artifact::Bullets, sys.clone(), user).await {
            Ok(text) => partials.push(text),
            Err(e) => crate::ui::warn(&format!(
                "bullets chunk {}/{} failed — {e:#}",
                i + 1,
                chunks.len()
            )),
        }
    }

    if partials.is_empty() {
        return Err(anyhow!("all transcript chunks failed"));
    }
    if partials.len() == 1 {
        return Ok(partials.remove(0));
    }

    // Reduce: merge the per-chunk outlines into one chronological outline.
    let combined = partials.join("\n");
    let merge_sys = prompts::bullets_merge_system();
    let merge_user = prompts::user_bullets_merge(&combined);
    call_one(backend, chat_opts, Artifact::Bullets, merge_sys, merge_user).await
}

/// Approximate character budget for one transcript chunk given a context
/// window. Reserves headroom for the system prompt and the generated output,
/// then converts the remaining tokens to characters (~3 chars/token).
fn char_budget(num_ctx: Option<u32>) -> usize {
    let ctx = num_ctx.unwrap_or(8192) as usize;
    // Reserve ~half the window for the prompt scaffold + generated bullets.
    let reserve = (ctx / 2).max(2048);
    let usable_tokens = ctx.saturating_sub(reserve).max(1024);
    usable_tokens * 3
}

/// Split text into overlapping windows of at most `max_chars`, preferring line
/// boundaries so chunks don't cut mid-sentence. `overlap` characters of the
/// previous window are prepended to the next for cross-boundary context.
fn chunk_text(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let max_chars = max_chars.max(1000);
    let overlap = overlap.min(max_chars / 2);

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.split_inclusive('\n') {
        // A single oversized line is hard-split on character boundaries.
        if line.len() > max_chars {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            let mut rest = line;
            while rest.len() > max_chars {
                let mut split = max_chars;
                while !rest.is_char_boundary(split) && split > 0 {
                    split -= 1;
                }
                chunks.push(rest[..split].to_string());
                rest = &rest[split..];
            }
            current.push_str(rest);
            continue;
        }
        if current.len() + line.len() > max_chars && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            // Seed the next chunk with the tail of the previous one for context.
            if overlap > 0 {
                if let Some(prev) = chunks.last() {
                    let start = prev.len().saturating_sub(overlap);
                    let mut s = start;
                    while !prev.is_char_boundary(s) && s < prev.len() {
                        s += 1;
                    }
                    current.push_str(&prev[s..]);
                }
            }
        }
        current.push_str(line);
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct UsageBackend(AtomicUsize);

    #[async_trait::async_trait]
    impl LlmBackend for UsageBackend {
        fn name(&self) -> &'static str {
            "mock"
        }

        async fn stream_chat(
            &self,
            _messages: Vec<ChatMessage>,
            _opts: ChatOptions,
        ) -> Result<tokio::sync::mpsc::Receiver<Result<llm::StreamEvent>>> {
            let (tx, rx) = tokio::sync::mpsc::channel(3);
            let call = self.0.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                tx.send(Ok(llm::StreamEvent::Text(
                    "The Bell\nA warning echoes.".into(),
                )))
                .await
                .unwrap();
                tx.send(Ok(llm::StreamEvent::Usage(LlmUsage {
                    input_tokens: 7,
                    output_tokens: 11,
                })))
                .await
                .unwrap();
            } else {
                tx.send(Ok(llm::StreamEvent::Text(
                    "- The bell remains unresolved.".into(),
                )))
                .await
                .unwrap();
                tx.send(Ok(llm::StreamEvent::Usage(LlmUsage {
                    input_tokens: 13,
                    output_tokens: 17,
                })))
                .await
                .unwrap();
            }
            Ok(rx)
        }
    }

    #[test]
    fn char_budget_scales_with_ctx() {
        assert!(char_budget(Some(32_768)) > char_budget(Some(8_192)));
        assert!(char_budget(None) > 0);
    }

    #[test]
    fn chunk_text_splits_long_input() {
        let line = "This is a line of the session transcript.\n";
        let text = line.repeat(200);
        let chunks = chunk_text(&text, 1000, 100);
        assert!(chunks.len() > 1, "expected multiple chunks");
        for c in &chunks {
            assert!(
                c.chars().count() <= 1000 + 100,
                "chunk within budget+overlap"
            );
        }
    }

    #[test]
    fn chunk_text_single_when_small() {
        let chunks = chunk_text("short transcript", 1000, 100);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn srt_parses_to_timestamped_lines() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nHello world\n\n\
                   2\n00:00:04,500 --> 00:00:06,000\nSecond line here\n";
        let ts = srt_to_timestamped(srt);
        assert!(ts.contains("[00:00:01] Hello world"), "got: {ts}");
        assert!(ts.contains("[00:00:04] Second line here"), "got: {ts}");
    }

    #[test]
    fn srt_parses_crlf_multiline_cues_without_a_trailing_newline() {
        let srt = "1\r\n00:00:01,000 --> 00:00:03,000\r\nFirst line\r\nsecond line\r\n\r\n2\r\n00:00:04,000 --> 00:00:05,000\r\nLast cue";
        assert_eq!(
            srt_to_timestamped(srt),
            "[00:00:01] First line second line\n[00:00:04] Last cue\n"
        );
    }

    #[test]
    fn ground_quotes_drops_fabricated_and_pins_timestamp() {
        let ts = "[00:00:05] hello there general kenobi\n\
                  [00:01:10] I have the high ground now\n";
        // First quote is real (model gave a bogus timestamp); second is invented.
        let raw = "> *\"I have the high ground now\"*\n> — Obi-Wan — [09:99:99]\n\n\
                   > *\"this line was completely invented\"*\n> — Nobody — [00:00:01]\n";
        let out = ground_quotes(raw, ts);
        assert!(
            out.contains("I have the high ground now"),
            "kept real quote: {out}"
        );
        assert!(
            out.contains("[00:01:10]"),
            "timestamp pinned to transcript: {out}"
        );
        assert!(
            !out.to_lowercase().contains("invented"),
            "dropped fabricated: {out}"
        );
    }

    #[test]
    fn ground_quotes_none_when_all_fabricated() {
        let ts = "[00:00:00] the party enters the tavern\n";
        let raw = "> *\"totally made up nonsense here\"*\n> — Ghost — [00:00:00]\n";
        let out = ground_quotes(raw, ts);
        assert!(out.contains("No verbatim quotes"), "got: {out}");
    }

    #[test]
    fn quote_grounder_uses_first_occurrence_and_skips_short_quotes() {
        let ts = "[00:00:05] the party enters the tavern\n[00:01:10] the party enters the tavern\n";
        let raw = "> *\"the party enters the tavern\"*\n> — GM — [00:00:00]\n\n> *\"too short\"*\n> — GM — [00:00:00]\n";
        let out = ground_quotes(raw, ts);
        assert!(out.contains("[00:00:05]"), "got: {out}");
        assert!(!out.contains("too short"), "got: {out}");
    }

    #[test]
    fn chunking_preserves_utf8_boundaries() {
        let text = format!("{}\n", "naïve café ".repeat(120));
        let chunks = chunk_text(&text, 1000, 100);
        assert!(chunks.len() > 1);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.is_char_boundary(chunk.len())));
        assert!(chunks
            .iter()
            .all(|chunk| chunk.chars().all(|character| character != '\u{FFFD}')));
    }

    #[test]
    fn candidate_names_are_derived() {
        assert_eq!(candidate_name("summary.md", false), "summary.md");
        assert_eq!(candidate_name("summary.md", true), "summary.candidate.md");
        assert_eq!(
            candidate_name("dm-notes.json", true),
            "dm-notes.candidate.json"
        );
        assert_eq!(artifact_file(Artifact::Quotes, true), "quotes.candidate.md");
        assert_eq!(artifact_file(Artifact::Quotes, false), "quotes.md");
    }

    #[test]
    fn estimates_cost_for_known_api_models_only() {
        assert!(estimated_cost("gpt-4o-mini", 1_000_000, 1_000_000).unwrap() > 0.0);
        assert!(estimated_cost("claude-sonnet-4", 1, 1).is_some());
        assert!(estimated_cost("qwen3.5:27b", 1, 1).is_none());
    }

    #[tokio::test]
    async fn campaign_log_calls_record_provider_usage_in_pipeline_report() {
        let backend = UsageBackend(AtomicUsize::new(0));
        let mut campaign = CampaignConfig::default();
        campaign.campaign.name = "Test Campaign".into();
        let preset = crate::presets::load("generic").unwrap();
        let options = ChatOptions::new(
            "mock-model".into(),
            None,
            None,
            Duration::from_secs(1),
            false,
        );
        let (_, _, entry) = generate_session_entry(
            &backend,
            &options,
            &campaign,
            &preset,
            "The group heard a bell.",
            "2026-08-22",
        )
        .await
        .unwrap();
        let (_, threads) =
            generate_threads(&backend, &options, &campaign, &preset, "", &entry.output)
                .await
                .unwrap();

        let mut report = PipelineReport {
            model: "mock-model".into(),
            usages: Vec::new(),
        };
        report.record(entry.artifact, &entry.input, &entry.output, entry.usage);
        report.record(
            threads.artifact,
            &threads.input,
            &threads.output,
            threads.usage,
        );
        assert_eq!(report.usages.len(), 2);
        assert_eq!(report.usages[0].artifact, "campaign-log entry");
        assert_eq!(report.usages[0].input_tokens, 7);
        assert_eq!(report.usages[0].output_tokens, 11);
        assert_eq!(report.usages[1].artifact, "campaign-log threads");
        assert_eq!(report.usages[1].input_tokens, 13);
        assert_eq!(report.usages[1].output_tokens, 17);
    }

    #[tokio::test]
    async fn mocked_notes_pipeline_writes_candidates_and_reuses_resumed_artifacts() {
        let temp = tempfile::tempdir().unwrap();
        let transcript = temp.path().join("session.txt");
        std::fs::write(&transcript, "The party heard the bell beneath the crypt.").unwrap();
        let session = Session::new(&transcript, temp.path().join("notes").as_path()).unwrap();
        let mut global = GlobalConfig::default();
        global.backend.model = Some("mock-model".into());
        global.runtime.index = false;
        let mut campaign = CampaignConfig::default();
        campaign.campaign.name = "Fixture Campaign".into();
        let preset = crate::presets::load("generic").unwrap();
        let backend = UsageBackend(AtomicUsize::new(0));
        let options = PipelineOpts {
            artifacts: vec![Artifact::Bullets],
            resume: false,
            force: true,
            update_log: false,
            model_override: None,
            candidate: false,
        };

        let first =
            run_notes_with_backend(&session, &global, &campaign, &preset, &options, &backend)
                .await
                .unwrap();
        assert_eq!(first.usages.len(), 1);
        assert!(session.notes_dir.join("bullets.md").exists());

        let candidate = PipelineOpts {
            candidate: true,
            ..options.clone()
        };
        run_notes_with_backend(&session, &global, &campaign, &preset, &candidate, &backend)
            .await
            .unwrap();
        assert!(session.notes_dir.join("bullets.candidate.md").exists());

        let calls_before_resume = backend.0.load(Ordering::SeqCst);
        let resumed = PipelineOpts {
            resume: true,
            force: false,
            ..options
        };
        let report =
            run_notes_with_backend(&session, &global, &campaign, &preset, &resumed, &backend)
                .await
                .unwrap();
        assert!(report.usages.is_empty());
        assert_eq!(backend.0.load(Ordering::SeqCst), calls_before_resume);
    }
}
