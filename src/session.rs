//! Session building: turn a list of selected audio files into one or more
//! `SessionInput` values, prompting the user to combine and order them.
//!
//! For single-file selections this is a no-op passthrough.
//! For multi-file selections the user can choose:
//!   • current order (newest-first, as shown in the picker)
//!   • oldest-first (reverse)
//!   • manual re-order (type the indices)
//!   • AI sort (transcribe 90 s snippets, ask the LLM to chronologically order)

use anyhow::{anyhow, Result};
use inquire::{Confirm, Select, Text};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::audio::AudioFile;
use crate::config::GlobalConfig;
use crate::ui;

/// One logical recording session, potentially spanning several audio files.
#[derive(Debug, Clone)]
pub struct SessionInput {
    /// Audio files in playback order (oldest → newest within the session).
    pub files: Vec<PathBuf>,
    /// Stem used for all output file naming.
    pub name: String,
}

/// Convert a list of `AudioFile` selections into `SessionInput` values,
/// prompting interactively when more than one file is selected.
pub async fn build_sessions(selected: Vec<AudioFile>, g: &GlobalConfig) -> Result<Vec<SessionInput>> {
    if selected.is_empty() {
        return Err(anyhow!("no files selected"));
    }
    if selected.len() == 1 {
        let f = selected.into_iter().next().unwrap();
        let name = f.stem();
        return Ok(vec![SessionInput { files: vec![f.path], name }]);
    }

    let as_one = Confirm::new(&format!(
        "{} files selected — combine them into one session?",
        selected.len()
    ))
    .with_default(false)
    .with_help_message("No → each file becomes its own session")
    .prompt()?;

    if !as_one {
        return Ok(selected.into_iter()
            .map(|f| SessionInput { files: vec![f.path.clone()], name: f.stem() })
            .collect());
    }

    // Combined session — determine playback order.
    let order_choice = Select::new(
        "File order for the combined session:",
        vec![
            "Current order (newest first, as shown)",
            "Oldest first (reverse current order)",
            "Manual — I'll enter the order",
            "AI sort — analyse audio snippets to determine order",
        ],
    )
    .with_help_message("Files will be concatenated with ffmpeg in this order before transcription")
    .prompt()?;

    let paths: Vec<PathBuf> = selected.iter().map(|f| f.path.clone()).collect();

    let ordered = match order_choice {
        "Current order (newest first, as shown)" => paths,
        "Oldest first (reverse current order)" => {
            let mut p = paths;
            p.reverse();
            p
        }
        "Manual — I'll enter the order" => manual_reorder(paths, &selected)?,
        "AI sort — analyse audio snippets to determine order" => {
            ai_sort_by_content(paths, &selected, g).await?
        }
        _ => paths,
    };

    let default_name = ordered.first()
        .and_then(|p| p.file_stem())
        .map(|s| format!("{}_combined", s.to_string_lossy()))
        .unwrap_or_else(|| "session_combined".to_string());

    let name = Text::new("Session name:")
        .with_default(&default_name)
        .prompt()?;

    Ok(vec![SessionInput { files: ordered, name }])
}

// ---------------------------------------------------------------------------
// Manual reorder
// ---------------------------------------------------------------------------

fn manual_reorder(files: Vec<PathBuf>, meta: &[AudioFile]) -> Result<Vec<PathBuf>> {
    println!();
    for (i, m) in meta.iter().enumerate() {
        ui::info(&format!(
            "[{}] {} — {} / {}",
            i + 1,
            m.path.display(),
            crate::audio::human_age(m.mtime),
            crate::audio::human_duration(m.duration_secs),
        ));
    }
    let hint = (1..=files.len()).map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
    let input = Text::new(&format!(
        "Enter playback order as space-separated numbers 1–{} (e.g. \"{hint}\"):",
        files.len()
    ))
    .with_placeholder(&hint)
    .prompt()?;

    let indices: Vec<usize> = {
        let mut seen = HashSet::new();
        input.split_whitespace()
            .filter_map(|s| s.parse::<usize>().ok())
            .filter(|&i| i >= 1 && i <= files.len())
            .map(|i| i - 1)
            .filter(|i| seen.insert(*i))
            .collect()
    };

    if indices.len() != files.len() {
        anyhow::bail!(
            "expected {} unique indices (1–{}), got {}. Aborting.",
            files.len(), files.len(), indices.len()
        );
    }
    Ok(indices.into_iter().map(|i| files[i].clone()).collect())
}

// ---------------------------------------------------------------------------
// AI content sort
// ---------------------------------------------------------------------------

