//! Interactive start screen shown when SessionSmith is invoked with no subcommand.

use anyhow::Result;
use inquire::Select;

use crate::cli::{DoctorArgs, LogAction, LogArgs, ModelsArgs, NotesArgs, RunArgs, TranscribeArgs};
use crate::config::GlobalConfig;
use crate::hardware;
use crate::presets;
use crate::{commands, deps, ui};

/// Possible actions returned from the home menu.
enum MenuChoice {
    RunPipeline,
    TranscribeOnly,
    GenerateNotes,
    CampaignLog,
    Models,
    SystemCheck,
    Quit,
}

pub async fn run() -> Result<()> {
    deps::ensure_dirs()?;

    // Build campaign info for the panel (soft-fail: show dashes if missing).
    let camp_path_res = commands::resolve_campaign(None);
    // Pin the chosen campaign in the env so every sub-command dispatched from
    // this menu (run, transcribe, notes, log…) uses the same campaign without
    // asking again.
    if let Ok(ref path) = camp_path_res {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        std::env::set_var("SESSIONSMITH_CAMPAIGN", abs);
    }
    let (camp_name, system_name, backend_str, asr_model_str) = match &camp_path_res {
        Ok(path) => match commands::load_campaign_or_die(path) {
            Ok(c) => {
                let preset_name = presets::load(&c.system.preset)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|_| c.system.preset.clone());
                let g = GlobalConfig::load_or_default().unwrap_or_default();
                let asr = g
                    .asr
                    .model
                    .clone()
                    .unwrap_or_else(|| hardware::recommend(&hardware::detect()).whisper_model.to_string());
                let model = g.backend.model.clone().unwrap_or_else(|| "(not set)".into());
                (
                    c.campaign.name.clone(),
                    preset_name,
                    format!("{} / {}", g.backend.kind, model),
                    asr,
                )
            }
            Err(_) => ("—".into(), "—".into(), "—".into(), "—".into()),
        },
        Err(_) => ("—".into(), "—".into(), "—".into(), "—".into()),
    };

    ui::header("SessionSmith");
    ui::panel(
        "Campaign",
        &[
            format!("Name     : {camp_name}"),
            format!("System   : {system_name}"),
            format!("Backend  : {backend_str}"),
            format!("ASR      : {asr_model_str}"),
        ],
    );

    let options: Vec<(&str, MenuChoice)> = vec![
        (
            "Run pipeline         — pick audio, transcribe & generate notes",
            MenuChoice::RunPipeline,
        ),
        (
            "Transcribe only      — audio → transcript",
            MenuChoice::TranscribeOnly,
        ),
        (
            "Generate notes       — transcript → notes",
            MenuChoice::GenerateNotes,
        ),
        (
            "Campaign log         — view the rolling session log",
            MenuChoice::CampaignLog,
        ),
        (
            "Models & config      — view installed models, configure backends",
            MenuChoice::Models,
        ),
        (
            "System check         — verify dependencies and hardware",
            MenuChoice::SystemCheck,
        ),
        ("Quit", MenuChoice::Quit),
    ];

    let labels: Vec<&str> = options.iter().map(|(label, _)| *label).collect();

    let choice_label = match Select::new("What would you like to do?", labels).prompt() {
        Ok(l) => l,
        // User hit Esc or Ctrl-C on the menu — treat as Quit.
        Err(_) => return Ok(()),
    };

    let choice = options
        .into_iter()
        .find(|(l, _)| *l == choice_label)
        .map(|(_, c)| c)
        .unwrap_or(MenuChoice::Quit);

    match choice {
        MenuChoice::RunPipeline => commands::run::run(RunArgs::default()).await,

        MenuChoice::TranscribeOnly => {
            commands::transcribe::run(TranscribeArgs {
                files: vec![],
                asr_model: None,
                force: false,
                language: "auto".into(),
            })
            .await
        }

        MenuChoice::GenerateNotes => {
            commands::notes::run(NotesArgs {
                transcript: None,
                backend: None,
                model: None,
                artifacts: None,
                resume: false,
                force: false,
                no_log: false,
            })
            .await
        }

        MenuChoice::CampaignLog => {
            commands::log_cmd::run(LogArgs {
                action: Some(LogAction::Show),
            })
            .await
        }

        MenuChoice::Models => commands::models::run(ModelsArgs { action: None }).await,

        MenuChoice::SystemCheck => commands::doctor::run(DoctorArgs { json: false }).await,

        MenuChoice::Quit => Ok(()),
    }
}
