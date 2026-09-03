use serde::{Deserialize, Serialize};
use sessionsmith::config::{
    CampaignConfig, CampaignEditableAsr, CampaignEditableBackend, CampaignEditableIdentity,
    CampaignEditableOutputs, CampaignEditablePrompts, CampaignEditableSettings,
    CampaignEditableSpeaker, CampaignEditableSystem, CampaignTextReplacement, DesktopConfig,
    GlobalConfig, Player as ConfigPlayer,
};
use std::path::{Path, PathBuf};

use crate::commands;

pub const ONBOARDING_VERSION: u32 = 1;

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub revision: String,
    pub date_format: String,
    pub appearance: String,
    pub theme: String,
    pub player_volume: u8,
    pub themes: Vec<ThemePalette>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemePalette {
    pub id: String,
    pub name: String,
    pub bg: String,
    pub fg: String,
    pub primary: String,
    pub accent: String,
    pub success: String,
    pub warn: String,
    pub error: String,
    pub muted: String,
    pub border: String,
    pub border_focus: String,
    pub selection_bg: String,
    pub selection_fg: String,
}

impl From<sessionsmith::themes::ThemePalette> for ThemePalette {
    fn from(theme: sessionsmith::themes::ThemePalette) -> Self {
        Self {
            id: theme.id,
            name: theme.name,
            bg: theme.bg,
            fg: theme.fg,
            primary: theme.primary,
            accent: theme.accent,
            success: theme.success,
            warn: theme.warn,
            error: theme.error,
            muted: theme.muted,
            border: theme.border,
            border_focus: theme.border_focus,
            selection_bg: theme.selection_bg,
            selection_fg: theme.selection_fg,
        }
    }
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsWriteRequest {
    pub expected_revision: String,
    pub date_format: String,
    pub appearance: String,
    pub theme: String,
    pub player_volume: u8,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingState {
    pub revision: String,
    pub current_version: u32,
    pub completed_version: u32,
    pub outcome: String,
    pub required: bool,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingCompleteRequest {
    pub expected_revision: String,
    pub version: u32,
    pub outcome: String,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignCreateRequest {
    pub name: String,
    pub gm: String,
    pub setting: String,
    pub notes: String,
    pub preset_id: String,
    pub players: Vec<EditablePlayer>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignCreateResult {
    pub campaign_id: String,
    pub name: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignCreateOptions {
    pub presets: Vec<PresetOption>,
}

pub(crate) fn app_settings() -> Result<AppSettings, String> {
    let path = GlobalConfig::path().map_err(|error| error.to_string())?;
    let result =
        sessionsmith::config::read_desktop_settings(&path).map_err(|error| error.to_string())?;
    Ok(AppSettings {
        revision: result.revision,
        date_format: result.settings.date_format,
        appearance: result.settings.appearance,
        theme: result.settings.theme,
        player_volume: result.settings.player_volume,
        themes: sessionsmith::themes::load_all()
            .into_iter()
            .map(ThemePalette::from)
            .collect(),
    })
}

pub(crate) fn write_app_settings(request: AppSettingsWriteRequest) -> Result<AppSettings, String> {
    let themes: Vec<_> = sessionsmith::themes::load_all()
        .into_iter()
        .map(ThemePalette::from)
        .collect();
    if !themes.iter().any(|theme| theme.id == request.theme) {
        return Err("Select an available application theme.".into());
    }
    let path = GlobalConfig::path().map_err(|error| error.to_string())?;
    let current = sessionsmith::config::read_desktop_settings(&path)
        .map_err(|error| error.to_string())?;
    if current.revision != request.expected_revision {
        return Err("app settings changed since they were read".into());
    }
    let result = sessionsmith::config::write_desktop_settings(
        &path,
        &request.expected_revision,
        DesktopConfig {
            date_format: request.date_format,
            appearance: request.appearance,
            theme: request.theme,
            player_volume: request.player_volume,
            onboarding_completed_version: current.settings.onboarding_completed_version,
            onboarding_outcome: current.settings.onboarding_outcome,
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(AppSettings {
        revision: result.revision,
        date_format: result.settings.date_format,
        appearance: result.settings.appearance,
        theme: result.settings.theme,
        player_volume: result.settings.player_volume,
        themes,
    })
}

pub(crate) fn onboarding_state() -> Result<OnboardingState, String> {
    let path = GlobalConfig::path().map_err(|error| error.to_string())?;
    let result = sessionsmith::config::read_desktop_settings(&path)
        .map_err(|error| error.to_string())?;
    Ok(build_onboarding_state(result))
}

pub(crate) fn complete_onboarding(
    request: OnboardingCompleteRequest,
) -> Result<OnboardingState, String> {
    let path = GlobalConfig::path().map_err(|error| error.to_string())?;
    complete_onboarding_at(&path, request)
}

fn complete_onboarding_at(
    path: &Path,
    request: OnboardingCompleteRequest,
) -> Result<OnboardingState, String> {
    if request.version != ONBOARDING_VERSION {
        return Err("The onboarding flow changed. Reload it before completing setup.".into());
    }
    if !matches!(request.outcome.as_str(), "finished" | "skipped") {
        return Err("Onboarding outcome must be finished or skipped.".into());
    }
    let current = sessionsmith::config::read_desktop_settings(path)
        .map_err(|error| error.to_string())?;
    if current.revision != request.expected_revision {
        return Err("app settings changed since onboarding was opened".into());
    }
    let mut desktop = current.settings;
    desktop.onboarding_completed_version = request.version;
    desktop.onboarding_outcome = request.outcome;
    let result = sessionsmith::config::write_desktop_settings(
        path,
        &request.expected_revision,
        desktop,
    )
    .map_err(|error| error.to_string())?;
    Ok(build_onboarding_state(result))
}

fn build_onboarding_state(result: sessionsmith::config::DesktopSettingsResult) -> OnboardingState {
    OnboardingState {
        revision: result.revision,
        current_version: ONBOARDING_VERSION,
        completed_version: result.settings.onboarding_completed_version,
        outcome: result.settings.onboarding_outcome,
        required: result.settings.onboarding_completed_version < ONBOARDING_VERSION,
    }
}

pub(crate) fn create_campaign(
    request: CampaignCreateRequest,
) -> Result<CampaignCreateResult, String> {
    create_campaign_at(&workspace_root(), request)
}

pub(crate) fn campaign_create_options() -> CampaignCreateOptions {
    CampaignCreateOptions {
        presets: preset_options(),
    }
}

fn create_campaign_at(
    root: &Path,
    request: CampaignCreateRequest,
) -> Result<CampaignCreateResult, String> {
    let configured_output = sessionsmith::config::output_dir();
    let output_dir = if configured_output.is_absolute() {
        configured_output
    } else {
        root.join(configured_output)
    };
    create_campaign_in(root, &output_dir, request)
}

fn create_campaign_in(
    root: &Path,
    output_dir: &Path,
    request: CampaignCreateRequest,
) -> Result<CampaignCreateResult, String> {
    let name = request.name.trim().to_string();
    let preset_id = request.preset_id.trim().to_string();
    sessionsmith::presets::load(&preset_id).map_err(|error| error.to_string())?;
    let config = sessionsmith::campaign_ops::new_campaign_config_with_notes(
        name.clone(),
        request.gm.trim().to_string(),
        request.setting.trim().to_string(),
        request
            .players
            .into_iter()
            .map(|player| ConfigPlayer {
                player: player.player.trim().to_string(),
                character: player.character.trim().to_string(),
                ancestry: player.ancestry.trim().to_string(),
                class: player.class.trim().to_string(),
            })
            .filter(|player| !player.player.is_empty() || !player.character.is_empty())
            .collect(),
        preset_id,
        request.notes.trim().to_string(),
    );
    let path = sessionsmith::campaign_ops::create_campaign(
        &root.join("campaigns"),
        output_dir,
        &config,
    )
    .map_err(|error| error.to_string())?;
    let campaign_id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| "Created campaign path was not valid UTF-8.".to_string())?
        .to_string();
    Ok(CampaignCreateResult { campaign_id, name })
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignSettings {
    pub revision: String,
    pub campaign: CampaignIdentity,
    pub players: Vec<Player>,
    pub system: SystemSettings,
    pub presets: Vec<PresetOption>,
    pub backend: Vec<EffectiveSetting>,
    pub backend_overrides: BackendOverrides,
    pub transcription: TranscriptionSettings,
    pub asr_overrides: AsrOverrides,
    pub asr_models: Vec<ModelOption>,
    pub outputs: Vec<String>,
    pub prompt_overrides: Vec<String>,
    pub prompt_values: EditablePromptOverrides,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignIdentity {
    pub id: String,
    pub name: String,
    pub gm: String,
    pub setting: String,
    pub notes: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub player: String,
    pub character: String,
    pub ancestry: String,
    pub class: String,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignSettingsWriteRequest {
    pub campaign_id: String,
    pub players: Vec<EditablePlayer>,
    pub vocabulary: Vec<String>,
    pub replacements: Vec<EditableReplacement>,
    pub identity: Option<EditableCampaignIdentity>,
    pub speakers: Option<Vec<EditableSpeakerMapping>>,
    pub outputs: Option<Vec<String>>,
    pub system: Option<EditableSystemSettings>,
    pub backend: Option<BackendOverrides>,
    pub asr: Option<AsrOverrides>,
    pub prompts: Option<EditablePromptOverrides>,
    pub expected_revision: String,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CampaignRenameRequest {
    pub campaign_id: String,
    pub new_name: String,
    pub expected_revision: String,
    pub confirmation: String,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditableCampaignIdentity {
    pub gm: String,
    pub setting: String,
    pub notes: String,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditableSpeakerMapping {
    pub label: String,
    pub name: String,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditableSystemSettings {
    pub preset_id: String,
    pub overrides: String,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditablePlayer {
    pub player: String,
    pub character: String,
    pub ancestry: String,
    pub class: String,
}

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditableReplacement {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemSettings {
    pub preset_id: String,
    pub overrides: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PresetOption {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackendOverrides {
    pub kind: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AsrOverrides {
    pub model: Option<String>,
    pub threads: Option<u32>,
    pub diarize: Option<bool>,
    pub vad: Option<bool>,
    pub device: Option<String>,
    pub engine: Option<String>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditablePromptOverrides {
    pub bullets: Option<String>,
    pub dm_notes: Option<String>,
    pub recap: Option<String>,
    pub summary: Option<String>,
    pub story: Option<String>,
    pub quotes: Option<String>,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveSetting {
    pub id: String,
    pub label: String,
    pub value: String,
    pub source: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSettings {
    pub asr: Vec<EffectiveSetting>,
    pub vocabulary: Vec<String>,
    pub replacements: Vec<Replacement>,
    pub speakers: Vec<SpeakerMapping>,
    pub vocab_prompt: bool,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Replacement {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerMapping {
    pub label: String,
    pub name: String,
}

pub(crate) fn campaign_settings(campaign_id: String) -> Result<CampaignSettings, String> {
    let library = commands::campaign_library(campaign_id)?;
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    let config_path = campaign_config_path(&library.campaign.id)?;
    let (campaign, revision) =
        CampaignConfig::load_with_revision(&config_path).map_err(|error| error.to_string())?;

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
    let result = if let Some(identity) = request.identity {
        sessionsmith::config::write_campaign_editable_identity(
            &config_path,
            &request.expected_revision,
            CampaignEditableIdentity {
                gm: identity.gm,
                setting: identity.setting,
                notes: identity.notes,
            },
        )
    } else if let Some(speakers) = request.speakers {
        sessionsmith::config::write_campaign_editable_speakers(
            &config_path,
            &request.expected_revision,
            speakers
                .into_iter()
                .map(|speaker| CampaignEditableSpeaker {
                    label: speaker.label,
                    name: speaker.name,
                })
                .collect(),
        )
    } else if let Some(outputs) = request.outputs {
        sessionsmith::config::write_campaign_editable_outputs(
            &config_path,
            &request.expected_revision,
            CampaignEditableOutputs { default: outputs },
        )
    } else if let Some(system) = request.system {
        sessionsmith::config::write_campaign_editable_system(
            &config_path,
            &request.expected_revision,
            CampaignEditableSystem {
                preset: system.preset_id,
                overrides: system.overrides,
            },
        )
    } else if let Some(backend) = request.backend {
        sessionsmith::config::write_campaign_editable_backend(
            &config_path,
            &request.expected_revision,
            CampaignEditableBackend {
                kind: backend.kind,
                base_url: backend.base_url,
                model: backend.model,
            },
        )
    } else if let Some(asr) = request.asr {
        sessionsmith::config::write_campaign_editable_asr(
            &config_path,
            &request.expected_revision,
            CampaignEditableAsr {
                model: asr.model,
                threads: asr.threads,
                diarize: asr.diarize,
                vad: asr.vad,
                device: asr.device,
                engine: asr.engine,
            },
        )
    } else if let Some(prompts) = request.prompts {
        sessionsmith::config::write_campaign_editable_prompts(
            &config_path,
            &request.expected_revision,
            CampaignEditablePrompts {
                bullets: prompts.bullets,
                dm_notes: prompts.dm_notes,
                recap: prompts.recap,
                summary: prompts.summary,
                story: prompts.story,
                quotes: prompts.quotes,
            },
        )
    } else {
        sessionsmith::config::write_campaign_editable_settings(
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
    }
    .map_err(|error| error.to_string())?;
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;

    Ok(build_campaign_settings(
        library.campaign.id,
        &global,
        &result.config,
        result.revision,
    ))
}

pub(crate) fn rename_campaign(request: CampaignRenameRequest) -> Result<CampaignSettings, String> {
    if request.confirmation != request.new_name.trim() {
        return Err("Type the new campaign name exactly to confirm the rename.".into());
    }
    let campaign_id = request.campaign_id.trim();
    if campaign_id.is_empty() {
        return Err("A campaign is required to rename it.".into());
    }
    let library = commands::campaign_library(campaign_id.into())?;
    let old_config_path = campaign_config_path(&library.campaign.id)?;
    let root = workspace_root();
    let campaigns_dir = root.join("campaigns");
    let configured_output = sessionsmith::config::output_dir();
    let output_dir = if configured_output.is_absolute() {
        configured_output
    } else {
        root.join(configured_output)
    };
    let outcome = sessionsmith::campaign_ops::rename_campaign(
        &campaigns_dir,
        &output_dir,
        &old_config_path,
        &request.new_name,
        &request.expected_revision,
    )
    .map_err(|error| error.to_string())?;
    if let Ok(global_path) = GlobalConfig::path() {
        if let Err(error) = sessionsmith::config::replace_campaign_order_id(
            &global_path,
            &outcome.old_campaign_id,
            &outcome.new_campaign_id,
        ) {
            sessionsmith::ui::warn(&format!(
                "renamed campaign but could not update campaign ordering: {error:#}"
            ));
        }
    }
    let (campaign, revision) = CampaignConfig::load_with_revision(&outcome.new_config_path)
        .map_err(|error| error.to_string())?;
    let global = GlobalConfig::load_or_default().map_err(|error| error.to_string())?;
    Ok(build_campaign_settings(
        outcome.new_campaign_id,
        &global,
        &campaign,
        revision,
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
        presets: preset_options(),
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
        backend_overrides: BackendOverrides {
            kind: campaign.backend.kind.clone(),
            base_url: campaign.backend.base_url.clone(),
            model: campaign.backend.model.clone(),
        },
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
        asr_overrides: AsrOverrides {
            model: campaign.asr.model.clone(),
            threads: campaign.asr.threads,
            diarize: campaign.asr.diarize,
            vad: campaign.asr.vad,
            device: campaign.asr.device.clone(),
            engine: campaign.asr.engine.clone(),
        },
        asr_models: sessionsmith::asr::ASR_CATALOG
            .iter()
            .map(|model| ModelOption {
                id: model.id.into(),
                label: model.display.into(),
            })
            .collect(),
        outputs: campaign.outputs.default.clone(),
        prompt_overrides: prompt_overrides(campaign),
        prompt_values: EditablePromptOverrides {
            bullets: campaign.prompts.bullets.clone(),
            dm_notes: campaign.prompts.dm_notes.clone(),
            recap: campaign.prompts.recap.clone(),
            summary: campaign.prompts.summary.clone(),
            story: campaign.prompts.story.clone(),
            quotes: campaign.prompts.quotes.clone(),
        },
    }
}

fn preset_options() -> Vec<PresetOption> {
    sessionsmith::presets::list_ids()
        .into_iter()
        .filter_map(|id| {
            sessionsmith::presets::load(id)
                .ok()
                .map(|preset| PresetOption {
                    id: id.into(),
                    name: preset.name,
                    description: preset.description,
                })
        })
        .collect()
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

    fn create_request(name: &str) -> CampaignCreateRequest {
        CampaignCreateRequest {
            name: name.into(),
            gm: "Sam".into(),
            setting: "The Reach".into(),
            notes: "Session zero complete.".into(),
            preset_id: "generic".into(),
            players: vec![EditablePlayer {
                player: "Alex".into(),
                character: "Mara".into(),
                ancestry: String::new(),
                class: String::new(),
            }],
        }
    }

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

        let settings =
            build_campaign_settings("test".into(), &global, &campaign, "revision".into());
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

    #[test]
    fn campaign_rename_confirmation_requires_the_exact_trimmed_name() {
        let request = CampaignRenameRequest {
            campaign_id: "test".into(),
            new_name: "New Name".into(),
            expected_revision: "revision".into(),
            confirmation: "new name".into(),
        };
        assert_eq!(
            rename_campaign(request).unwrap_err(),
            "Type the new campaign name exactly to confirm the rename."
        );
    }

    #[test]
    fn onboarding_state_requires_the_current_version() {
        let settings = DesktopConfig {
            onboarding_completed_version: ONBOARDING_VERSION - 1,
            onboarding_outcome: "skipped".into(),
            ..DesktopConfig::default()
        };
        let state = build_onboarding_state(sessionsmith::config::DesktopSettingsResult {
            settings,
            revision: "revision".into(),
        });
        assert!(state.required);
        assert_eq!(state.current_version, ONBOARDING_VERSION);
        assert_eq!(state.outcome, "skipped");
    }

    #[test]
    fn onboarding_completion_persists_outcome_and_rejects_stale_revision() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let initial = sessionsmith::config::read_desktop_settings(&path).unwrap();
        let state = complete_onboarding_at(
            &path,
            OnboardingCompleteRequest {
                expected_revision: initial.revision.clone(),
                version: ONBOARDING_VERSION,
                outcome: "finished".into(),
            },
        )
        .unwrap();
        assert!(!state.required);
        assert_eq!(state.completed_version, ONBOARDING_VERSION);
        assert_eq!(state.outcome, "finished");
        let persisted = sessionsmith::config::read_desktop_settings(&path).unwrap();
        assert_eq!(persisted.settings.onboarding_outcome, "finished");

        let error = complete_onboarding_at(
            &path,
            OnboardingCompleteRequest {
                expected_revision: initial.revision,
                version: ONBOARDING_VERSION,
                outcome: "skipped".into(),
            },
        )
        .unwrap_err();
        assert!(error.contains("changed since onboarding was opened"));
    }

    #[test]
    fn campaign_creation_uses_core_scaffolding_and_rejects_config_collision() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("output");
        let created = create_campaign_in(directory.path(), &output, create_request("My Game"))
            .unwrap();
        assert_eq!(created.campaign_id, "my-game");
        let config = CampaignConfig::load(
            &directory.path().join("campaigns").join("my-game.toml"),
        )
        .unwrap();
        assert_eq!(config.campaign.name, "My Game");
        assert_eq!(config.players[0].character, "Mara");

        let error = create_campaign_in(directory.path(), &output, create_request("My Game"))
            .unwrap_err();
        assert!(error.contains("campaign already exists"));
    }

    #[test]
    fn campaign_creation_rejects_output_collision_and_unknown_preset() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("output");
        std::fs::create_dir_all(output.join("my-game")).unwrap();
        let error = create_campaign_in(directory.path(), &output, create_request("My Game"))
            .unwrap_err();
        assert!(error.contains("campaign output already exists"));

        let mut invalid = create_request("Another Game");
        invalid.preset_id = "not-a-preset".into();
        let error = create_campaign_in(directory.path(), &output, invalid).unwrap_err();
        assert!(error.contains("unknown system preset"));

        let error = create_campaign_in(directory.path(), &output, create_request("   "))
            .unwrap_err();
        assert!(error.contains("campaign name cannot be empty"));
    }
}
