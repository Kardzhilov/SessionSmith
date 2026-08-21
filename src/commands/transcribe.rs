use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::audio;
use crate::cli::TranscribeArgs;
use crate::config::GlobalConfig;
use crate::session::{self, SessionInput};
use crate::transcribe::{self, TranscribeOpts};
use crate::{deps, hardware, ui};

pub async fn run(args: TranscribeArgs) -> Result<()> {
    ui::header("SessionSmith · transcribe");
    deps::ensure_dirs()?;
    let camp_path = crate::commands::resolve_campaign(None)?;
    let campaign  = crate::commands::load_campaign_or_die(&camp_path)?;
    let g = crate::config::effective(&GlobalConfig::load_or_default()?, &campaign);
    let tx_dir    = campaign.transcripts_dir();
    let mut speaker_map = campaign.transcription.speakers.clone();
    for value in &args.speakers {
        let (label, name) = crate::speakers::parse_mapping(value)?;
        speaker_map.insert(label, name);
    }
    if let Some(stem) = &args.remap {
        crate::speakers::apply_to_session(&tx_dir, stem, &speaker_map)?;
        ui::ok(&format!("updated speaker mapping for {stem}"));
        return Ok(());
    }

    let model = args.asr_model
        .or_else(|| g.asr.model.clone())
        .unwrap_or_else(|| hardware::recommend(&hardware::detect()).whisper_model.to_string());
    let tx_opts = TranscribeOpts {
        model,
        language: args.language,
        force: args.force,
        replacements: campaign.transcription.replacements.clone(),
        source_files: Vec::new(),
        initial_prompt: crate::transcribe::vocabulary_prompt(
            &campaign,
            &crate::presets::load(&campaign.system.preset)?,
        ),
        session_date: args.date.clone(),
        diarize: args.diarize || g.asr.diarize,
        vad: args.vad || g.asr.vad,
    };

    let sessions: Vec<SessionInput> = if args.combine {
        if args.files.is_empty() {
            anyhow::bail!("--combine needs at least one audio file");
        }
        vec![SessionInput {
            files: args.files.clone(),
            name: args.name.clone().expect("clap requires --name with --combine"),
        }]
    } else if args.files.is_empty() {
        pick_and_build_sessions(&g, &tx_dir).await?
    } else {
        args.files.iter().map(|f| SessionInput {
            files: vec![f.clone()],
            name: f.file_stem().map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "session".to_string()),
        }).collect()
    };

    let merge_dir = crate::config::audio_dir().join("merged");
    for (i, sess) in sessions.iter().enumerate() {
        ui::step(i + 1, sessions.len(), &sess.name);
        let audio_path = prepare_audio(sess, &merge_dir).await?;
        let mut session_opts = tx_opts.clone();
        session_opts.source_files = sess.files.clone();
        let out = transcribe::transcribe(&audio_path, &tx_dir, &g, &session_opts).await?;
        if !speaker_map.is_empty() {
            let stem = out.txt.file_stem().and_then(|value| value.to_str()).unwrap_or(&sess.name);
            crate::speakers::apply_to_session(&tx_dir, stem, &speaker_map)?;
        }
    }
    Ok(())
}

/// Show the audio picker and session builder interactively.
pub async fn pick_and_build_sessions(g: &GlobalConfig, transcripts_dir: &Path) -> Result<Vec<SessionInput>> {
    let mut files = audio::scan(&crate::config::audio_dir(), transcripts_dir)?;
    if files.is_empty() {
        anyhow::bail!(
            "no audio files found in `audio/` — drop .wav/.mp3/.m4a/.flac/.ogg/.opus files there."
        );
    }
    audio::enrich_durations(&mut files);

    let mut table = ui::new_table(&["#", "file", "age", "duration", "size", "transcribed?"]);
    for (i, f) in files.iter().enumerate() {
        let size_mb = format!("{:.1} MB", f.size_bytes as f64 / 1_048_576.0);
        let mark = if f.already_transcribed { "yes" } else { "—" };
        table.add_row(vec![
            comfy_table::Cell::new(i + 1),
            comfy_table::Cell::new(f.path.display()),
            comfy_table::Cell::new(audio::human_age(f.mtime)),
            comfy_table::Cell::new(audio::human_duration(f.duration_secs)),
            comfy_table::Cell::new(size_mb),
            comfy_table::Cell::new(mark),
        ]);
    }
    println!("{table}");

    let labels: Vec<String> = files.iter().enumerate().map(|(i, f)| {
        format!("{}. {} ({}, {})",
            i + 1, f.path.display(),
            audio::human_age(f.mtime),
            audio::human_duration(f.duration_secs))
    }).collect();

    let chosen = inquire::MultiSelect::new(
        "Select file(s) (space = toggle, enter = confirm):",
        labels.clone(),
    )
    .with_default(&[0])
    .prompt()?;

    let selected: Vec<audio::AudioFile> = chosen.into_iter()
        .filter_map(|c| labels.iter().position(|l| *l == c).map(|i| files[i].clone()))
        .collect();

    session::build_sessions(selected, g).await
}

/// If a session has multiple files, concat them to a durable merged file; otherwise
/// return the single file path directly.
pub async fn prepare_audio(sess: &SessionInput, merge_dir: &Path) -> Result<PathBuf> {
    if sess.files.len() <= 1 {
        Ok(sess.files.first().cloned().unwrap_or_default())
    } else {
        transcribe::concat_audio_files(&sess.files, &sess.name, merge_dir).await
    }
}
