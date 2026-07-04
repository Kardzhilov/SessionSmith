//! Cross-session search over indexed artifacts.

use anyhow::Result;

use crate::cli::SearchArgs;
use crate::{commands, index, ui};

pub async fn run(args: SearchArgs) -> Result<()> {
    let camp = commands::load_campaign_or_die(&commands::resolve_campaign(None)?)?;
    let query = args.query.join(" ");
    ui::header(&format!("Search · {query}"));

    let hits = index::search(&camp, &query)?;
    if hits.is_empty() {
        ui::warn("no matches (generate notes first, or the index may be empty)");
        return Ok(());
    }

    let mut table = ui::new_table(&["session", "artifact", "match"]);
    for h in &hits {
        table.add_row(vec![h.session.clone(), h.kind.clone(), h.snippet.clone()]);
    }
    println!("{table}");
    ui::ok(&format!("{} match(es)", hits.len()));
    Ok(())
}
