use serde::{Deserialize, Serialize};
use sessionsmith::config::{
    CampaignConfig, CampaignEditableSettings, CampaignTextReplacement, GlobalConfig,
    Player as ConfigPlayer,
};
use std::path::{Path, PathBuf};

use crate::commands;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignSettings {
    pub revision: String,
    pub campaign: CampaignIdentity,
    pub players: Vec<Player>,
    pub system: SystemSettings,
    pub backend: Vec<EffectiveSetting>,
    pub transcription: TranscriptionSettings,
    pub outputs: Vec<String>,
    pub prompt_overrides: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignIdentity {
    pub id: String,
    pub name: String,
    pub gm: String,
    pub setting: String,
    pub notes: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub player: String,
    pub character: String,
    pub ancestry: String,
    pub class: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignSettingsWriteRequest {
    pub campaign_id: String,
    pub players: Vec<EditablePlayer>,
    pub vocabulary: Vec<String>,
    pub replacements: Vec<EditableReplacement>,
    pub expected_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditablePlayer {
    pub player: String,
    pub character: String,
    pub ancestry: String,
    pub class: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableReplacement {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSettings {
    pub preset_id: String,
    pub overrides: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveSetting {
    pub id: String,
    pub label: String,
    pub value: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSettings {
    pub asr: Vec<EffectiveSetting>,
    pub vocabulary: Vec<String>,
    pub replacements: Vec<Replacement>,
    pub speakers: Vec<SpeakerMapping>,
    pub vocab_prompt: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Replacement {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerMapping {
    pub label: String,
    pub name: String,
}

pub(crate) fn campaign_settings(campaign_id: String) -> Result<CampaignSettings, String> {
    let library = commands::campaign_library(campaign_id)?;
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    let config_path = campaign_config_path(&library.campaign.id)?;
    let (campaign, revision) = CampaignConfig::load_with_revision(&config_path)
        .map_err(|error| error.to_string())?;

    Ok(build_campaign_settings(
        library.campaign.id,
        &global,
        &campaign,
        revision,
    ))
}

pub(crate) fn write_campaign_settings(
    request: CampaignSettingsWriteRequest,
) -> Result<CampaignSettings, String> {
    let campaign_id = request.campaign_id.trim();
    if campaign_id.is_empty() {
        return Err("A campaign is required to save settings.".into());
    }
    let library = commands::campaign_library(campaign_id.into())?;
    let config_path = campaign_config_path(&library.campaign.id)?;
    let result = sessionsmith::config::write_campaign_editable_settings(
        &config_path,
        &request.expected_revision,
        CampaignEditableSettings {
            players: request
                .players
                .into_iter()
                .map(|player| ConfigPlayer {
                    player: player.player,
                    character: player.character,
                    ancestry: player.ancestry,
                    class: player.class,
                })
                .collect(),
            vocabulary: request.vocabulary,
            replacements: request
                .replacements
                .into_iter()
                .map(|replacement| CampaignTextReplacement {
                    from: replacement.from,
                    to: replacement.to,
                })
                .collect(),
        },
    )
    .map_err(|error| error.to_string())?;
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;

    Ok(build_campaign_settings(
        library.campaign.id,
        &global,
        &result.config,
        result.revision,
    ))
}

fn campaign_config_path(campaign_id: &str) -> Result<PathBuf, String> {
    if !is_simple_campaign_id(campaign_id) {
        return Err("Campaign identifiers must be simple campaign file names.".into());
    }
    let root = workspace_root();
    let campaign_path = root.join("campaigns").join(format!("{campaign_id}.toml"));
    if campaign_path.is_file() {
        return Ok(campaign_path);
    }

    let root_campaign = root.join("campaign.toml");
    if campaign_id == "campaign" && root_campaign.is_file() {
        return Ok(root_campaign);
    }

    Err(format!(
        "Campaign configuration for '{campaign_id}' was not found."
    ))
}

fn is_simple_campaign_id(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && !value.chars().any(char::is_control)
        && Path::new(value).file_name().and_then(|name| name.to_str()) == Some(value)
}

fn workspace_root() -> PathBuf {
    if let Some(path) = std::env::var_os("SESSIONSMITH_WORKSPACE") {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return path;
        }
    }

    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("desktop app must live at apps/desktop/src-tauri")
        .to_path_buf()
}

fn build_campaign_settings(
    campaign_id: String,
    global: &GlobalConfig,
    campaign: &CampaignConfig,
    revision: String,
) -> CampaignSettings {
    CampaignSettings {
        revision,
        campaign: CampaignIdentity {
            id: campaign_id,
            name: campaign.campaign.name.clone(),
            gm: campaign.campaign.gm.clone(),
            setting: campaign.campaign.setting.clone(),
            notes: campaign.campaign.notes.clone(),
        },
        players: campaign
            .players
            .iter()
            .map(|player| Player {
                player: player.player.clone(),
                character: player.character.clone(),
                ancestry: player.ancestry.clone(),
                class: player.class.clone(),
            })
            .collect(),
        system: SystemSettings {
            preset_id: campaign.system.preset.clone(),
            overrides: campaign.system.overrides.clone(),
        },
        backend: vec![
            text_setting(
                "backend-kind",
                "Backend",
                campaign.backend.kind.as_ref(),
                Some(&global.backend.kind),
                "Not configured",
            ),
            text_setting(
                "backend-url",
                "Endpoint",
                campaign.backend.base_url.as_ref(),
                global.backend.base_url.as_ref(),
                "Not configured",
            ),
            text_setting(
                "backend-model",
                "Notes model",
                campaign.backend.model.as_ref(),
                global.backend.model.as_ref(),
                "Not configured",
            ),
            secret_setting(
                "backend-api-key",
                "API key",
                campaign.backend.api_key.as_ref(),
                global.backend.api_key.as_ref(),
            ),
        ],
        transcription: TranscriptionSettings {
            asr: vec![
                text_setting(
                    "asr-engine",
                    "Engine",
                    campaign.asr.engine.as_ref(),
                    global.asr.engine.as_ref(),
                    "Automatic",
                ),
                text_setting(
                    "asr-model",
                    "Speech model",
                    campaign.asr.model.as_ref(),
                    global.asr.model.as_ref(),
                    "Automatic",
                ),
                text_setting(
                    "asr-device",
                    "Device",
                    campaign.asr.device.as_ref(),
                    global.asr.device.as_ref(),
                    "Automatic",
                ),
                number_setting(
                    "asr-threads",
                    "Threads",
                    campaign.asr.threads,
                    global.asr.threads,
                ),
                bool_setting(
                    "asr-diarize",
                    "Speaker diarization",
                    campaign.asr.diarize,
                    global.asr.diarize,
                ),
                bool_setting(
                    "asr-vad",
                    "Silence removal",
                    campaign.asr.vad,
                    global.asr.vad,
                ),
                secret_setting(
                    "asr-hf-token",
                    "Hugging Face token",
                    campaign.asr.hf_token.as_ref(),
                    global.asr.hf_token.as_ref(),
                ),
            ],
            vocabulary: campaign.transcription.vocabulary.clone(),
            replacements: campaign
                .transcription
                .replacements
                .iter()
                .map(|(from, to)| Replacement {
                    from: from.clone(),
                    to: to.clone(),
                })
                .collect(),
            speakers: campaign
                .transcription
                .speakers
                .iter()
                .map(|(label, name)| SpeakerMapping {
                    label: label.clone(),
                    name: name.clone(),
                })
                .collect(),
            vocab_prompt: campaign.transcription.vocab_prompt,
        },
        outputs: campaign.outputs.default.clone(),
        prompt_overrides: prompt_overrides(campaign),
    }
}

fn text_setting(
    id: &str,
    label: &str,
    campaign_value: Option<&String>,
    global_value: Option<&String>,
    fallback: &str,
) -> EffectiveSetting {
    let (value, source) = match campaign_value {
        Some(value) => (value.clone(), "Campaign"),
        None => (
            global_value.cloned().unwrap_or_else(|| fallback.into()),
            "Global",
        ),
    };
    EffectiveSetting {
        id: id.into(),
        label: label.into(),
        value,
        source: source.into(),
    }
}

fn number_setting(
    id: &str,
    label: &str,
    campaign_value: Option<u32>,
    global_value: Option<u32>,
) -> EffectiveSetting {
    let (value, source) = match campaign_value {
        Some(value) => (value.to_string(), "Campaign"),
        None => (
            global_value
                .map(|value| value.to_string())
                .unwrap_or_else(|| "Automatic".into()),
            "Global",
        ),
    };
    EffectiveSetting {
        id: id.into(),
        label: label.into(),
        value,
        source: source.into(),
    }
}

fn bool_setting(
    id: &str,
    label: &str,
    campaign_value: Option<bool>,
    global_value: bool,
) -> EffectiveSetting {
    let (value, source) = match campaign_value {
        Some(value) => (value, "Campaign"),
        None => (global_value, "Global"),
    };
    EffectiveSetting {
        id: id.into(),
        label: label.into(),
        value: if value { "On" } else { "Off" }.into(),
        source: source.into(),
    }
}

fn secret_setting(
    id: &str,
    label: &str,
    campaign_value: Option<&String>,
    global_value: Option<&String>,
) -> EffectiveSetting {
    let (value, source) = if campaign_value.is_some() {
        ("Configured", "Campaign")
    } else if global_value.is_some() {
        ("Configured", "Global")
    } else {
        ("Not configured", "None")
    };
    EffectiveSetting {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        source: source.into(),
    }
}

fn prompt_overrides(campaign: &CampaignConfig) -> Vec<String> {
    [
        ("bullets", campaign.prompts.bullets.as_ref()),
        ("dm-notes", campaign.prompts.dm_notes.as_ref()),
        ("recap", campaign.prompts.recap.as_ref()),
        ("summary", campaign.prompts.summary.as_ref()),
        ("story", campaign.prompts.story.as_ref()),
        ("quotes", campaign.prompts.quotes.as_ref()),
    ]
    .into_iter()
    .filter_map(|(id, value)| value.as_ref().map(|_| id.into()))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sessionsmith::config::{CampaignAsrConfig, CampaignBackendConfig};

    #[test]
    fn effective_settings_preserve_provenance_without_exposing_secrets() {
        let mut global = GlobalConfig::default();
        global.backend.model = Some("global-model".into());
        global.backend.api_key = Some("global-secret".into());
        global.asr.diarize = false;

        let mut campaign = CampaignConfig::default();
        campaign.campaign.name = "Test campaign".into();
        campaign.backend = CampaignBackendConfig {
            model: Some("campaign-model".into()),
            api_key: Some("campaign-secret".into()),
            ..Default::default()
        };
        campaign.asr = CampaignAsrConfig {
            diarize: Some(true),
            hf_token: Some("hf-secret".into()),
            ..Default::default()
        };

        let settings = build_campaign_settings("test".into(), &global, &campaign, "revision".into());
        let model = settings
            .backend
            .iter()
            .find(|setting| setting.id == "backend-model")
            .expect("notes model setting should exist");
        assert_eq!(model.value, "campaign-model");
        assert_eq!(model.source, "Campaign");
        let diarize = settings
            .transcription
            .asr
            .iter()
            .find(|setting| setting.id == "asr-diarize")
            .expect("diarization setting should exist");
        assert_eq!(diarize.value, "On");
        assert_eq!(diarize.source, "Campaign");
        let api_key = settings
            .backend
            .iter()
            .find(|setting| setting.id == "backend-api-key")
            .expect("API key setting should exist");
        assert_eq!(api_key.value, "Configured");
        assert_eq!(api_key.source, "Campaign");
        let hf_token = settings
            .transcription
            .asr
            .iter()
            .find(|setting| setting.id == "asr-hf-token")
            .expect("Hugging Face token setting should exist");
        assert_eq!(hf_token.value, "Configured");
        assert_eq!(hf_token.source, "Campaign");

        let serialized = serde_json::to_string(&settings).expect("settings should serialize");
        assert!(!serialized.contains("global-secret"));
        assert!(!serialized.contains("campaign-secret"));
        assert!(!serialized.contains("hf-secret"));
    }

    #[test]
    fn campaign_config_path_rejects_non_simple_identifiers() {
        assert!(!is_simple_campaign_id("../outside"));
        assert!(!is_simple_campaign_id("campaign/name"));
        assert!(!is_simple_campaign_id(".hidden"));
        assert!(is_simple_campaign_id("DnDThursday"));
    }
}
