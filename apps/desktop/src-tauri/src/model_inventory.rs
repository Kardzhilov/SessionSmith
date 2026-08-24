use serde::Serialize;
use sessionsmith::{
    asr,
    config::GlobalConfig,
    models::{self, OllamaModel},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub label: String,
    pub family: Option<String>,
    pub engine: String,
    pub state: String,
    pub is_default: bool,
    pub size_bytes: u64,
    pub released: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaService {
    pub reachable: bool,
    pub endpoint: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInventory {
    pub whisper: Vec<ModelEntry>,
    pub asr: Vec<ModelEntry>,
    pub ollama: Vec<ModelEntry>,
    pub ollama_service: OllamaService,
}

pub(crate) async fn inventory() -> Result<ModelInventory, String> {
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    let asr_default = global.asr.model.as_deref();
    let llm_default = global.backend.model.as_deref();
    let whisper_cache = models::whisper_cache_dir(global.asr.model_dir.as_deref())
        .map_err(|error| error.to_string())?;
    let whisper = models::WHISPER_MODELS
        .iter()
        .map(|model| {
            let path = models::whisper_path(model.id, &whisper_cache)
                .map_err(|error| error.to_string())?;
            let installed = path.is_file();
            let size_bytes = std::fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or_else(|_| models::whisper_approx_size(model.id));
            Ok(ModelEntry {
                id: model.id.into(),
                label: format!("Whisper {}", model.id),
                family: None,
                engine: "whisper.cpp".into(),
                state: if installed { "installed" } else { "available" }.into(),
                is_default: asr_default == Some(model.id),
                size_bytes,
                released: model.released.into(),
                detail: "Local GGML speech-to-text model".into(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let asr = asr::ASR_CATALOG
        .iter()
        .filter(|model| model.engine != asr::AsrEngine::WhisperCpp)
        .map(|model| {
            let installed = if model.engine == asr::AsrEngine::TranscribeCpp {
                models::gguf_asr_cache_dir()
                    .ok()
                    .and_then(|cache| models::gguf_asr_path(model.id, &cache).ok())
                    .is_some_and(|path| path.is_file())
            } else {
                asr::is_prepared(model.id)
            };
            ModelEntry {
                id: model.id.into(),
                label: model.display.into(),
                family: Some(model.engine.label().into()),
                engine: model.engine.label().into(),
                state: if installed { "ready" } else { "available" }.into(),
                is_default: asr_default == Some(model.id),
                size_bytes: model.size,
                released: model.released.into(),
                detail: model.note.into(),
            }
        })
        .collect();

    let endpoint = global
        .backend
        .base_url
        .clone()
        .unwrap_or_else(|| "http://localhost:11434".into());
    let (installed_ollama, ollama_service) =
        match models::ollama_local_models_checked(&endpoint).await {
            Ok(installed) => (
                installed.into_iter().collect(),
                OllamaService {
                    reachable: true,
                    endpoint: endpoint.clone(),
                    detail: "Local Ollama server is reachable.".into(),
                },
            ),
            Err(error) => (
                BTreeMap::new(),
                OllamaService {
                    reachable: false,
                    endpoint: endpoint.clone(),
                    detail: error.to_string(),
                },
            ),
        };

    Ok(ModelInventory {
        whisper,
        asr,
        ollama: ollama_entries(&installed_ollama, llm_default),
        ollama_service,
    })
}

fn ollama_entries(
    installed: &BTreeMap<String, u64>,
    default_model: Option<&str>,
) -> Vec<ModelEntry> {
    let mut entries = Vec::new();
    let mut catalog_ids = BTreeSet::new();

    for model in models::OLLAMA_CATALOG {
        entries.extend(catalog_entries(
            model,
            installed,
            default_model,
            &mut catalog_ids,
        ));
    }

    for (id, size_bytes) in installed {
        if catalog_ids.contains(id) {
            continue;
        }
        entries.push(ModelEntry {
            id: id.clone(),
            label: id.clone(),
            family: None,
            engine: "Ollama".into(),
            state: "installed".into(),
            is_default: default_model == Some(id.as_str()),
            size_bytes: *size_bytes,
            released: models::ollama_released(id).into(),
            detail: "Installed outside the curated catalog".into(),
        });
    }

    entries
}

fn catalog_entries(
    model: &OllamaModel,
    installed: &BTreeMap<String, u64>,
    default_model: Option<&str>,
    catalog_ids: &mut BTreeSet<String>,
) -> Vec<ModelEntry> {
    model
        .options
        .iter()
        .map(|option| {
            catalog_ids.insert(option.pull.into());
            let installed_size = installed.get(option.pull).copied();
            ModelEntry {
                id: option.pull.into(),
                label: format!("{} {}", model.display, option.label),
                family: Some(model.display.into()),
                engine: "Ollama".into(),
                state: if installed_size.is_some() {
                    "installed"
                } else {
                    "available"
                }
                .into(),
                is_default: default_model == Some(option.pull),
                size_bytes: installed_size.unwrap_or(option.size),
                released: model.released.into(),
                detail: "Local language model".into(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::ollama_entries;
    use std::collections::BTreeMap;

    #[test]
    fn includes_catalog_defaults_and_unmanaged_installed_models() {
        let entries = ollama_entries(
            &BTreeMap::from([
                ("qwen3.5:9b".into(), 6_500_000_000),
                ("local:latest".into(), 123),
            ]),
            Some("qwen3.5:9b"),
        );

        let default = entries
            .iter()
            .find(|entry| entry.id == "qwen3.5:9b")
            .expect("catalog entry should exist");
        assert_eq!(default.state, "installed");
        assert!(default.is_default);
        assert_eq!(default.size_bytes, 6_500_000_000);
        assert!(entries.iter().any(|entry| entry.id == "local:latest"));
    }
}
