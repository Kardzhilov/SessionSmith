mod audio_player;
mod commands;
mod health;
mod inbox_watch;
mod jobs;
mod model_inventory;
mod search;
mod settings;
mod transcript;
mod workspace;

#[specta::specta]
#[tauri::command]
fn session_workspace(
    campaign_id: String,
    stem: String,
) -> Result<workspace::SessionWorkspace, String> {
    workspace::session_workspace(campaign_id, stem)
}

#[tauri::command]
#[specta::specta]
fn artifact_read(
    campaign_id: String,
    stem: String,
    artifact_id: String,
    candidate: bool,
    alternate_name: Option<String>,
) -> Result<workspace::ArtifactDocument, String> {
    workspace::artifact_read(campaign_id, stem, artifact_id, candidate, alternate_name)
}

#[tauri::command]
#[specta::specta]
async fn artifact_write(
    campaign_id: String,
    stem: String,
    artifact_id: String,
    markdown: String,
    expected_revision: String,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
) -> Result<workspace::ArtifactWriteResult, String> {
    let mutation = jobs.begin_artifact_mutation()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation = mutation;
        workspace::artifact_write(campaign_id, stem, artifact_id, markdown, expected_revision)
    })
    .await
    .map_err(|error| format!("Document save task failed: {error}"))?
}

#[specta::specta]
#[tauri::command]
fn transcript_read(
    campaign_id: String,
    stem: String,
    offset_line: u32,
    limit: u32,
    query: Option<String>,
) -> Result<workspace::TranscriptPage, String> {
    workspace::transcript_read(
        campaign_id,
        stem,
        offset_line as usize,
        limit as usize,
        query,
    )
}

#[specta::specta]
#[tauri::command]
fn transcript_locate(
    campaign_id: String,
    stem: String,
    position_ms: u32,
    page_size: u32,
    query: Option<String>,
) -> Result<workspace::TranscriptFollowLocation, String> {
    workspace::transcript_locate(campaign_id, stem, position_ms, page_size as usize, query)
}

