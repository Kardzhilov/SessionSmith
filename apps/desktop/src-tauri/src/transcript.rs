use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
};

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptEntry {
    pub text: String,
    pub t0: Option<f64>,
    pub t1: Option<f64>,
    pub speaker: Option<String>,
}

pub fn read(
    transcripts_dir: &Path,
    stem: &str,
    speaker_map: &BTreeMap<String, String>,
) -> Result<Vec<TranscriptEntry>, String> {
    let json = transcripts_dir.join(format!("{stem}.json"));
    if let Some(entries) = fs::read_to_string(&json)
        .ok()
        .and_then(|contents| parse_json(&contents, speaker_map))
        .filter(|entries| !entries.is_empty())
    {
        return Ok(entries);
    }

    let tsv = transcripts_dir.join(format!("{stem}.tsv"));
    if let Some(entries) = fs::read_to_string(&tsv)
        .ok()
        .map(|contents| parse_tsv(&contents, speaker_map))
        .filter(|entries| !entries.is_empty())
    {
        return Ok(entries);
    }

    for extension in ["srt", "vtt"] {
        let captions = transcripts_dir.join(format!("{stem}.{extension}"));
        if let Some(entries) = fs::read_to_string(&captions)
            .ok()
            .map(|contents| parse_captions(&contents, speaker_map))
            .filter(|entries| !entries.is_empty())
        {
            return Ok(entries);
        }
    }

    let text = transcripts_dir.join(format!("{stem}.txt"));
    let contents = fs::read_to_string(&text)
        .map_err(|error| format!("Could not read transcript for session '{stem}': {error}"))?;
    Ok(parse_text(&contents, speaker_map))
}

