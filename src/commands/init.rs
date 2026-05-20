use anyhow::Result;
use inquire::{Confirm, Select, Text};
use std::path::PathBuf;

use crate::cli::InitArgs;
use crate::config::{Campaign, CampaignConfig, GlobalConfig, OutputsConfig, Player, PromptOverrides, SystemRef};
use crate::{commands, deps, hardware, models, presets, ui};

pub async fn run(args: InitArgs) -> Result<()> {
    ui::header("SessionSmith · init");

    deps::ensure_dirs()?;
    std::fs::create_dir_all("campaigns")?;

    let preset_id = match args.system {
        Some(s) => s,
        None => {
            let ids = presets::list_ids();
            let pick = Select::new("Pick a game system preset:", ids.clone())
                .with_starting_cursor(ids.iter().position(|x| *x == "generic").unwrap_or(0))
                .prompt()?;
            pick.to_string()
        }
    };
    let preset = presets::load(&preset_id)?;
    ui::ok(&format!("system: {} — {}", preset.name, preset.description));

    let name    = Text::new("Campaign name:").with_default("My Campaign").prompt()?;
    let gm      = Text::new("GM name:").prompt()?;
    let setting = Text::new("Setting (one line):").prompt()?;

    let mut players = Vec::new();
    loop {
        if !Confirm::new(&format!(
            "Add {}player?",
            if players.is_empty() { "a " } else { "another " }
        ))
        .with_default(players.is_empty())
        .prompt()?
        {
            break;
        }
        let player    = Text::new("Player name:").prompt()?;
        let character = Text::new("Character name:").prompt()?;
        let ancestry  = Text::new("Ancestry / species (optional):").prompt().unwrap_or_default();
        let class     = Text::new("Class / role (optional):").prompt().unwrap_or_default();
        players.push(Player { player, character, ancestry, class });
    }

    let slug = commands::slugify(&name);
    let campaign_path = PathBuf::from("campaigns").join(format!("{slug}.toml"));

    if campaign_path.exists() && !args.force {
        ui::warn(&format!(
            "{} already exists. Re-run with --force to overwrite.",
            campaign_path.display()
        ));
        return Ok(());
    }

    // Seed notes from legacy campaign.txt if present.
    let notes_seed = std::fs::read_to_string("campaign.txt").unwrap_or_default();

    let cfg = CampaignConfig {
        campaign: Campaign { name: name.clone(), gm, setting, notes: notes_seed },
        players,
        system: SystemRef { preset: preset_id, overrides: String::new() },
        outputs: OutputsConfig::default(),
        prompts: PromptOverrides::default(),
    };
    cfg.save(&campaign_path)?;
    ui::ok(&format!("wrote {}", campaign_path.display()));

    // Detect hardware and fetch model sizes in parallel.
    let hw  = hardware::detect();
    let rec = hardware::recommend(&hw);

    let ollama_base = "http://localhost:11434".to_string();
    let (whisper_size, llm_size) = tokio::join!(
        models::fetch_hf_size(rec.whisper_model),
        models::fetch_ollama_size(rec.llm_model, &ollama_base),
    );

    let whisper_label = whisper_size
        .map(models::human_bytes)
        .unwrap_or_else(|| "size unknown".to_string());
    let llm_label = llm_size
        .map(models::human_bytes)
        .unwrap_or_else(|| "size unknown".to_string());

    ui::panel("Hardware", &[
        format!("OS: {}, {} cores, {} GB RAM", hw.os, hw.cpu_cores, hw.ram_gb),
        hw.gpu.as_ref()
            .map(|g| format!("GPU: {} {} ({} GB VRAM)", g.vendor, g.name, g.vram_gb))
            .unwrap_or_else(|| "GPU: none detected".to_string()),
    ]);

    ui::panel("Recommended models", &[
        format!("Whisper ASR : {}  ({})", rec.whisper_model, whisper_label),
        format!("LLM         : {}  ({})", rec.llm_model,     llm_label),
        format!("Reason      : {}", rec.reason),
    ]);

    // Persist recommendations into global config.
    let mut g = GlobalConfig::load_or_default()?;
    if g.asr.model.is_none()    { g.asr.model    = Some(rec.whisper_model.to_string()); }
    if g.backend.model.is_none() { g.backend.model = Some(rec.llm_model.to_string()); }
    g.hardware = Some(hw);
    g.save()?;
    ui::ok(&format!("wrote {}", GlobalConfig::path()?.display()));

    // Offer to download whisper model (show size in prompt).
    if Confirm::new(&format!(
        "Download Whisper model '{}' now?  ({})",
        rec.whisper_model, whisper_label
    ))
    .with_default(true)
    .prompt()
    .unwrap_or(false)
    {
        let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
        models::ensure_whisper(rec.whisper_model, &cache).await?;
    }

    // Offer to pull LLM (show size in prompt).
    if g.backend.kind == "ollama" {
        if Confirm::new(&format!(
            "Pull Ollama LLM '{}' now?  ({})",
            rec.llm_model, llm_label
        ))
        .with_default(true)
        .prompt()
        .unwrap_or(false)
        {
            models::ollama_pull(rec.llm_model).ok();
        }
    }

    ui::header(&format!(
        "Done. Drop audio in `audio/` and run `sessionsmith`.\nCampaign: {}",
        campaign_path.display()
    ));
    Ok(())
}
