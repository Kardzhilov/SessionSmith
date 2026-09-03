use serde::Serialize;
use sessionsmith::{
    asr::{self, AsrEngine, AsrModelSpec},
    config::GlobalConfig,
    deps::{self, DepStatus},
    hardware, models, util,
};

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    pub id: String,
    pub label: String,
    pub state: String,
    pub detail: String,
    pub remedy: Option<String>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HealthGpu {
    pub vendor: String,
    pub name: String,
    #[specta(type = specta_typescript::Number)]
    pub vram_gb: u64,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HealthHardware {
    pub os: String,
    #[specta(type = specta_typescript::Number)]
    pub cpu_cores: usize,
    #[specta(type = specta_typescript::Number)]
    pub ram_gb: u64,
    pub gpu: Option<HealthGpu>,
    pub recommended_asr_model: String,
    pub recommended_llm_model: String,
    pub recommendation_reason: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub checks: Vec<HealthCheck>,
    pub hardware: HealthHardware,
}

pub(crate) async fn report() -> Result<HealthReport, String> {
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    let profile = hardware::detect();
    let recommendation = hardware::recommend(&profile);
    let cache = models::whisper_cache_dir(global.asr.model_dir.as_deref())
        .map_err(|error| error.to_string())?;
    let asr_model = global
        .asr
        .model
        .clone()
        .unwrap_or_else(|| recommendation.whisper_model.to_string());
    let mut statuses = vec![deps::check_ffmpeg(), deps::check_ffprobe()];
    if configured_model_uses_whisper(&asr_model) {
        statuses.push(deps::check_whisper_cli(global.asr.binary.as_deref()));
    }
    statuses.push(configured_asr_model_check(&asr_model, &cache));
    statuses.push(deps::check_uv());
    statuses.push(desktop_audio_player_check());
    statuses.push(deps::check_backend(&global).await);

    Ok(HealthReport {
        checks: statuses.into_iter().map(map_check).collect(),
        hardware: HealthHardware {
            os: profile.os,
            cpu_cores: profile.cpu_cores,
            ram_gb: profile.ram_gb,
            gpu: profile.gpu.map(|gpu| HealthGpu {
                vendor: gpu.vendor,
                name: gpu.name,
                vram_gb: gpu.vram_gb,
            }),
            recommended_asr_model: recommendation.whisper_model.into(),
            recommended_llm_model: recommendation.llm_model.into(),
            recommendation_reason: recommendation.reason,
        },
    })
}

fn configured_model_uses_whisper(model: &str) -> bool {
    asr::find(model).is_none_or(|spec| spec.engine == AsrEngine::WhisperCpp)
}

fn configured_asr_model_check(model: &str, whisper_cache: &std::path::Path) -> DepStatus {
    match asr::find(model).filter(|spec| spec.engine != AsrEngine::WhisperCpp) {
        Some(spec) => advanced_asr_model_check(spec),
        None => deps::check_whisper_model(model, whisper_cache),
    }
}

fn advanced_asr_model_check(spec: &AsrModelSpec) -> DepStatus {
    let prepared = asr::is_prepared(spec.id);
    DepStatus {
        name: format!("asr model: {}", spec.display),
        ok: prepared,
        detail: if prepared {
            format!("{} is prepared for {}", spec.display, spec.engine.label())
        } else {
            format!(
                "{} is not prepared; open Models to prepare it",
                spec.display
            )
        },
    }
}

fn desktop_audio_player_check() -> DepStatus {
    let player = util::find_in_path("ffplay")
        .map(|_| "ffplay")
        .or_else(|| util::find_in_path("mpv").map(|_| "mpv"));
    DepStatus {
        name: "desktop audio playback".into(),
        ok: player.is_some(),
        detail: match player {
            Some(player) => format!("{player} is available for session playback"),
            None => "ffplay or mpv was not found; session playback is unavailable".into(),
        },
    }
}

fn map_check(status: DepStatus) -> HealthCheck {
    let optional = status.name.starts_with("uv") || status.name == "desktop audio playback";
    let state = if status.ok {
        "ok"
    } else if optional {
        "warn"
    } else {
        "fail"
    };

    HealthCheck {
        id: health_id(&status.name),
        label: status.name.clone(),
        state: state.into(),
        detail: status.detail,
        remedy: (!status.ok).then(|| remedy_for(&status.name).into()),
    }
}

fn health_id(label: &str) -> String {
    label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn remedy_for(label: &str) -> &'static str {
    if label.starts_with("ffmpeg")
        || label.starts_with("ffprobe")
        || label == "desktop audio playback"
    {
        "install-ffmpeg"
    } else if label.starts_with("uv") {
        "install-uv"
    } else if label.starts_with("whisper model") || label.starts_with("asr") {
        "open-models"
    } else if label.starts_with("backend") {
        "open-backend-settings"
    } else {
        "open-health-docs"
    }
}

#[cfg(test)]
mod tests {
    use super::{configured_asr_model_check, configured_model_uses_whisper, health_id, map_check};
    use sessionsmith::deps::DepStatus;
    use std::path::Path;

    #[test]
    fn maps_optional_uv_to_a_warning() {
        let check = map_check(DepStatus {
            name: "uv (advanced ASR engines)".into(),
            ok: false,
            detail: "not found".into(),
        });
        assert_eq!(check.state, "warn");
        assert_eq!(check.remedy.as_deref(), Some("install-uv"));
    }

    #[test]
    fn maps_missing_desktop_audio_to_a_warning() {
        let check = map_check(DepStatus {
            name: "desktop audio playback".into(),
            ok: false,
            detail: "not found".into(),
        });
        assert_eq!(check.state, "warn");
        assert_eq!(check.remedy.as_deref(), Some("install-ffmpeg"));
    }

    #[test]
    fn produces_stable_ids_from_dependency_labels() {
        assert_eq!(health_id("backend: ollama"), "backend-ollama");
    }

    #[test]
    fn advanced_asr_models_are_not_checked_as_whisper_models() {
        let model = "moss-transcribe-diarize-0.9b";
        let check = configured_asr_model_check(model, Path::new("."));

        assert!(!configured_model_uses_whisper(model));
        assert_eq!(check.name, "asr model: MOSS Transcribe-Diarize 0.9B");
        assert!(!check.detail.contains("unknown whisper model"));
    }
}
