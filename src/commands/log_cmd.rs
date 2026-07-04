use anyhow::Result;

use crate::cli::{LogAction, LogArgs};
use crate::config::GlobalConfig;
use crate::llm::ChatOptions;
use crate::pipeline;
use crate::{commands, presets, ui};

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
            let chat_opts = ChatOptions {
                model: g.backend.model.clone().ok_or_else(|| anyhow::anyhow!("no model configured"))?,
                temperature: Some(0.3),
                max_tokens: None,
                timeout: std::time::Duration::from_secs(g.runtime.timeout_secs),
                think: g.runtime.think,
                num_ctx: g.effective_num_ctx(),
                format: None,
            };
            pipeline::rebuild_campaign_log(&g, &camp, &preset, &chat_opts).await?;
        }
    }
    Ok(())
}
