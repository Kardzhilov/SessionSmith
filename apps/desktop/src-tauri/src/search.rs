use crate::{commands, workspace};
use serde::Serialize;
use sessionsmith::{candidates, config::CampaignConfig, index, pipeline, prompts::ALL_ARTIFACTS};
use std::collections::BTreeSet;
use std::path::Path;

const MAX_QUERY_CHARS: usize = 160;
const MAX_RESULTS: usize = 60;

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub campaign_id: String,
    pub campaign_name: String,
    pub stem: String,
    pub artifact_id: Option<String>,
    pub artifact_label: String,
    pub candidate: bool,
    pub alternate_name: Option<String>,
    pub transcript: bool,
    pub snippet: String,
    #[specta(type = Option<specta_typescript::Number>)]
    pub transcript_line: Option<usize>,
    pub transcript_timestamp: Option<f64>,
}

#[derive(Debug, Serialize, PartialEq, Eq, specta::Type)]
pub struct SearchSource {
    pub id: String,
    pub label: String,
}

pub(crate) fn sources() -> Vec<SearchSource> {
    std::iter::once(SearchSource {
        id: "transcript".into(),
        label: "Transcripts".into(),
    })
    .chain(ALL_ARTIFACTS.iter().map(|artifact| SearchSource {
        id: artifact.id().into(),
        label: artifact.label().into(),
    }))
    .collect()
}

pub(crate) fn query(
    campaign_id: Option<String>,
    query: String,
    source_kinds: Vec<String>,
) -> Result<Vec<SearchResult>, String> {
    let query = validate_query(&query)?;
    let index_kinds = validate_source_kinds(source_kinds)?;
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
        results.extend(search_campaign(&campaign_id, &query, &index_kinds)?);
        if results.len() >= MAX_RESULTS {
            results.truncate(MAX_RESULTS);
            break;
        }
    }
    Ok(results)
}

fn search_campaign(
    campaign_id: &str,
    query: &str,
    index_kinds: &[String],
) -> Result<Vec<SearchResult>, String> {
    let library = commands::campaign_library(campaign_id.to_string())?;
    let campaign_path = workspace::campaign_config_path(&workspace::workspace_root(), campaign_id)?;
    let campaign = CampaignConfig::load(&campaign_path).map_err(|error| error.to_string())?;
    let known_stems = library
        .sessions
        .iter()
        .map(|session| session.stem.as_str())
        .collect::<BTreeSet<_>>();

    let results = index::search_filtered(&campaign, query, index_kinds)
        .map_err(|error| format!("Could not search the campaign index: {error}"))?
        .into_iter()
        .filter(|hit| known_stems.contains(hit.session.as_str()))
        .map(|hit| {
            let (artifact_id, artifact_label, candidate, alternate_name) =
                artifact_details(&hit.kind);
            let transcript = hit.kind == "transcript";
            let (transcript_line, transcript_timestamp) = if transcript {
                transcript_hint(Path::new(&hit.path), &hit.session, query)
            } else {
                (None, None)
            };
            SearchResult {
                campaign_id: campaign_id.to_string(),
                campaign_name: library.campaign.name.clone(),
                stem: hit.session,
                artifact_id,
                artifact_label,
                candidate,
                alternate_name,
                transcript,
                snippet: sanitize_snippet(&hit.snippet),
                transcript_line,
                transcript_timestamp,
            }
        })
        .collect::<Vec<_>>();
    Ok(results)
}

fn transcript_hint(path: &Path, stem: &str, query: &str) -> (Option<usize>, Option<f64>) {
    let Some(directory) = path.parent() else {
        return (None, None);
    };
    let query = query.to_lowercase();
    crate::transcript::read(directory, stem, &Default::default())
        .ok()
        .and_then(|entries| {
            entries.into_iter().enumerate().find_map(|(index, entry)| {
                (entry.text.to_lowercase().contains(&query)
                    || entry
                        .speaker
                        .as_deref()
                        .is_some_and(|speaker| speaker.to_lowercase().contains(&query)))
                .then_some((Some(index + 1), entry.t0))
            })
        })
        .unwrap_or((None, None))
}

fn validate_source_kinds(source_kinds: Vec<String>) -> Result<Vec<String>, String> {
    let mut kinds = BTreeSet::new();
    for source in source_kinds {
        if source == "transcript" {
            kinds.insert(source);
            continue;
        }
        let Some(artifact) = ALL_ARTIFACTS
            .iter()
            .find(|artifact| artifact.id() == source)
        else {
            return Err("Select only known search sources.".into());
        };
        kinds.insert(artifact.filename().to_string());
        kinds.insert(pipeline::artifact_file(*artifact, true));
    }
    Ok(kinds.into_iter().collect())
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
    if kind == "transcript" {
        return (None, "Transcript".into(), false, None);
    }
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
    use super::{
        artifact_details, sanitize_snippet, sources, transcript_hint, validate_query,
        validate_source_kinds,
    };

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
            (
                Some("summary".into()),
                "Summary candidate".into(),
                true,
                None
            )
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
        assert_eq!(
            sanitize_snippet("A\u{0000} raven\narrives"),
            "A raven arrives"
        );
    }

    #[test]
    fn search_sources_are_bounded_to_known_artifacts_and_transcripts() {
        let kinds = validate_source_kinds(vec!["summary".into(), "transcript".into()]).unwrap();
        assert!(kinds.contains(&"summary.md".into()));
        assert!(kinds.contains(&"summary.candidate.md".into()));
        assert!(kinds.contains(&"transcript".into()));
        assert!(validate_source_kinds(vec!["filesystem".into()]).is_err());
        assert_eq!(
            artifact_details("transcript"),
            (None, "Transcript".into(), false, None)
        );
        assert_eq!(
            sources().len(),
            sessionsmith::prompts::ALL_ARTIFACTS.len() + 1
        );
        assert!(sources().iter().any(|source| source.id == "story"));
        assert!(sources().iter().any(|source| source.id == "quotes"));
    }

    #[test]
    fn transcript_hint_returns_first_logical_line_and_timestamp() {
        let directory = std::env::temp_dir().join(format!(
            "sessionsmith-search-hint-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("session.txt");
        std::fs::write(&path, "first\nmoonwell opens\nthird").unwrap();
        assert_eq!(
            transcript_hint(&path, "session", "moonwell"),
            (Some(2), None)
        );

        std::fs::write(
            directory.join("session.srt"),
            "1\n00:00:01,500 --> 00:00:02,000\nfirst\n\n2\n00:00:04,250 --> 00:00:05,000\nmoonwell opens\n",
        )
        .unwrap();
        assert_eq!(
            transcript_hint(&path, "session", "moonwell"),
            (Some(2), Some(4.25))
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
