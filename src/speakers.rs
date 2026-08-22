//! Speaker-label detection and non-destructive mapping for diarized transcripts.

use anyhow::{Context, Result};
use inquire::{Select, Text};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

// WhisperX emits `SPEAKER_00`; MOSS emits bracketed labels such as `[S01]`.
const SPEAKER_PATTERN: &str = r"(?:\bSPEAKER_\d+\b|\[S\d+\])";

/// A representative line attributed to a diarization label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerSample {
    pub label: String,
    pub text: String,
}

/// A representative diarized subtitle cue with the interval to play from the
/// source audio.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedSpeakerSample {
    pub label: String,
    pub text: String,
    pub start: f64,
    pub end: f64,
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

/// Discover up to three of each speaker's longest subtitle cues, retaining
/// their source-audio interval for preview playback.
pub fn detect_timed_samples(srt: &str) -> Vec<TimedSpeakerSample> {
    let pattern = Regex::new(SPEAKER_PATTERN).expect("valid speaker label regex");
    let mut samples: BTreeMap<String, Vec<TimedSpeakerSample>> = BTreeMap::new();
    let normalized = srt.replace("\r\n", "\n");
    for cue in normalized.split("\n\n") {
        let mut lines = cue.lines();
        let _ = lines.next();
        let Some(timing) = lines.next() else {
            continue;
        };
        let Some((start, end)) = timing.split_once(" --> ") else {
            continue;
        };
        let Some(start) = parse_srt_time(start.trim()) else {
            continue;
        };
        let Some(end) = end.split_whitespace().next().and_then(parse_srt_time) else {
            continue;
        };
        let text = lines.collect::<Vec<_>>().join(" ");
        let labels: Vec<(usize, usize, String)> = pattern
            .find_iter(&text)
            .map(|found| (found.start(), found.end(), found.as_str().to_string()))
            .collect();
        for (index, (_, label_end, label)) in labels.iter().enumerate() {
            let next_start = labels
                .get(index + 1)
                .map(|(start, _, _)| *start)
                .unwrap_or(text.len());
            let sample = text[*label_end..next_start]
                .trim_start_matches([':', '-', ' '])
                .trim();
            if !sample.is_empty() {
                samples
                    .entry(label.clone())
                    .or_default()
                    .push(TimedSpeakerSample {
                        label: label.clone(),
                        text: sample.to_string(),
                        start,
                        end: end.max(start),
                    });
            }
        }
    }
    samples
        .into_iter()
        .flat_map(|(_, mut cues)| {
            cues.sort_by_key(|cue| std::cmp::Reverse(cue.text.chars().count()));
            cues.into_iter().take(3)
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
    fn recognizes_and_maps_moss_bracketed_labels() {
        let raw = "[S01] Hi\n[S02] The raven is watching the road\n[S01] Longer line here\n";
        let samples = detect_samples(raw);
        assert_eq!(
            samples[0],
            SpeakerSample {
                label: "[S01]".into(),
                text: "Longer line here".into()
            }
        );
        assert_eq!(labels(raw), vec!["[S01]", "[S02]"]);
        let map = BTreeMap::from([("[S01]".into(), "Alice".into())]);
        assert_eq!(
            apply_map(raw, &map),
            "Alice Hi\n[S02] The raven is watching the road\nAlice Longer line here\n"
        );
        assert_eq!(
            parse_mapping("[S01]=Alice").unwrap(),
            ("[S01]".into(), "Alice".into())
        );
        let offsets = preview_offsets(
            "1\n00:00:12,500 --> 00:00:15,000\n[S01] First line\n\n2\n00:00:20,000 --> 00:00:22,000\n[S02] Hi\n",
        );
        assert_eq!(offsets.get("[S01]"), Some(&12.5));
        assert_eq!(offsets.get("[S02]"), Some(&20.0));
    }

    #[test]
    fn retains_the_audio_interval_for_moss_samples() {
        let srt = "1\n00:00:02,000 --> 00:00:03,000\n[S01] Hi\n\n2\n00:00:04,500 --> 00:00:08,250\n[S01] A longer sample to identify this speaker\n\n3\n00:00:10,000 --> 00:00:12,000\n[S02] Another speaker\n";
        let samples = detect_timed_samples(srt);
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].label, "[S01]");
        assert_eq!(samples[0].text, "A longer sample to identify this speaker");
        assert_eq!(samples[0].start, 4.5);
        assert_eq!(samples[0].end, 8.25);
        assert_eq!(samples[2].label, "[S02]");
        assert_eq!(samples[2].start, 10.0);
        assert_eq!(samples[2].end, 12.0);
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
