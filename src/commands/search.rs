//! Cross-session search over indexed artifacts.

use anyhow::Result;
use owo_colors::OwoColorize;

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
            .filter(|path| {
                path.extension().and_then(|extension| extension.to_str()) == Some("toml")
            })
            .collect();
        for path in paths {
            let campaign = commands::load_campaign_or_die(&path)?;
            let name = campaign.campaign.name.clone();
            hits.extend(
                index::search(&campaign, &query)?
                    .into_iter()
                    .map(|hit| (name.clone(), hit)),
            );
        }
    } else {
        let campaign = commands::load_campaign_or_die(&commands::resolve_campaign(None)?)?;
        let name = campaign.campaign.name.clone();
        hits.extend(
            index::search(&campaign, &query)?
                .into_iter()
                .map(|hit| (name.clone(), hit)),
        );
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
            vec![
                campaign.clone(),
                hit.session.clone(),
                format_kind(&hit.kind),
                format_snippet(&hit.snippet, &query),
            ]
        } else {
            vec![
                hit.session.clone(),
                format_kind(&hit.kind),
                format_snippet(&hit.snippet, &query),
            ]
        };
        table.add_row(row);
    }
    println!("{table}");
    ui::ok(&format!("{} match(es)", hits.len()));
    Ok(())
}

fn format_kind(kind: &str) -> String {
    if !ui::color_enabled() {
        return kind.to_string();
    }
    match kind {
        value if value.contains("summary") => format!("{}", kind.cyan()),
        value if value.contains("quote") => format!("{}", kind.magenta()),
        value if value.contains("dm") => format!("{}", kind.green()),
        _ => format!("{}", kind.bright_black()),
    }
}

fn format_snippet(snippet: &str, query: &str) -> String {
    let marked = index::highlight_markers(snippet, query);
    if !ui::color_enabled() {
        return marked;
    }
    let mut output = String::new();
    let mut rest = marked.as_str();
    while let Some(start) = rest.find('«') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + '«'.len_utf8()..];
        let Some(end) = after_start.find('»') else {
            output.push_str(&rest[start..]);
            return output;
        };
        output.push_str(&format!(
            "{}",
            after_start[..end].to_string().yellow().bold()
        ));
        rest = &after_start[end + '»'.len_utf8()..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_search_snippets_keep_match_markers() {
        ui::set_color_enabled(false);
        assert_eq!(format_snippet("a «match»", "match"), "a «match»");
        ui::set_color_enabled(true);
    }
}
