use anyhow::Result;

use crate::cli::{SystemsAction, SystemsArgs};
use crate::{presets, ui};

pub async fn run(args: SystemsArgs) -> Result<()> {
    match args.action.unwrap_or(SystemsAction::List) {
        SystemsAction::List => {
            ui::header("Bundled game-system presets");
            let mut table = ui::new_table(&["id", "name", "description"]);
            for id in presets::list_ids() {
                let p = presets::load(id)?;
                table.add_row(vec![id.to_string(), p.name, p.description]);
            }
            println!("{table}");
        }
        SystemsAction::Show { name } => {
            let raw = presets::raw_toml(&name)?;
            println!("{raw}");
        }
    }
    Ok(())
}
