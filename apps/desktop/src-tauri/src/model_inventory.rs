use serde::{Deserialize, Serialize};
use sessionsmith::{
    asr,
    config::GlobalConfig,
    models::{self, OllamaModel},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub label: String,
    pub family: Option<String>,
    pub cataloged: bool,
    pub engine: String,
    pub state: String,
    pub is_default: bool,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: u64,
    pub released: String,
    pub detail: String,
    #[specta(type = Option<specta_typescript::Number>)]
    pub params: Option<u64>,
    pub languages: Option<Vec<String>>,
    pub language_summary: Option<String>,
    pub license: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OllamaService {
    pub reachable: bool,
    pub endpoint: String,
    pub detail: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelInventory {
    pub whisper: Vec<ModelEntry>,
    pub asr: Vec<ModelEntry>,
    pub ollama: Vec<ModelEntry>,
    pub ollama_service: OllamaService,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ModelDefaultKind {
    Transcription,
    Llm,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefaultRequest {
    pub kind: ModelDefaultKind,
    pub model_id: String,
}

pub(crate) async fn inventory() -> Result<ModelInventory, String> {
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    let asr_default = global.asr.model.as_deref();
    let llm_default = if global.backend.kind.eq_ignore_ascii_case("ollama") {
        global.backend.model.as_deref()
    } else {
        None
    };
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
            Ok(whisper_entry(
                model,
                installed,
                asr_default == Some(model.id),
                size_bytes,
            ))
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
            asr_entry(model, installed, asr_default == Some(model.id))
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

pub(crate) async fn set_default(request: ModelDefaultRequest) -> Result<(), String> {
    let model_id = request.model_id.trim();
    if model_id.is_empty() {
        return Err("Select an installed model.".into());
    }

    let inventory = inventory().await?;
    let installed = |model: &ModelEntry| model.state != "available";
    match request.kind {
        ModelDefaultKind::Transcription => {
            if !inventory
                .whisper
                .iter()
                .chain(inventory.asr.iter())
                .any(|model| model.id == model_id && installed(model))
            {
                return Err("Select an installed transcription model.".into());
            }
            let mut global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
            global.asr.model = Some(model_id.into());
            global.save().map_err(|error| error.to_string())
        }
        ModelDefaultKind::Llm => {
            if !inventory.ollama_service.reachable {
                return Err("Start Ollama before choosing a local language model.".into());
            }
            if !inventory
                .ollama
                .iter()
                .any(|model| model.id == model_id && installed(model))
            {
                return Err("Select an installed local language model.".into());
            }
            let mut global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
            global.backend.kind = "ollama".into();
            global.backend.model = Some(model_id.into());
            global.save().map_err(|error| error.to_string())
        }
    }
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
            cataloged: false,
            engine: "Ollama".into(),
            state: "installed".into(),
            is_default: default_model == Some(id.as_str()),
            size_bytes: *size_bytes,
            released: models::ollama_released(id).into(),
            detail: "Installed outside the curated catalog".into(),
            params: None,
            languages: None,
            language_summary: None,
            license: None,
            note: None,
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
                cataloged: true,
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
                params: Some(option.params),
                languages: language_list(model.languages),
                language_summary: model.langs.map(str::to_string),
                license: model.license.map(str::to_string),
                note: model.note.map(str::to_string),
            }
        })
        .collect()
}

fn whisper_entry(
    model: &models::WhisperModel,
    installed: bool,
    is_default: bool,
    size_bytes: u64,
) -> ModelEntry {
    ModelEntry {
        id: model.id.into(),
        label: format!("Whisper {}", model.id),
        family: None,
        cataloged: true,
        engine: "whisper.cpp".into(),
        state: if installed { "installed" } else { "available" }.into(),
        is_default,
        size_bytes,
        released: model.released.into(),
        detail: "Local GGML speech-to-text model".into(),
        params: Some(model.params),
        languages: language_list(model.languages),
        language_summary: Some(model.langs.into()),
        license: Some(model.license.into()),
        note: Some(model.note.into()),
    }
}

fn asr_entry(model: &asr::AsrModelSpec, installed: bool, is_default: bool) -> ModelEntry {
    ModelEntry {
        id: model.id.into(),
        label: model.display.into(),
        family: Some(model.engine.label().into()),
        cataloged: true,
        engine: model.engine.label().into(),
        state: if installed { "ready" } else { "available" }.into(),
        is_default,
        size_bytes: model.size,
        released: model.released.into(),
        detail: model.note.into(),
        params: model.params,
        languages: language_list(model.languages),
        language_summary: Some(model.langs.into()),
        license: Some(model.license.into()),
        note: Some(model.note.into()),
    }
}

fn language_list(languages: &[&str]) -> Option<Vec<String>> {
    (!languages.is_empty()).then(|| {
        languages
            .iter()
            .map(|language| (*language).into())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::{asr_entry, ollama_entries, whisper_entry};
    use sessionsmith::{asr, models};
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
        assert_eq!(default.params, Some(9_000_000_000));
        assert_eq!(default.languages, None);
        assert_eq!(default.license, None);
        assert!(default.note.is_some());
        assert!(entries.iter().any(|entry| entry.id == "local:latest"));
    }

    #[test]
    fn whisper_entry_carries_catalog_metadata() {
        let model = models::WHISPER_MODELS
            .iter()
            .find(|model| model.id == "large-v3-turbo")
            .expect("Whisper turbo is cataloged");
        let entry = whisper_entry(model, false, false, 1_620_000_000);

        assert_eq!(entry.params, Some(809_000_000));
        assert_eq!(entry.languages, Some(vec!["Multilingual".into()]));
        assert_eq!(entry.language_summary.as_deref(), Some("99 langs"));
        assert_eq!(entry.license.as_deref(), Some("MIT"));
        assert!(entry.note.is_some());
    }

    #[test]
    fn advanced_asr_entry_carries_catalog_metadata() {
        let model = asr::find("granite-speech-4.1-2b").expect("Granite Speech is cataloged");
        let entry = asr_entry(model, false, false);

        assert_eq!(entry.params, Some(2_000_000_000));
        assert_eq!(
            entry.languages,
            Some(vec![
                "English".into(),
                "French".into(),
                "German".into(),
                "Spanish".into(),
                "Portuguese".into(),
                "Japanese".into(),
            ])
        );
        assert_eq!(entry.license.as_deref(), Some("Apache-2.0"));
        assert!(entry.note.is_some());
    }
}