pub fn filter(entries: Vec<TranscriptEntry>, query: Option<&str>) -> Vec<TranscriptEntry> {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return entries;
    };
    let query = query.to_lowercase();
    entries
        .into_iter()
        .filter(|entry| {
            entry.text.to_lowercase().contains(&query)
                || entry
                    .speaker
                    .as_deref()
                    .is_some_and(|speaker| speaker.to_lowercase().contains(&query))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonTranscript {
    WithSegments { segments: Vec<JsonSegment> },
    Segments(Vec<JsonSegment>),
}

#[derive(Debug, Deserialize)]
struct JsonSegment {
    #[serde(default)]
    start: Option<f64>,
    #[serde(default)]
    end: Option<f64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    speaker: Option<String>,
    #[serde(default)]
    words: Vec<JsonWord>,
}

#[derive(Debug, Deserialize)]
struct JsonWord {
    #[serde(default)]
    speaker: Option<String>,
}

fn parse_json(contents: &str, speaker_map: &BTreeMap<String, String>) -> Option<Vec<TranscriptEntry>> {
    let document = serde_json::from_str::<JsonTranscript>(contents).ok()?;
    let segments = match document {
        JsonTranscript::WithSegments { segments } | JsonTranscript::Segments(segments) => segments,
    };
    Some(
        segments
            .into_iter()
            .filter_map(|segment| {
                let speaker = segment
                    .speaker
                    .or_else(|| segment.words.into_iter().find_map(|word| word.speaker));
                entry(
                    segment.text.as_deref().unwrap_or_default(),
                    segment.start,
                    segment.end,
                    speaker.as_deref(),
                    speaker_map,
                )
            })
            .collect(),
    )
}

fn parse_tsv(contents: &str, speaker_map: &BTreeMap<String, String>) -> Vec<TranscriptEntry> {
    let mut rows = contents.lines();
    let Some(header) = rows.next() else {
        return Vec::new();
    };
    let columns = header
        .split('\t')
        .map(|column| column.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let Some(start_column) = columns.iter().position(|column| column == "start") else {
        return Vec::new();
    };
    let Some(end_column) = columns.iter().position(|column| column == "end") else {
        return Vec::new();
    };
    let Some(text_column) = columns.iter().position(|column| column == "text") else {
        return Vec::new();
    };
    let speaker_column = columns.iter().position(|column| column == "speaker");

    rows.filter_map(|row| {
        let fields = row.split('\t').collect::<Vec<_>>();
        let start = fields
            .get(start_column)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .and_then(valid_timestamp)
            .map(|milliseconds| milliseconds / 1_000.0);
        let end = fields
            .get(end_column)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .and_then(valid_timestamp)
            .map(|milliseconds| milliseconds / 1_000.0);
        let text = fields.get(text_column).copied().unwrap_or_default();
        let speaker = speaker_column
            .and_then(|index| fields.get(index))
            .copied();
        entry(text, start, end, speaker, speaker_map)
    })
    .collect()
}

fn parse_captions(contents: &str, speaker_map: &BTreeMap<String, String>) -> Vec<TranscriptEntry> {
    let normalized = contents.replace("\r\n", "\n");
    normalized
        .split("\n\n")
        .filter_map(|cue| {
            let lines = cue.lines().collect::<Vec<_>>();
            let timing_index = lines.iter().position(|line| line.contains("-->"))?;
            let (start, end) = lines[timing_index].split_once("-->")?;
            let start = parse_caption_timestamp(start.trim())?;
            let end = end
                .split_whitespace()
                .next()
                .and_then(parse_caption_timestamp)?;
            let text = lines[timing_index + 1..].join(" ");
            entry(&text, Some(start), Some(end), None, speaker_map)
        })
        .collect()
}

fn parse_text(contents: &str, speaker_map: &BTreeMap<String, String>) -> Vec<TranscriptEntry> {
    contents
        .lines()
        .filter_map(|line| entry(line, None, None, None, speaker_map))
        .collect()
}

fn entry(
    text: &str,
    t0: Option<f64>,
    t1: Option<f64>,
    explicit_speaker: Option<&str>,
    speaker_map: &BTreeMap<String, String>,
) -> Option<TranscriptEntry> {
    let (embedded_speaker, text) = split_speaker_prefix(text);
    let text = normalize_text(text);
    if text.is_empty() {
        return None;
    }
    let t0 = t0.and_then(valid_timestamp);
    let t1 = t1
        .and_then(valid_timestamp)
        .filter(|end| t0.is_none_or(|start| *end >= start));
    let speaker = explicit_speaker
        .or(embedded_speaker.as_deref())
        .and_then(|speaker| display_speaker(speaker, speaker_map));
    Some(TranscriptEntry {
        text,
        t0,
        t1,
        speaker,
    })
}

fn split_speaker_prefix(text: &str) -> (Option<String>, &str) {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('[') {
        if let Some((label, after)) = rest.split_once(']') {
            let label = format!("[{label}]");
            if is_speaker_label(&label) {
                return (Some(label), after.trim_start_matches([':', '-', ' ']));
            }
        }
    }
    if let Some(rest) = text.strip_prefix("SPEAKER_") {
        let digit_count = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digit_count > 0 {
            let label = format!("SPEAKER_{}", &rest[..digit_count]);
            let after = &rest[digit_count..];
            if after.is_empty() || after.starts_with([':', '-', ' ']) {
                return (Some(label), after.trim_start_matches([':', '-', ' ']));
            }
        }
    }
    (None, text)
}

fn is_speaker_label(value: &str) -> bool {
    value
        .strip_prefix("[S")
        .and_then(|value| value.strip_suffix(']'))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
}

fn display_speaker(value: &str, speaker_map: &BTreeMap<String, String>) -> Option<String> {
    let value = normalize_text(value);
    if value.is_empty() {
        return None;
    }
    let mapped = speaker_map.get(&value).map(String::as_str).unwrap_or(&value);
    let mapped = normalize_text(mapped);
    (!mapped.is_empty()).then_some(mapped)
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .filter_map(|character| {
            if character.is_control() {
                character.is_whitespace().then_some(' ')
            } else {
                Some(character)
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn valid_timestamp(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn parse_caption_timestamp(value: &str) -> Option<f64> {
    let normalized = value.replace(',', ".");
    let (whole, fractional) = normalized.split_once('.').unwrap_or((&normalized, "0"));
    let parts = whole
        .split(':')
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0.0, *minutes, *seconds),
        [hours, minutes, seconds] => (*hours, *minutes, *seconds),
        _ => return None,
    };
    let fractional = fractional.parse::<f64>().ok()? / 10_f64.powi(fractional.len() as i32);
    let timestamp = hours * 3_600.0 + minutes * 60.0 + seconds + fractional;
    (minutes < 60.0 && seconds < 60.0)
        .then_some(timestamp)
        .and_then(valid_timestamp)
}

#[cfg(test)]
mod tests {
    use super::{filter, parse_captions, parse_json, parse_text, parse_tsv};
    use std::collections::BTreeMap;

    #[test]
    fn json_segments_prefer_explicit_speakers_and_apply_mappings() {
        let map = BTreeMap::from([("SPEAKER_00".into(), "Alice".into())]);
        let entries = parse_json(
            r#"{"segments":[{"start":1.25,"end":2.5,"text":" Hello there ","speaker":"SPEAKER_00"},{"start":3.0,"end":4.0,"text":"[S01]: Welcome"}] }"#,
            &map,
        )
        .unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "Hello there");
        assert_eq!(entries[0].speaker.as_deref(), Some("Alice"));
        assert_eq!(entries[0].t0, Some(1.25));
        assert_eq!(entries[1].text, "Welcome");
        assert_eq!(entries[1].speaker.as_deref(), Some("[S01]"));
    }

    #[test]
    fn tsv_milliseconds_become_seconds_and_caption_formats_preserve_timing() {
        let entries = parse_tsv("start\tend\ttext\n125\t2500\t[S01]: Hello", &BTreeMap::new());
        assert_eq!(entries[0].t0, Some(0.125));
        assert_eq!(entries[0].t1, Some(2.5));
        assert_eq!(entries[0].speaker.as_deref(), Some("[S01]"));

        let captions = parse_captions(
            "WEBVTT\n\n00:01.250 --> 00:02.500\nSPEAKER_00: Welcome",
            &BTreeMap::new(),
        );
        assert_eq!(captions[0].t0, Some(1.25));
        assert_eq!(captions[0].t1, Some(2.5));
        assert_eq!(captions[0].text, "Welcome");
    }

    #[test]
    fn text_fallback_normalizes_lines_and_retains_unlabeled_text() {
        let entries = parse_text("Plain line\n\n[S02] - Another line", &BTreeMap::new());

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "Plain line");
        assert_eq!(entries[0].speaker, None);
        assert_eq!(entries[1].text, "Another line");
        assert_eq!(entries[1].speaker.as_deref(), Some("[S02]"));
    }

    #[test]
    fn filter_matches_normalized_text_or_display_speaker() {
        let entries = parse_text("[S01]: Hello table\n[S02]: Goodbye", &BTreeMap::new());

        assert_eq!(filter(entries.clone(), Some("table")).len(), 1);
        assert_eq!(filter(entries.clone(), Some("s02"))[0].text, "Goodbye");
        assert_eq!(filter(entries, Some("missing")).len(), 0);
    }
}