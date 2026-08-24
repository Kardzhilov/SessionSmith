mod audio_player;
mod commands;
mod health;
mod jobs;
mod model_inventory;
mod search;
mod settings;
mod transcript;
mod workspace;

#[tauri::command]
fn session_workspace(
    campaign_id: String,
    stem: String,
) -> Result<workspace::SessionWorkspace, String> {
    workspace::session_workspace(campaign_id, stem)
}

#[tauri::command]
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

#[tauri::command]
fn transcript_read(
    campaign_id: String,
    stem: String,
    offset_line: usize,
    limit: usize,
    query: Option<String>,
) -> Result<workspace::TranscriptPage, String> {
    workspace::transcript_read(campaign_id, stem, offset_line, limit, query)
}

#[tauri::command]
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
fn audio_play(
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> Result<audio_player::AudioPlayerSnapshot, String> {
    player.play()
}

#[tauri::command]
fn audio_pause(
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> Result<audio_player::AudioPlayerSnapshot, String> {
    player.pause()
}

#[tauri::command]
fn audio_seek(
    position_ms: u64,
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> Result<audio_player::AudioPlayerSnapshot, String> {
    player.seek(position_ms)
}

#[tauri::command]
fn audio_stop(
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> audio_player::AudioPlayerSnapshot {
    player.stop()
}

#[tauri::command]
fn audio_state(
    player: tauri::State<'_, audio_player::DesktopAudioPlayer>,
) -> audio_player::AudioPlayerSnapshot {
    player.snapshot()
}

#[tauri::command]
fn campaign_log_read(campaign_id: String) -> Result<workspace::CampaignLogDocument, String> {
    workspace::campaign_log_read(campaign_id)
}

#[tauri::command]
fn speaker_review(
    campaign_id: String,
    stem: String,
) -> Result<workspace::SpeakerReview, String> {
    workspace::speaker_review(campaign_id, stem)
}

#[tauri::command]
async fn search_query(
    campaign_id: Option<String>,
    query: String,
) -> Result<Vec<search::SearchResult>, String> {
    tauri::async_runtime::spawn_blocking(move || search::query(campaign_id, query))
        .await
        .map_err(|error| format!("Search task failed: {error}"))?
}

#[tauri::command]
async fn health_report() -> Result<health::HealthReport, String> {
    health::report().await
}

#[tauri::command]
async fn models_inventory() -> Result<model_inventory::ModelInventory, String> {
    model_inventory::inventory().await
}

#[tauri::command]
fn campaign_settings(campaign_id: String) -> Result<settings::CampaignSettings, String> {
    settings::campaign_settings(campaign_id)
}

#[tauri::command]
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
async fn job_submit_doctor(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_doctor(app)
}

#[tauri::command]
fn job_submit_record(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::RecordRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_record(app, request)
}

#[tauri::command]
fn job_submit_import(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::ImportAudioRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_import(app, request)
}

#[tauri::command]
fn job_submit_process(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::ProcessRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_process(app, request)
}

#[tauri::command]
fn job_submit_log_rebuild(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    campaign_id: String,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_log_rebuild(app, campaign_id)
}

#[tauri::command]
fn job_submit_reindex(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    campaign_id: String,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_reindex(app, campaign_id)
}

#[tauri::command]
fn job_submit_export(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::ExportRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_export(app, request)
}

#[tauri::command]
fn job_submit_notes(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::NotesRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_notes(app, request)
}

#[tauri::command]
fn job_submit_transcribe(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::TranscribeRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_transcribe(app, request)
}

#[tauri::command]
fn job_submit_model(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::ModelRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_model(app, request)
}

#[tauri::command]
fn job_submit_speaker_map(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::SpeakerMapRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_speaker_map(app, request)
}

#[tauri::command]
fn job_submit_candidate_resolve(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
    request: jobs::CandidateResolveRequest,
) -> Result<jobs::JobSubmission, String> {
    jobs.submit_candidate_resolve(app, request)
}

#[tauri::command]
fn jobs_list(jobs: tauri::State<'_, jobs::DesktopJobs>) -> Vec<jobs::JobSnapshot> {
    jobs.list()
}

#[tauri::command]
fn job_cancel(
    job_id: u64,
    app: tauri::AppHandle,
    jobs: tauri::State<'_, jobs::DesktopJobs>,
) -> Result<(), String> {
    jobs.cancel(&app, job_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(jobs::DesktopJobs::default())
        .manage(audio_player::DesktopAudioPlayer::default())
        .invoke_handler(tauri::generate_handler![
            commands::app_bootstrap,
            commands::campaign_library,
            session_workspace,
            artifact_read,
            artifact_write,
            transcript_read,
            audio_load,
            audio_play,
            audio_pause,
            audio_seek,
            audio_stop,
            audio_state,
            campaign_log_read,
            speaker_review,
            search_query,
            health_report,
            models_inventory,
            campaign_settings,
            campaign_settings_write,
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
            job_submit_candidate_resolve,
            jobs_list,
            job_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