#[tauri::command]
#[specta::specta]
async fn audio_load(
    campaign_id: String,
    stem: String,
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> Result<audio_player::AudioPlayerSnapshot, String> {
    let player = player.inner().clone();
    tauri::async_runtime::spawn_blocking(move || player.load(&campaign_id, &stem))
        .await
        .map_err(|error| format!("Audio load task failed: {error}"))?
}

#[tauri::command]
#[specta::specta]
fn audio_play(
    source_id: u32,
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> Result<audio_player::AudioPlayerSnapshot, String> {
    player.play(source_id)
}

#[tauri::command]
#[specta::specta]
fn audio_pause(
    source_id: u32,
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> Result<audio_player::AudioPlayerSnapshot, String> {
    player.pause(source_id)
}

#[specta::specta]
#[tauri::command]
fn audio_seek(
    source_id: u32,
    position_ms: u32,
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> Result<audio_player::AudioPlayerSnapshot, String> {
    player.seek(source_id, u64::from(position_ms))
}

#[tauri::command]
#[specta::specta]
fn audio_set_volume(
    source_id: Option<u32>,
    volume: u8,
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> Result<audio_player::AudioPlayerSnapshot, String> {
    player.set_volume(source_id, volume)
}

#[tauri::command]
#[specta::specta]
fn audio_stop(
    source_id: Option<u32>,
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> audio_player::AudioPlayerSnapshot {
    player.stop(source_id)
}

#[tauri::command]
#[specta::specta]
fn audio_state(
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> audio_player::AudioPlayerSnapshot {
    player.snapshot()
}

#[tauri::command]
#[specta::specta]
fn campaign_log_read(campaign_id: String) -> Result<workspace::CampaignLogDocument, String> {
    workspace::campaign_log_read(campaign_id)
}

#[tauri::command]
#[specta::specta]
fn speaker_review(campaign_id: String, stem: String) -> Result<workspace::SpeakerReview, String> {
    workspace::speaker_review(campaign_id, stem)
}

#[tauri::command]
#[specta::specta]
async fn session_name_suggest(campaign_id: String, stem: String) -> Result<Vec<String>, String> {
    workspace::session_name_suggest(campaign_id, stem).await
}

#[tauri::command]
#[specta::specta]
async fn search_query(
    campaign_id: Option<String>,
    query: String,
    source_kinds: Vec<String>,
) -> Result<Vec<search::SearchResult>, String> {
    tauri::async_runtime::spawn_blocking(move || search::query(campaign_id, query, source_kinds))
        .await
        .map_err(|error| format!("Search task failed: {error}"))?
}

#[tauri::command]
#[specta::specta]
fn search_sources() -> Vec<search::SearchSource> {
    search::sources()
}

#[tauri::command]
#[specta::specta]
async fn health_report() -> Result<health::HealthReport, String> {
    health::report().await
}

#[tauri::command]
#[specta::specta]
async fn models_inventory() -> Result<model_inventory::ModelInventory, String> {
    model_inventory::inventory().await
}

#[tauri::command]
#[specta::specta]
async fn model_set_default(request: model_inventory::ModelDefaultRequest) -> Result<(), String> {
    model_inventory::set_default(request).await
}

#[tauri::command]
#[specta::specta]
fn app_settings() -> Result<settings::AppSettings, String> {
    settings::app_settings()
}

#[tauri::command]
#[specta::specta]
fn export_default_dir() -> Result<String, String> {
    settings::default_export_dir()
}

#[tauri::command]
#[specta::specta]
async fn app_settings_write(
    request: settings::AppSettingsWriteRequest,
) -> Result<settings::AppSettings, String> {
    tauri::async_runtime::spawn_blocking(move || settings::write_app_settings(request))
        .await
        .map_err(|error| format!("App settings save task failed: {error}"))?
}

#[tauri::command]
#[specta::specta]
fn onboarding_state() -> Result<settings::OnboardingState, String> {
    settings::onboarding_state()
}

#[tauri::command]
#[specta::specta]
async fn onboarding_complete(
    request: settings::OnboardingCompleteRequest,
) -> Result<settings::OnboardingState, String> {
    tauri::async_runtime::spawn_blocking(move || settings::complete_onboarding(request))
        .await
        .map_err(|error| format!("Onboarding save task failed: {error}"))?
}

#[tauri::command]
#[specta::specta]
async fn campaign_create(
    request: settings::CampaignCreateRequest,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
) -> Result<settings::CampaignCreateResult, String> {
    let mutation = jobs.begin_artifact_mutation()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation = mutation;
        settings::create_campaign(request)
    })
    .await
    .map_err(|error| format!("Campaign creation task failed: {error}"))?
}

#[tauri::command]
#[specta::specta]
fn campaign_create_options() -> settings::CampaignCreateOptions {
    settings::campaign_create_options()
}

#[tauri::command]
#[specta::specta]
fn campaign_settings(campaign_id: String) -> Result<settings::CampaignSettings, String> {
    settings::campaign_settings(campaign_id)
}

#[tauri::command]
#[specta::specta]
async fn campaign_settings_write(
    request: settings::CampaignSettingsWriteRequest,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
) -> Result<settings::CampaignSettings, String> {
    let mutation = jobs.begin_artifact_mutation()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _mutation = mutation;
        settings::write_campaign_settings(request)
    })
    .await
    .map_err(|error| format!("Campaign settings save task failed: {error}"))?
}

#[tauri::command]
#[specta::specta]
async fn campaign_rename(
    request: settings::CampaignRenameRequest,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    watch: tauri::State<'_, inbox_watch::InboxWatchService>,
) -> Result<settings::CampaignSettings, String> {
    let watch_guard = watch.begin_campaign_rename()?;
    let mutation = jobs.begin_artifact_mutation()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _watch_guard = watch_guard;
        let _mutation = mutation;
        settings::rename_campaign(request)
    })
    .await
    .map_err(|error| format!("Campaign rename task failed: {error}"))?
}

#[tauri::command]
#[specta::specta]
async fn job_submit_doctor(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_doctor(app)
}

#[tauri::command]
#[specta::specta]
fn job_submit_record(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::RecordRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_record(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_import(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::ImportAudioRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_import(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_process(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::ProcessRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_process(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_log_rebuild(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    campaign_id: String,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_log_rebuild(app, campaign_id)
}

#[tauri::command]
#[specta::specta]
fn job_submit_reindex(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    campaign_id: String,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_reindex(app, campaign_id)
}

#[tauri::command]
#[specta::specta]
fn job_submit_export(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::ExportRequest,
) -> Result<jobs::ExportSubmission, String> {
    jobs.submit_export(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_notes(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::NotesRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_notes(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_transcribe(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::TranscribeRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_transcribe(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_model(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::ModelRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_model(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_speaker_map(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::SpeakerMapRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_speaker_map(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_speaker_reset(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::SpeakerResetRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_speaker_reset(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_session_rename(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::SessionRenameRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_session_rename(app, request)
}

#[tauri::command]
#[specta::specta]
fn job_submit_candidate_resolve(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::CandidateResolveRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_candidate_resolve(app, request)
}

#[tauri::command]
#[specta::specta]
fn jobs_list(jobs: tauri::State<'_, jobs::DesktopJobs>) -> Vec<jobs::JobSnapshot> {
    jobs.list()
}

#[tauri::command]
#[specta::specta]
fn jobs_clear_history(jobs: tauri::State<'_, jobs::DesktopJobs>) -> Result<(), String> {
    jobs.clear_history()
}

#[specta::specta]
#[tauri::command]
fn job_cancel(
    job_id: u32,
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
) -> Result<(), String> {
    jobs.cancel(&app, u64::from(job_id))
}

#[tauri::command]
#[specta::specta]
fn inbox_watch_start(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    watch: tauri::State<'_, inbox_watch::InboxWatchService>,
    request: inbox_watch::InboxWatchStartRequest,
) -> Result<inbox_watch::InboxWatchStatus, String> {
    watch.start(app, jobs.inner().clone(), request)
}

#[tauri::command]
#[specta::specta]
async fn inbox_watch_stop(
    app: tauri::AppHandle,
    watch: tauri::State<'_, inbox_watch::InboxWatchService>,
) -> Result<inbox_watch::InboxWatchStatus, String> {
    Ok(watch.stop(&app).await)
}

#[tauri::command]
#[specta::specta]
fn inbox_watch_status(
    watch: tauri::State<'_, inbox_watch::InboxWatchService>,
) -> inbox_watch::InboxWatchStatus {
    watch.status()
}

fn command_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
        .commands(tauri_specta::collect_commands![
            commands::app_bootstrap,
            commands::campaign_library,
            session_workspace,
            artifact_read,
            artifact_write,
            transcript_read,
            transcript_locate,
            audio_load,
            audio_play,
            audio_pause,
            audio_seek,
            audio_set_volume,
            audio_stop,
            audio_state,
            campaign_log_read,
            speaker_review,
            session_name_suggest,
            search_query,
            search_sources,
            health_report,
            models_inventory,
            model_set_default,
            app_settings,
            app_settings_write,
            export_default_dir,
            onboarding_state,
            onboarding_complete,
            campaign_create,
            campaign_create_options,
            campaign_settings,
            campaign_settings_write,
            campaign_rename,
            job_submit_doctor,
            job_submit_record,
            job_submit_import,
            job_submit_process,
            job_submit_log_rebuild,
            job_submit_reindex,
            job_submit_export,
            job_submit_notes,
            job_submit_transcribe,
            job_submit_model,
            job_submit_speaker_map,
            job_submit_speaker_reset,
            job_submit_session_rename,
            job_submit_candidate_resolve,
            jobs_list,
            jobs_clear_history,
            job_cancel,
            inbox_watch_start,
            inbox_watch_stop,
            inbox_watch_status,
        ])
        .events(tauri_specta::collect_events![
            inbox_watch::InboxWatchStatus,
            audio_player::AudioPlayerTransition
        ])
}

pub fn export_bindings() -> Result<(), String> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/api/generated/bindings.ts");
    command_builder()
        .export(specta_typescript::Typescript::default(), path)
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = command_builder();
    let event_builder = builder.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(jobs::DesktopJobs::default())
        .manage(inbox_watch::InboxWatchService::default())
        .setup(move |app| {
            use tauri::Manager;
            commands::recover_campaign_renames()
                .map_err(|error| format!("campaign rename recovery failed: {error}"))?;
            if let Err(error) = app
                .state::<jobs::DesktopJobs>()
                .initialize_history(app.handle())
            {
                eprintln!("warning: job history could not be loaded: {error}");
            }
            event_builder.mount_events(app);
            app.manage(audio_player::DesktopAudioPlayer::new(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(builder.invoke_handler())
        .build(tauri::generate_context!())
        .expect("error while running tauri application");
    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            use tauri::Manager;
            app_handle
                .state::<audio_player::DesktopAudioPlayer>()
                .stop_on_exit();
            app_handle
                .state::<inbox_watch::InboxWatchService>()
                .stop_on_exit();
        }
    });
}
