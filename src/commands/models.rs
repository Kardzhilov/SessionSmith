use anyhow::{Context, Result};
use inquire::{Confirm, Select, Text};
use std::path::{Path, PathBuf};

use crate::cli::{ModelsAction, ModelsArgs};
use crate::config::GlobalConfig;
use crate::{hardware, models, ui};

/// Known faster-whisper model names used by whisperx (HuggingFace cache).
const WHISPERX_MODELS: &[&str] = &[
    "tiny",
    "base",
    "small",
    "medium",
    "large-v2",
    "large-v3",
    "large-v3-turbo",
];

/// Recursively sum the size of a directory tree.
fn dir_size_bytes(path: &Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size_bytes(&p);
            } else if let Ok(meta) = p.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Returns Some(bytes) if the whisperx/faster-whisper model exists in the HF cache.
fn whisperx_cache_bytes(model_name: &str) -> Option<u64> {
    let cache_dir = dirs::cache_dir()?;
    let model_dir = cache_dir
        .join("huggingface")
        .join("hub")
        .join(format!("models--Systran--faster-whisper-{model_name}"));
    if model_dir.exists() {
        Some(dir_size_bytes(&model_dir))
    } else {
        None
    }
}

/// Find the Python interpreter in the project venv (falls back to system python3).
fn venv_python() -> PathBuf {
    let venv_wx = Path::new(".venv/bin/whisperx");
    if venv_wx.exists() {
        let abs = std::fs::canonicalize(venv_wx).unwrap_or_else(|_| venv_wx.to_path_buf());
        if let Some(parent) = abs.parent() {
            return parent.join("python3");
        }
    }
    PathBuf::from("python3")
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

pub async fn run(args: ModelsArgs) -> Result<()> {
    let g = GlobalConfig::load_or_default()?;
    match args.action {
        Some(ModelsAction::List) => {
            print_list(&g).await?;
        }

        Some(ModelsAction::Pull { name }) => {
            if let Some(ollama_name) = name.strip_prefix("ollama:") {
                models::ollama_pull(ollama_name)?;
            } else {
                let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
                models::download_whisper(&name, &cache).await?;
            }
        }

        Some(ModelsAction::Recommend) => {
            let hw = hardware::detect();
            let rec = hardware::recommend(&hw);
            ui::panel(
                "Recommendation",
                &[
                    format!("Whisper : {}", rec.whisper_model),
                    format!("LLM     : {}", rec.llm_model),
                    format!("Reason  : {}", rec.reason),
                ],
            );
        }

        // Interactive mode — show list then offer configure/download actions.
        None => {
            print_list(&g).await?;
            interactive_menu().await?;
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Model list display
// ─────────────────────────────────────────────────────────────────────────────

async fn print_list(g: &GlobalConfig) -> Result<()> {
    let hw = hardware::detect();
    let rec = hardware::recommend(&hw);
    let configured_asr = g.asr.model.clone().unwrap_or_else(|| rec.whisper_model.to_string());
    let configured_llm = g.backend.model.clone().unwrap_or_default();

    ui::header("Models");

    // ── whisperx / faster-whisper (HuggingFace cache) ─────────────────
    {
        let mut t = ui::new_table(&["whisperx model", "status", "size"]);
        for &name in WHISPERX_MODELS {
            let marker = if name == configured_asr { " ← configured" } else { "" };
            match whisperx_cache_bytes(name) {
                Some(bytes) => {
                    t.add_row(vec![
                        format!("{name}{marker}"),
                        "✓  installed".into(),
                        format!("{:.1} GB", bytes as f64 / 1e9),
                    ]);
                }
                None => {
                    t.add_row(vec![
                        format!("{name}{marker}"),
                        "—  not downloaded".into(),
                        "".into(),
                    ]);
                }
            }
        }
        println!("ASR — whisperx (faster-whisper HuggingFace cache)");
        println!("{t}");
    }

    // ── ggml / whisper.cpp ────────────────────────────────────────────
    {
        let cache = models::whisper_cache_dir(g.asr.model_dir.as_deref())?;
        let mut t = ui::new_table(&["ggml model", "status", "size"]);
        let mut any_installed = false;
        for model in models::WHISPER_MODELS {
            let path = cache.join(model.filename);
            let (status, size_str) = if path.exists() {
                any_installed = true;
                let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
                ("✓  installed".to_string(), format!("{:.1} GB", bytes as f64 / 1e9))
            } else {
                ("—  not downloaded".to_string(), String::new())
            };
            t.add_row(vec![model.id.to_string(), status, size_str]);
        }
        println!("ASR — ggml / whisper.cpp  (whisper-cli; not needed for whisperx)");
        if any_installed {
            println!("{t}");
        } else {
            println!("  none downloaded\n");
        }
    }

    // ── Ollama LLM ────────────────────────────────────────────────────
    {
        let base = g
            .backend
            .base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".into());
        let url = format!("{}/api/tags", base.trim_end_matches('/'));
        match reqwest::Client::new().get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let v: serde_json::Value = resp.json().await.unwrap_or_default();
                let mut t = ui::new_table(&["ollama model", "size"]);
                if let Some(arr) = v.get("models").and_then(|m| m.as_array()) {
                    for m in arr {
                        let name = m.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                        let size = m.get("size").and_then(|x| x.as_u64()).unwrap_or(0);
                        let marker = if name == configured_llm
                            || name.starts_with(&format!("{configured_llm}:"))
                        {
                            " ← configured"
                        } else {
                            ""
                        };
                        t.add_row(vec![
                            format!("{name}{marker}"),
                            format!("{:.1} GB", size as f64 / 1e9),
                        ]);
                    }
                }
                println!("LLM — Ollama  ({})", base);
                println!("{t}");
            }
            _ => ui::warn("Ollama not reachable — skipping LLM model list"),
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Interactive configure / download menu
// ─────────────────────────────────────────────────────────────────────────────

async fn interactive_menu() -> Result<()> {
    let actions = vec![
        "Change ASR model        — set which whisperx model to use",
        "Change LLM model        — set which Ollama / API model to use",
        "Set LLM backend         — switch between ollama / openai / anthropic",
        "Download a whisperx model",
        "Pull an Ollama model",
        "Done",
    ];
    loop {
        let choice = match Select::new("Configure:", actions.clone()).prompt() {
            Ok(c) => c,
            Err(_) => break,
        };
        let result = match choice {
            c if c.starts_with("Change ASR") => change_asr_model().await,
            c if c.starts_with("Change LLM") => change_llm_model().await,
            c if c.starts_with("Set LLM backend") => set_llm_backend().await,
            c if c.starts_with("Download a whisperx") => download_whisperx_model().await,
            c if c.starts_with("Pull an Ollama") => pull_ollama_model().await,
            _ => break,
        };
        if let Err(e) = result {
            ui::warn(&format!("{e:#}"));
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Configure actions
// ─────────────────────────────────────────────────────────────────────────────

async fn change_asr_model() -> Result<()> {
    let mut g = GlobalConfig::load_or_default()?;
    let current = g.asr.model.clone().unwrap_or_default();

    let labels: Vec<String> = WHISPERX_MODELS
        .iter()
        .map(|&name| {
            let installed = whisperx_cache_bytes(name)
                .map(|b| format!("{:.1} GB  ✓", b as f64 / 1e9))
                .unwrap_or_else(|| "not downloaded".into());
            let marker = if name == current { "  ← current" } else { "" };
            format!("{:<20} {}{}", name, installed, marker)
        })
        .collect();

    let chosen = Select::new("Select ASR model:", labels.clone()).prompt()?;
    let idx = labels.iter().position(|l| l == &chosen).unwrap_or(0);
    let selected = WHISPERX_MODELS[idx];

    g.asr.model = Some(selected.to_string());
    g.save()?;
    ui::ok(&format!("ASR model → {selected}"));

    if whisperx_cache_bytes(selected).is_none() {
        ui::warn("model not cached yet — it will download automatically on first transcription");
        ui::warn("or use 'Download a whisperx model' to prefetch it now");
    }
    Ok(())
}

async fn change_llm_model() -> Result<()> {
    let mut g = GlobalConfig::load_or_default()?;
    let current = g.backend.model.clone().unwrap_or_default();

    let base = g
        .backend
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434".into());
    let url = format!("{}/api/tags", base.trim_end_matches('/'));

    let mut model_names: Vec<String> = Vec::new();
    match reqwest::Client::new().get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let v: serde_json::Value = resp.json().await.unwrap_or_default();
            if let Some(arr) = v.get("models").and_then(|m| m.as_array()) {
                for m in arr {
                    if let Some(name) = m.get("name").and_then(|x| x.as_str()) {
                        model_names.push(name.to_string());
                    }
                }
            }
        }
        _ => ui::warn("Ollama not reachable — showing manual entry only"),
    }

    let mut labels: Vec<String> = model_names
        .iter()
        .map(|name| {
            if name == &current {
                format!("{name}  ← current")
            } else {
                name.clone()
            }
        })
        .collect();
    labels.push("→ Enter a custom model name".into());

    let chosen = Select::new("Select LLM model:", labels.clone()).prompt()?;

    let model_name = if chosen.starts_with("→ Enter") {
        Text::new("Model name:")
            .with_placeholder("qwen2.5:32b")
            .prompt()?
    } else {
        let idx = labels.iter().position(|l| l == &chosen).unwrap_or(0);
        model_names.get(idx).cloned().unwrap_or(chosen)
    };

    if model_name.is_empty() {
        ui::warn("no model name entered — nothing changed");
        return Ok(());
    }

    g.backend.model = Some(model_name.clone());
    g.save()?;
    ui::ok(&format!("LLM model → {model_name}"));
    Ok(())
}

async fn set_llm_backend() -> Result<()> {
    let mut g = GlobalConfig::load_or_default()?;
    let current = g.backend.kind.clone();

    let backends = ["ollama", "openai", "anthropic"];
    let labels: Vec<String> = backends
        .iter()
        .map(|&b| {
            if b == current {
                format!("{b}  ← current")
            } else {
                b.to_string()
            }
        })
        .collect();

    let chosen = Select::new("Select backend:", labels.clone()).prompt()?;
    let idx = labels.iter().position(|l| l == &chosen).unwrap_or(0);
    let backend = backends[idx];

    g.backend.kind = backend.to_string();

    let default_url = match backend {
        "openai" => "https://api.openai.com/v1",
        "anthropic" => "https://api.anthropic.com",
        _ => "http://localhost:11434",
    };
    let url = Text::new("Base URL:")
        .with_default(
            g.backend
                .base_url
                .as_deref()
                .unwrap_or(default_url),
        )
        .prompt()?;
    g.backend.base_url = Some(url);

    if backend != "ollama" {
        let api_key = Text::new("API key (or ${ENV_VAR} to read from environment):")
            .with_placeholder("sk-...")
            .prompt()?;
        if !api_key.is_empty() {
            g.backend.api_key = Some(api_key);
        }
    }

    g.save()?;
    ui::ok(&format!("backend → {backend}"));
    Ok(())
}

async fn download_whisperx_model() -> Result<()> {
    let not_installed: Vec<&&str> = WHISPERX_MODELS
        .iter()
        .filter(|&&name| whisperx_cache_bytes(name).is_none())
        .collect();

    if not_installed.is_empty() {
        ui::ok("All whisperx models are already downloaded.");
        return Ok(());
    }

    let options: Vec<&str> = not_installed.iter().map(|&&n| n).collect();
    let chosen = Select::new("Which model to download?", options).prompt()?;

    let python = venv_python();
    let script = format!(
        "from faster_whisper import WhisperModel; \
         print('Fetching faster-whisper-{chosen} from HuggingFace...'); \
         WhisperModel('{chosen}', device='cpu', compute_type='int8'); \
         print('Done.')"
    );

    ui::info(&format!("Downloading faster-whisper-{chosen} (may take several minutes)..."));

    let status = std::process::Command::new(&python)
        .args(["-c", &script])
        .status()
        .with_context(|| format!("failed to run {}", python.display()))?;

    if status.success() {
        ui::ok(&format!("faster-whisper-{chosen} is ready"));

        // Offer to set as configured model.
        if Confirm::new(&format!("Set {chosen} as your configured ASR model?"))
            .with_default(true)
            .prompt()
            .unwrap_or(false)
        {
            let mut g = GlobalConfig::load_or_default()?;
            g.asr.model = Some(chosen.to_string());
            g.save()?;
            ui::ok(&format!("ASR model → {chosen}"));
        }
    } else {
        ui::warn("Download may have failed — check output above");
    }
    Ok(())
}

async fn pull_ollama_model() -> Result<()> {
    let known: Vec<&str> = models::OLLAMA_KNOWN_SIZES.iter().map(|(n, _)| *n).collect();
    let mut options: Vec<&str> = known;
    let custom = "→ Enter a custom model name";
    options.push(custom);

    let chosen = Select::new("Which Ollama model to pull?", options).prompt()?;

    let model_name = if chosen == custom {
        Text::new("Model name:")
            .with_placeholder("qwen2.5:32b")
            .prompt()?
    } else {
        chosen.to_string()
    };

    if model_name.is_empty() {
        ui::warn("no model name entered — nothing pulled");
        return Ok(());
    }

    models::ollama_pull(&model_name)?;

    // Offer to set as default LLM model.
    if Confirm::new(&format!("Set {model_name} as your default LLM model?"))
        .with_default(true)
        .prompt()
        .unwrap_or(false)
    {
        let mut g = GlobalConfig::load_or_default()?;
        g.backend.model = Some(model_name.clone());
        g.save()?;
        ui::ok(&format!("LLM model → {model_name}"));
    }
    Ok(())
}

