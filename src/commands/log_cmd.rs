use anyhow::Result;

use crate::cli::{LogAction, LogArgs};
use crate::config::GlobalConfig;
use crate::llm::{self, ChatMessage, ChatOptions, Role};
use crate::pipeline;
use crate::{commands, presets, prompts, ui};

pub async fn run(args: LogArgs) -> Result<()> {
    let camp = commands::load_campaign_or_die(&commands::resolve_campaign(None)?)?;
    let notes_dir = camp.notes_dir();
    let path = notes_dir.join("_campaign-log.md");
    match args.action.unwrap_or(LogAction::Show) {
        LogAction::Show => {
            if !path.exists() {
                ui::warn(&format!("no campaign log at {}", path.display()));
                return Ok(());
            }
            print!("{}", std::fs::read_to_string(&path)?);
        }
        LogAction::Rebuild => {
            ui::header("Rebuilding campaign log from session summaries");
            let preset = presets::load(&camp.system.preset)?;
            let g = GlobalConfig::load_or_default()?;

            // Collect summary.md files in notes/<stem>/ ordered by mtime asc.
            let mut sessions: Vec<(std::path::PathBuf, std::time::SystemTime, String)> = Vec::new();
            if notes_dir.exists() {
                for entry in std::fs::read_dir(&notes_dir)? {
                    let entry = entry?;
                    if !entry.file_type()?.is_dir() { continue; }
                    let summary = entry.path().join("summary.md");
                    if !summary.exists() { continue; }
                    let mtime = std::fs::metadata(&summary)?.modified()?;
                    let stem = entry.file_name().to_string_lossy().to_string();
                    sessions.push((summary, mtime, stem));
                }
            }
            sessions.sort_by_key(|s| s.1);
            if sessions.is_empty() {
                anyhow::bail!("no {}/*/summary.md files found to rebuild from", notes_dir.display());
            }

            // Wipe existing log; merge each session one at a time.
            if path.exists() { std::fs::remove_file(&path)?; }
            std::fs::create_dir_all(&notes_dir)?;
            let backend = llm::build(&g)?;
            let chat_opts = ChatOptions {
                model: g.backend.model.clone().ok_or_else(|| anyhow::anyhow!("no model configured"))?,
                temperature: Some(0.3),
                max_tokens: None,
                timeout: std::time::Duration::from_secs(g.runtime.timeout_secs),
                think: g.runtime.think,
            };

            for (summary_path, mtime, stem) in sessions {
                let summary = std::fs::read_to_string(&summary_path)?;
                let existing = if path.exists() { std::fs::read_to_string(&path)? } else { String::new() };
                let date = time::OffsetDateTime::from(mtime).date().to_string();
                let sys = prompts::campaign_log_system(&camp, &preset);
                let user = prompts::user_campaign_log_merge(&existing, &summary, &date, &stem);
                let pb = ui::spinner(&format!("merging {}", stem));
                let merged = llm::collect(backend.as_ref(), vec![
                    ChatMessage { role: Role::System, content: sys },
                    ChatMessage { role: Role::User, content: user },
                ], chat_opts.clone(), Some(&pb)).await?;
                pb.finish_and_clear();
                let tmp = path.with_extension("md.tmp");
                std::fs::write(&tmp, merged)?;
                std::fs::rename(&tmp, &path)?;
                ui::ok(&format!("merged {}", stem));
            }
            // Silence unused import
            let _ = pipeline::parse_artifacts;
        }
    }
    Ok(())
}