async fn ai_sort_by_content(
    files: Vec<PathBuf>,
    meta: &[AudioFile],
    g: &GlobalConfig,
) -> Result<Vec<PathBuf>> {
    use std::process::Command;

    // Check that whisper-cli is on PATH (needed for snippet transcription).
    let whisper_ok = g.asr.binary.as_ref().map(|b| b.exists()).unwrap_or(false)
        || Command::new("which").arg("whisper-cli").output()
            .map(|o| o.status.success())
            .unwrap_or(false);

    if !whisper_ok {
        ui::warn("whisper-cli not found — falling back to oldest-first order");
        ui::info("Install whisper-cli and retry to use AI content sorting.");
        let mut f = files;
        f.reverse();
        return Ok(f);
    }

    // Use the fast "base" model for snippet ordering.
    let fast_model = "base";
    let cache = crate::models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
    if let Ok(p) = crate::models::whisper_path(fast_model, &cache) {
        if !p.exists() {
            ui::info(&format!("downloading '{}' model for snippet ordering…", fast_model));
            crate::models::download_whisper(fast_model, &cache).await?;
        }
    }

    let snippet_dir = std::env::temp_dir().join("sessionsmith_snippets");
    std::fs::create_dir_all(&snippet_dir)?;

    let tx_opts = crate::transcribe::TranscribeOpts {
        model: fast_model.to_string(),
        language: "auto".to_string(),
        force: true,
    };

    ui::header(&format!("Transcribing first 90 s of {} files for ordering…", files.len()));

    let mut snippets: Vec<String> = Vec::new();
    for (i, file) in files.iter().enumerate() {
        let wav = snippet_dir.join(format!("snippet_{i}.wav"));

        // Extract 90-second snippet.
        let ok = Command::new("ffmpeg")
            .args(["-y", "-i"])
            .arg(file)
            .args(["-t", "90", "-ar", "16000", "-ac", "1", "-f", "wav"])
            .arg(&wav)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !ok {
            ui::warn(&format!("could not extract snippet from {}", file.display()));
            snippets.push(String::new());
            continue;
        }

        let pb = ui::spinner(&format!("snippet {}/{}", i + 1, files.len()));
        let res = crate::transcribe::transcribe(&wav, &snippet_dir, g, &tx_opts).await;
        pb.finish_and_clear();
        std::fs::remove_file(&wav).ok();

        match res {
            Ok(out) => {
                let text = std::fs::read_to_string(&out.txt).unwrap_or_default();
                let preview: String = text.lines().take(8).collect::<Vec<_>>().join(" ");
                std::fs::remove_file(&out.txt).ok();
                std::fs::remove_file(&out.srt).ok();
                snippets.push(preview);
            }
            Err(e) => {
                ui::warn(&format!("snippet {}: transcription failed ({e}))", i + 1));
                snippets.push(String::new());
            }
        }
    }

    // Show snippet previews.
    println!();
    for (i, s) in snippets.iter().enumerate() {
        let name = meta[i].path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let preview = if s.is_empty() { "(no text)" } else { s.as_str() };
        ui::info(&format!(
            "[{}] {} →  {}",
            i + 1,
            name,
            preview.chars().take(180).collect::<String>()
        ));
    }

    // Ask the LLM to determine the chronological order.
    let backend = crate::llm::build(g)?;
    let model_name = g.backend.model.clone()
        .ok_or_else(|| anyhow!("no LLM model configured for AI sort"))?;

    let snippet_text = snippets.iter().enumerate()
        .map(|(i, t)| {
            let content = if t.is_empty() { "(no content)" } else { t.as_str() };
            format!("File {}: {}", i + 1, content)
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    let messages = vec![
        crate::llm::ChatMessage {
            role: crate::llm::Role::System,
            content: "You determine the chronological order of TTRPG session \
                      audio files by analysing transcription snippets from their \
                      beginnings. Reply with ONLY a space-separated list of \
                      1-based file numbers in chronological order. \
                      Example for 3 files: 3 1 2".to_string(),
        },
        crate::llm::ChatMessage {
            role: crate::llm::Role::User,
            content: format!(
                "{} audio files from the same session. Order them chronologically (first → last):\n\n{}",
                files.len(),
                snippet_text
            ),
        },
    ];

    let pb = ui::spinner("asking LLM for chronological order");
    let response = crate::llm::collect(
        backend.as_ref(),
        messages,
        crate::llm::ChatOptions {
            model: model_name,
            temperature: Some(0.0),
            max_tokens: Some(64),
            timeout: std::time::Duration::from_secs(30),
            think: false,
        },
        Some(&pb),
    )
    .await?;
    pb.finish_and_clear();

    // Parse response: extract 1-based indices in the order the LLM listed them.
    let ordered_indices: Vec<usize> = {
        let mut seen = HashSet::new();
        response
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|tok| tok.parse::<usize>().ok())
            .filter(|&n| n >= 1 && n <= files.len())
            .map(|n| n - 1)
            .filter(|n| seen.insert(*n))
            .collect()
    };

    if ordered_indices.len() == files.len() {
        ui::ok(&format!(
            "AI order: {}",
            ordered_indices.iter().map(|i| (i + 1).to_string()).collect::<Vec<_>>().join(" → ")
        ));
        Ok(ordered_indices.into_iter().map(|i| files[i].clone()).collect())
    } else {
        ui::warn(&format!(
            "LLM response could not be parsed ({:?}) — keeping current order",
            response.trim()
        ));
        Ok(files)
    }
}
