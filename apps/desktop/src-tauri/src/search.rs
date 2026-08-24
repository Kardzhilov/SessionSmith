use crate::{commands, workspace};
use serde::Serialize;
use sessionsmith::{
    candidates,
    config::CampaignConfig,
    index,
    pipeline,
    prompts::ALL_ARTIFACTS,
};
use std::collections::BTreeSet;

const MAX_QUERY_CHARS: usize = 160;
const MAX_RESULTS: usize = 60;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub campaign_id: String,
    pub campaign_name: String,
    pub stem: String,
    pub artifact_id: Option<String>,
    pub artifact_label: String,
    pub candidate: bool,
    pub alternate_name: Option<String>,
    pub snippet: String,
}

pub(crate) fn query(
    campaign_id: Option<String>,
    query: String,
) -> Result<Vec<SearchResult>, String> {
    let query = validate_query(&query)?;
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let campaign_ids = match campaign_id {
        Some(campaign_id) => {
            let campaign_id = campaign_id.trim();
            if campaign_id.is_empty() {
                return Err("Select a campaign or search all campaigns.".into());
            }
            vec![campaign_id.to_string()]
        }
        None => commands::app_bootstrap()?
            .campaigns
            .into_iter()
            .filter(|campaign| campaign.load_error.is_none())
            .map(|campaign| campaign.id)
            .collect(),
    };

    let mut results = Vec::new();
    for campaign_id in campaign_ids {
        results.extend(search_campaign(&campaign_id, &query)?);
        if results.len() >= MAX_RESULTS {
            results.truncate(MAX_RESULTS);
            break;
        }
    }
    Ok(results)
}

fn search_campaign(campaign_id: &str, query: &str) -> Result<Vec<SearchResult>, String> {
    let library = commands::campaign_library(campaign_id.to_string())?;
    let campaign_path = workspace::campaign_config_path(&workspace::workspace_root(), campaign_id)?;
    let campaign = CampaignConfig::load(&campaign_path).map_err(|error| error.to_string())?;
    let known_stems = library
        .sessions
        .iter()
        .map(|session| session.stem.as_str())
        .collect::<BTreeSet<_>>();

    let results = index::search(&campaign, query)
        .map_err(|error| format!("Could not search the campaign index: {error}"))?
        .into_iter()
        .filter(|hit| known_stems.contains(hit.session.as_str()))
        .map(|hit| {
            let (artifact_id, artifact_label, candidate, alternate_name) = artifact_details(&hit.kind);
            SearchResult {
                campaign_id: campaign_id.to_string(),
                campaign_name: library.campaign.name.clone(),
                stem: hit.session,
                artifact_id,
                artifact_label,
                candidate,
                alternate_name,
                snippet: sanitize_snippet(&hit.snippet),
            }
        })
        .collect::<Vec<_>>();
    Ok(results)
}

fn validate_query(value: &str) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err("Search terms must contain 2 to 160 printable characters.".into());
    }
    let query = value.trim();
    if query.is_empty() {
        return Ok(String::new());
    }
    let length = query.chars().count();
    if !(2..=MAX_QUERY_CHARS).contains(&length) {
        return Err("Search terms must contain 2 to 160 printable characters.".into());
    }
    Ok(query.to_string())
}

fn artifact_details(kind: &str) -> (Option<String>, String, bool, Option<String>) {
    ALL_ARTIFACTS
        .iter()
        .find_map(|artifact| {
            if artifact.filename() == kind {
                Some((
                    Some(artifact.id().to_string()),
                    artifact.label().to_string(),
                    false,
                    None,
                ))
            } else if pipeline::artifact_file(*artifact, true) == kind {
                Some((
                    Some(artifact.id().to_string()),
                    format!("{} candidate", artifact.label()),
                    true,
                    None,
                ))
            } else if candidates::is_alternate_filename(artifact.filename(), kind) {
                Some((
                    Some(artifact.id().to_string()),
                    saved_artifact_label(*artifact, kind),
                    false,
                    Some(kind.to_string()),
                ))
            } else {
                None
            }
        })
        .unwrap_or_else(|| (None, "Generated document".into(), false, None))
}

fn saved_artifact_label(artifact: sessionsmith::prompts::Artifact, name: &str) -> String {
    let (stem, extension) = artifact
        .filename()
        .rsplit_once('.')
        .expect("built-in artifact filenames have extensions");
    let prefix = format!("{stem}-alt");
    let numbered_suffix = name
        .strip_prefix(&format!("{prefix}-"))
        .and_then(|suffix| suffix.strip_suffix(&format!(".{extension}")));
    match numbered_suffix {
        Some(number) => format!("{} saved version {number}", artifact.label()),
        None => format!("{} saved version", artifact.label()),
    }
}

fn sanitize_snippet(value: &str) -> String {
    let normalized = value
        .chars()
        .filter_map(|character| {
            if character.is_control() {
                character.is_whitespace().then_some(' ')
            } else {
                Some(character)
            }
        })
        .collect::<String>();
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let snippet = chars.by_ref().take(420).collect::<String>();
    if chars.next().is_some() {
        format!("{snippet}…")
    } else {
        snippet
    }
}

#[cfg(test)]
mod tests {
    use super::{artifact_details, sanitize_snippet, validate_query};

    #[test]
    fn search_queries_are_bounded_and_printable() {
        assert_eq!(validate_query("  raven  ").as_deref(), Ok("raven"));
        assert_eq!(validate_query(" ").as_deref(), Ok(""));
        assert!(validate_query("x").is_err());
        assert!(validate_query("raven\n").is_err());
        assert!(validate_query(&"x".repeat(161)).is_err());
    }

    #[test]
    fn search_result_display_values_do_not_expose_paths_or_controls() {
        assert_eq!(
            artifact_details("summary.md"),
            (Some("summary".into()), "Summary".into(), false, None)
        );
        assert_eq!(
            artifact_details("summary.candidate.md"),
            (Some("summary".into()), "Summary candidate".into(), true, None)
        );
        assert_eq!(
            artifact_details("summary-alt-2.md"),
            (
                Some("summary".into()),
                "Summary saved version 2".into(),
                false,
                Some("summary-alt-2.md".into()),
            )
        );
        assert_eq!(
            artifact_details("unrecognized.json"),
            (None, "Generated document".into(), false, None)
        );
        assert_eq!(sanitize_snippet("A\u{0000} raven\narrives"), "A raven arrives");
    }
}