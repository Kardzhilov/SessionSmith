//! Speaker-label detection and non-destructive mapping for diarized transcripts.

use anyhow::{Context, Result};
use inquire::{Select, Text};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const SPEAKER_PATTERN: &str = r"\bSPEAKER_\d+\b";

/// A representative line attributed to a diarization label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerSample {
    pub label: String,
    pub text: String,
}

/// Discover diarization labels and retain up to three of their longest lines.
pub fn detect_samples(text: &str) -> Vec<SpeakerSample> {
    let pattern = Regex::new(SPEAKER_PATTERN).expect("valid speaker label regex");
    let mut samples: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        let Some(found) = pattern.find(line) else {
            continue;
        };
        let label = found.as_str().to_string();
        let sample = line[found.end()..]
            .trim_start_matches([':', '-', ' '])
            .trim();
        if !sample.is_empty() {
            samples.entry(label).or_default().push(sample.to_string());
        }
    }
    samples
        .into_iter()
        .flat_map(|(label, mut lines)| {
            lines.sort_by_key(|line| std::cmp::Reverse(line.chars().count()));
            lines.into_iter().take(3).map(move |text| SpeakerSample {
                label: label.clone(),
                text,
            })
        })
        .collect()
}

/// Labels in stable lexical order, useful for mapping interfaces.
pub fn labels(text: &str) -> Vec<String> {
    let pattern = Regex::new(SPEAKER_PATTERN).expect("valid speaker label regex");
    let mut found = BTreeSet::new();
    for label in pattern.find_iter(text) {
        found.insert(label.as_str().to_string());
    }
    found.into_iter().collect()
}

/// First cue offset for each diarization label in an SRT transcript.
pub fn preview_offsets(srt: &str) -> BTreeMap<String, f64> {
    let mut offsets = BTreeMap::new();
    for cue in srt.split("\n\n") {
        let mut lines = cue.lines();
        let _ = lines.next();
        let Some(timing) = lines.next() else { continue };
        let Some(start) = timing.split(" --> ").next() else {
            continue;
        };
        let seconds = parse_srt_time(start.trim()).unwrap_or(0.0);
        let text = lines.collect::<Vec<_>>().join(" ");
        for label in labels(&text) {
            offsets.entry(label).or_insert(seconds);
        }
    }
    offsets
}

fn parse_srt_time(value: &str) -> Option<f64> {
    let (hms, millis) = value.split_once(',').unwrap_or((value, "0"));
    let mut parts = hms.split(':').map(str::parse::<f64>);
    let hours = parts.next().and_then(Result::ok)?;
    let minutes = parts.next().and_then(Result::ok)?;
    let seconds = parts.next().and_then(Result::ok)?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds + millis.parse::<f64>().ok()? / 1000.0)
}

/// Replace exact diarization labels in text, leaving unknown labels unchanged.
pub fn apply_map(text: &str, map: &BTreeMap<String, String>) -> String {
    let pattern = Regex::new(SPEAKER_PATTERN).expect("valid speaker label regex");
    pattern
        .replace_all(text, |captures: &regex::Captures<'_>| {
            map.get(captures.get(0).expect("full regex match").as_str())
                .cloned()
                .unwrap_or_else(|| {
                    captures
                        .get(0)
                        .expect("full regex match")
                        .as_str()
                        .to_string()
                })
        })
        .into_owned()
}

/// Preserve the raw TXT transcript once, then apply a mapping to TXT and SRT.
pub fn apply_to_files(txt: &Path, srt: &Path, map: &BTreeMap<String, String>) -> Result<()> {
    if map.is_empty() {
        return Ok(());
    }
    for path in [txt, srt] {
        if path.exists() {
            let raw = path.with_file_name(format!(
                "{}.diarized.{}",
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("transcript"),
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .unwrap_or("txt"),
            ));
            if !raw.exists() {
                std::fs::copy(path, &raw).with_context(|| {
                    format!("saving raw diarized transcript: {}", raw.display())
                })?;
            }
            let text = std::fs::read_to_string(&raw)
                .with_context(|| format!("reading raw diarized transcript: {}", raw.display()))?;
            std::fs::write(path, apply_map(&text, map)).with_context(|| {
                format!("writing speaker-mapped transcript: {}", path.display())
            })?;
        }
    }
    Ok(())
}

