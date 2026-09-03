use anyhow::Result;

use crate::cli::{LogAction, LogArgs};
use crate::config::GlobalConfig;
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
            crate::campaign_log::rebuild_for(&camp, &g, &preset).await?;
        }
    }
    Ok(())
}
