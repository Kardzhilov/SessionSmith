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
        num_ctx: g.effective_num_ctx(),
        format: None,
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
            let text = generate_bullets(backend.as_ref(), &chat_opts, sys, &transcript, g).await?;
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

    // --- Structured JSON companion (opt-in) ---
    if g.runtime.structured && opts.artifacts.contains(&Artifact::DmNotes) {
        let out = session.notes_dir.join("dm-notes.json");
        if opts.resume && out.exists() && !opts.force {
            crate::ui::ok(&format!("dm-notes.json: reuse {}", out.display()));
        } else {
            let mut sopts = chat_opts.clone();
            sopts.format = Some(prompts::dm_notes_schema());
            let sys = prompts::dm_notes_structured_system(campaign, preset);
            let user = prompts::user_from_bullets(&bullets);
            match call_one(backend.as_ref(), &sopts, Artifact::DmNotes, sys, user).await {
                Ok(text) => {
                    // Best-effort pretty-print; write raw if not valid JSON.
                    let pretty = serde_json::from_str::<serde_json::Value>(&text)
                        .ok()
                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                        .unwrap_or(text);
                    std::fs::write(&out, &pretty)?;
                    crate::ui::ok(&format!("wrote {}", out.display()));
                }
                Err(e) => crate::ui::warn(&format!("dm-notes.json: failed — {e:#}")),
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

    // --- Local search index (opt-in, on by default) ---
    if g.runtime.index {
        if let Err(e) = crate::index::record_session(campaign, &session.stem, &session.notes_dir) {
            crate::ui::warn(&format!("index: {e:#}"));
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

// ---------------------------------------------------------------------------
// Bullets generation with transcript chunking (map-reduce)
// ---------------------------------------------------------------------------

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
            Err(e) => crate::ui::warn(&format!("bullets chunk {}/{} failed — {e:#}", i + 1, chunks.len())),
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
                while !rest.is_char_boundary(split) && split > 0 { split -= 1; }
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
                    while !prev.is_char_boundary(s) && s < prev.len() { s += 1; }
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
            assert!(c.chars().count() <= 1000 + 100, "chunk within budget+overlap");
        }
    }

    #[test]
    fn chunk_text_single_when_small() {
        let chunks = chunk_text("short transcript", 1000, 100);
        assert_eq!(chunks.len(), 1);
    }
}
