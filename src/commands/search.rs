//! Cross-session search over indexed artifacts.

use anyhow::Result;

use crate::cli::SearchArgs;
use crate::{commands, index, ui};

pub async fn run(args: SearchArgs) -> Result<()> {
    let query = args.query.join(" ");
    ui::header(&format!("Search · {query}"));

    let mut hits = Vec::new();
    if args.all {
        let paths: Vec<_> = std::fs::read_dir("campaigns")
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("toml"))
            .collect();
        for path in paths {
            let campaign = commands::load_campaign_or_die(&path)?;
            let name = campaign.campaign.name.clone();
            hits.extend(index::search(&campaign, &query)?.into_iter().map(|hit| (name.clone(), hit)));
        }
    } else {
        let campaign = commands::load_campaign_or_die(&commands::resolve_campaign(None)?)?;
        let name = campaign.campaign.name.clone();
        hits.extend(index::search(&campaign, &query)?.into_iter().map(|hit| (name.clone(), hit)));
    }
    if hits.is_empty() {
        ui::warn("no matches (generate notes first, or the index may be empty)");
        return Ok(());
    }

    let mut table = if args.all {
        ui::new_table(&["campaign", "session", "artifact", "match"])
    } else {
        ui::new_table(&["session", "artifact", "match"])
    };
    for (campaign, hit) in &hits {
        let row = if args.all {
            vec![campaign.clone(), hit.session.clone(), hit.kind.clone(), hit.snippet.clone()]
        } else {
            vec![hit.session.clone(), hit.kind.clone(), hit.snippet.clone()]
        };
        table.add_row(row);
    }
    println!("{table}");
    ui::ok(&format!("{} match(es)", hits.len()));
    Ok(())
}
