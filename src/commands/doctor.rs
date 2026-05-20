use anyhow::Result;
use comfy_table::Cell;
use owo_colors::OwoColorize;

use crate::cli::DoctorArgs;
use crate::config::GlobalConfig;
use crate::{deps, hardware, models, ui};

pub async fn run(args: DoctorArgs) -> Result<()> {
    let g = GlobalConfig::load_or_default()?;
    let hw = hardware::detect();
    let rec = hardware::recommend(&hw);

    if args.json {
        let payload = serde_json::json!({
            "hardware": hw,
            "recommendation": {
                "whisper_model": rec.whisper_model,
                "llm_model": rec.llm_model,
                "reason": rec.reason,
            },
            "backend": g.backend.kind,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    ui::header("SessionSmith · doctor");

    ui::panel("Hardware", &[
        format!("OS         : {}", hw.os),
        format!("CPU cores  : {}", hw.cpu_cores),
        format!("RAM        : {} GB", hw.ram_gb),
        match &hw.gpu {
            Some(g) => format!("GPU        : {} {} ({} GB VRAM)", g.vendor, g.name, g.vram_gb),
            None => "GPU        : none detected".into(),
        },
    ]);

    ui::panel("Recommendation", &[
        format!("Whisper    : {}", rec.whisper_model.bold()),
        format!("LLM        : {}", rec.llm_model.bold()),
        format!("Reason     : {}", rec.reason),
    ]);

    // Dependency checks
    let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
    let asr_model = g.asr.model.clone().unwrap_or_else(|| rec.whisper_model.to_string());

    let mut checks = vec![
        deps::check_ffmpeg(),
        deps::check_ffprobe(),
        deps::check_whisper_cli(g.asr.binary.as_deref()),
        deps::check_whisper_model(&asr_model, &cache),
    ];
    checks.push(deps::check_backend(&g).await);

    let mut table = ui::new_table(&["Dependency", "Status", "Detail"]);
    for c in &checks {
        let status = if c.ok { Cell::new("✓ ok").fg(comfy_table::Color::Green) }
                     else { Cell::new("✗ missing").fg(comfy_table::Color::Red) };
        table.add_row(vec![Cell::new(&c.name), status, Cell::new(&c.detail)]);
    }
    println!("{table}");

    // Refresh cached hardware in global config
    let mut g_mut = g;
    g_mut.hardware = Some(hw);
    g_mut.save().ok();

    let any_fail = checks.iter().any(|c| !c.ok);
    if any_fail {
        ui::warn("Some dependencies are missing. See `sessionsmith models pull <name>` and `sessionsmith init`.");
    } else {
        ui::ok("All systems operational.");
    }
    Ok(())
}
