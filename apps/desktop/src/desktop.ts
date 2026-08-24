import { invoke } from "@tauri-apps/api/core";
import type {
  AudioPlayerSnapshot,
  AppBootstrap,
  ArtifactDocument,
  ArtifactId,
  ArtifactWriteRequest,
  ArtifactWriteResult,
  CandidateResolveRequest,
  CampaignLibrary,
  CampaignLogDocument,
  CampaignSettings,
  CampaignSettingsWriteRequest,
  DesktopJob,
  ExportRequest,
  HealthReport,
  ImportAudioRequest,
  JobSubmission,
  ModelInventory,
  ModelRequest,
  NotesRequest,
  ProcessRequest,
  RecordRequest,
  SessionWorkspace,
  SearchResult,
  SpeakerMapRequest,
  SpeakerReview,
  TranscribeRequest,
  TranscriptPage,
} from "./types";

export const desktop = {
  bootstrap: () => invoke<AppBootstrap>("app_bootstrap"),
  campaignLibrary: (campaignId: string) =>
    invoke<CampaignLibrary>("campaign_library", { campaignId }),
  sessionWorkspace: (campaignId: string, stem: string) =>
    invoke<SessionWorkspace>("session_workspace", { campaignId, stem }),
  artifactRead: (
    campaignId: string,
    stem: string,
    artifactId: ArtifactId,
    candidate: boolean,
    alternateName?: string,
  ) => invoke<ArtifactDocument>("artifact_read", {
    campaignId,
    stem,
    artifactId,
    candidate,
    alternateName,
  }),
  artifactWrite: (request: ArtifactWriteRequest) =>
    invoke<ArtifactWriteResult>("artifact_write", request),
  transcriptRead: (
    campaignId: string,
    stem: string,
    offsetLine: number,
    limit: number,
    query?: string,
  ) => invoke<TranscriptPage>("transcript_read", { campaignId, stem, offsetLine, limit, query }),
  audioLoad: (campaignId: string, stem: string) =>
    invoke<AudioPlayerSnapshot>("audio_load", { campaignId, stem }),
  audioPlay: () => invoke<AudioPlayerSnapshot>("audio_play"),
  audioPause: () => invoke<AudioPlayerSnapshot>("audio_pause"),
  audioSeek: (positionMs: number) =>
    invoke<AudioPlayerSnapshot>("audio_seek", { positionMs }),
  audioStop: () => invoke<AudioPlayerSnapshot>("audio_stop"),
  audioState: () => invoke<AudioPlayerSnapshot>("audio_state"),
  campaignLogRead: (campaignId: string) =>
    invoke<CampaignLogDocument>("campaign_log_read", { campaignId }),
  speakerReview: (campaignId: string, stem: string) =>
    invoke<SpeakerReview>("speaker_review", { campaignId, stem }),
  healthReport: () => invoke<HealthReport>("health_report"),
  modelsInventory: () => invoke<ModelInventory>("models_inventory"),
  campaignSettings: (campaignId: string) =>
    invoke<CampaignSettings>("campaign_settings", { campaignId }),
  campaignSettingsWrite: (request: CampaignSettingsWriteRequest) =>
    invoke<CampaignSettings>("campaign_settings_write", { request }),
  searchQuery: (campaignId: string | null, query: string) =>
    invoke<SearchResult[]>("search_query", { campaignId, query }),
  jobsList: () => invoke<DesktopJob[]>("jobs_list"),
  jobSubmitDoctor: () => invoke<JobSubmission>("job_submit_doctor"),
  jobSubmitRecord: (request: RecordRequest) =>
    invoke<JobSubmission>("job_submit_record", { request }),
  jobSubmitImport: (request: ImportAudioRequest) =>
    invoke<JobSubmission>("job_submit_import", { request }),
  jobSubmitExport: (request: ExportRequest) =>
    invoke<JobSubmission>("job_submit_export", { request }),
  jobSubmitProcess: (request: ProcessRequest) =>
    invoke<JobSubmission>("job_submit_process", { request }),
  jobSubmitLogRebuild: (campaignId: string) =>
    invoke<JobSubmission>("job_submit_log_rebuild", { campaignId }),
  jobSubmitReindex: (campaignId: string) =>
    invoke<JobSubmission>("job_submit_reindex", { campaignId }),
  jobSubmitNotes: (request: NotesRequest) =>
    invoke<JobSubmission>("job_submit_notes", { request }),
  jobSubmitTranscribe: (request: TranscribeRequest) =>
    invoke<JobSubmission>("job_submit_transcribe", { request }),
  jobSubmitModel: (request: ModelRequest) =>
    invoke<JobSubmission>("job_submit_model", { request }),
  jobSubmitSpeakerMap: (request: SpeakerMapRequest) =>
    invoke<JobSubmission>("job_submit_speaker_map", { request }),
  jobSubmitCandidateResolve: (request: CandidateResolveRequest) =>
    invoke<JobSubmission>("job_submit_candidate_resolve", { request }),
  jobCancel: (jobId: number) => invoke<void>("job_cancel", { jobId }),
};

export function errorMessage(error: unknown) {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  try {
    return JSON.stringify(error);
  } catch {
    return "The desktop backend returned an unknown error.";
  }
}