/// Apply a map to a transcript session and record it in the metadata sidecar.
pub fn apply_to_session(
    transcripts_dir: &Path,
    stem: &str,
    map: &BTreeMap<String, String>,
) -> Result<()> {
    let txt = transcripts_dir.join(format!("{stem}.txt"));
    let srt = transcripts_dir.join(format!("{stem}.srt"));
    apply_to_files(&txt, &srt, map)?;
    if let Some(mut meta) = crate::meta::load(transcripts_dir, stem) {
        meta.speaker_map = Some(map.clone());
        crate::meta::save(transcripts_dir, stem, &meta)
            .with_context(|| format!("saving speaker map for {stem}"))?;
    }
    Ok(())
}

/// Parse a repeatable CLI mapping in the form `SPEAKER_00=Alice`.
pub fn parse_mapping(value: &str) -> Result<(String, String)> {
    let (label, name) = value
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("speaker mapping must be LABEL=Name, got '{value}'"))?;
    let label = label.trim();
    let name = name.trim();
    if !Regex::new(&format!("^{SPEAKER_PATTERN}$"))?.is_match(label) || name.is_empty() {
        anyhow::bail!("speaker mapping must be SPEAKER_00=Name, got '{value}'");
    }
    Ok((label.to_string(), name.to_string()))
}

/// Prompt for a roster-backed mapping of every diarization label. Existing
/// entries are used as the initial choice, so campaign defaults are confirmed
/// rather than silently trusted.
pub fn confirm_interactively(
    text: &str,
    defaults: &BTreeMap<String, String>,
    roster_names: &[String],
) -> Result<BTreeMap<String, String>> {
    let samples = detect_samples(text);
    let mut map = BTreeMap::new();
    for label in labels(text) {
        crate::ui::header(&format!("Map {label}"));
        for sample in samples.iter().filter(|sample| sample.label == label) {
            crate::ui::info(&format!("  {}", sample.text));
        }

        let mut choices = roster_names.to_vec();
        if let Some(default) = defaults.get(&label) {
            if !choices.contains(default) {
                choices.insert(0, default.clone());
            }
        }
        choices.push("Type a name...".into());
        choices.push("Skip".into());
        let starting_cursor = defaults
            .get(&label)
            .and_then(|name| choices.iter().position(|choice| choice == name))
            .unwrap_or(choices.len().saturating_sub(1));
        let selection = Select::new(&format!("{label} is:"), choices)
            .with_starting_cursor(starting_cursor)
            .prompt()?;
        let name = if selection == "Type a name..." {
            Text::new(&format!("Name for {label}:"))
                .prompt()?
                .trim()
                .to_string()
        } else if selection == "Skip" {
            String::new()
        } else {
            selection
        };
        if !name.is_empty() {
            map.insert(label, name);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_longest_samples_and_replaces_only_labels() {
        let raw = "SPEAKER_00: Hi\nSPEAKER_01: The raven is watching the road\nSPEAKER_00: Longer line here\n";
        let samples = detect_samples(raw);
        assert_eq!(
            samples[0],
            SpeakerSample {
                label: "SPEAKER_00".into(),
                text: "Longer line here".into()
            }
        );
        assert_eq!(labels(raw), vec!["SPEAKER_00", "SPEAKER_01"]);
        let map = BTreeMap::from([("SPEAKER_00".into(), "Alice".into())]);
        assert_eq!(
            apply_map(raw, &map),
            "Alice: Hi\nSPEAKER_01: The raven is watching the road\nAlice: Longer line here\n"
        );
    }

    #[test]
    fn parses_cli_mappings() {
        assert_eq!(
            parse_mapping("SPEAKER_02=Garrick").unwrap(),
            ("SPEAKER_02".into(), "Garrick".into())
        );
        assert!(parse_mapping("speaker=Garrick").is_err());
        assert!(parse_mapping("SPEAKER_02").is_err());
    }

    #[test]
    fn extracts_first_preview_offset_per_label() {
        let srt = "1\n00:00:12,500 --> 00:00:15,000\nSPEAKER_01: First line\n\n2\n00:00:20,000 --> 00:00:22,000\nSPEAKER_01: Later\nSPEAKER_02: Hi\n";
        let offsets = preview_offsets(srt);
        assert_eq!(offsets.get("SPEAKER_01"), Some(&12.5));
        assert_eq!(offsets.get("SPEAKER_02"), Some(&20.0));
    }
}
