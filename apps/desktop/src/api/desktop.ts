import { listen } from "@tauri-apps/api/event";
import {
  commands,
  events,
  type CampaignSettingsWriteRequest as GeneratedCampaignSettingsWriteRequest,
  type JobSnapshot,
} from "./generated/bindings";
import {
  adaptAppSettings,
  adaptArtifactDocument,
  adaptAudioPlayerTransition,
  adaptAudioPlayerSnapshot,
  adaptCampaignLibrary,
  adaptDesktopJob,
  adaptHealthReport,
  adaptModelInventory,
  adaptSearchResults,
  adaptSessionWorkspace,
} from "./adapters";
import type {
  AppSettingsWriteRequest,
  AudioPlayerTransition,
  ArtifactId,
  ArtifactWriteRequest,
  CandidateResolveRequest,
  CampaignRenameRequest,
  CampaignCreateRequest,
  CampaignSettingsWriteRequest,
  ExportRequest,
  ImportAudioRequest,
  InboxWatchStartRequest,
  InboxWatchStatus,
  OnboardingCompleteRequest,
  ModelDefaultRequest,
  ModelRequest,
  NotesRequest,
  ProcessRequest,
  RecordRequest,
  SessionRenameRequest,
  SpeakerMapRequest,
  SpeakerResetRequest,
  TranscribeRequest,
} from "./types";

function generatedCampaignSettingsRequest(
  request: CampaignSettingsWriteRequest,
): GeneratedCampaignSettingsWriteRequest {
  return {
    ...request,
    identity: request.identity ?? null,
    speakers: request.speakers ?? null,
    outputs: request.outputs ?? null,
    system: request.system ?? null,
    backend: request.backend ?? null,
    asr: request.asr ?? null,
    prompts: request.prompts ?? null,
  };
}

export const desktop = {
  bootstrap: () => commands.appBootstrap(),
  campaignLibrary: (campaignId: string) =>
    commands.campaignLibrary(campaignId).then(adaptCampaignLibrary),
  sessionWorkspace: (campaignId: string, stem: string) =>
    commands.sessionWorkspace(campaignId, stem).then(adaptSessionWorkspace),
  artifactRead: (
    campaignId: string,
    stem: string,
    artifactId: ArtifactId,
    candidate: boolean,
    alternateName?: string,
  ) => commands.artifactRead(
    campaignId,
    stem,
    artifactId,
    candidate,
    alternateName ?? null,
  ).then(adaptArtifactDocument),
  artifactWrite: (request: ArtifactWriteRequest) =>
    commands.artifactWrite(
      request.campaignId,
      request.stem,
      request.artifactId,
      request.markdown,
      request.expectedRevision,
    ),
  transcriptRead: (
    campaignId: string,
    stem: string,
    offsetLine: number,
    limit: number,
    query?: string,
  ) => commands.transcriptRead(campaignId, stem, offsetLine, limit, query ?? null),
  transcriptLocate: (
    campaignId: string,
    stem: string,
    positionMs: number,
    pageSize: number,
    query?: string,
  ) => commands.transcriptLocate(campaignId, stem, positionMs, pageSize, query ?? null),
  audioLoad: (campaignId: string, stem: string) =>
    commands.audioLoad(campaignId, stem).then(adaptAudioPlayerSnapshot),
  audioPlay: (sourceId: number) =>
    commands.audioPlay(sourceId).then(adaptAudioPlayerSnapshot),
  audioPause: (sourceId: number) =>
    commands.audioPause(sourceId).then(adaptAudioPlayerSnapshot),
  audioSeek: (sourceId: number, positionMs: number) =>
    commands.audioSeek(sourceId, positionMs).then(adaptAudioPlayerSnapshot),
  audioSetVolume: (sourceId: number | null, volume: number) =>
    commands.audioSetVolume(sourceId, volume).then(adaptAudioPlayerSnapshot),
  audioStop: (sourceId: number | null) =>
    commands.audioStop(sourceId).then(adaptAudioPlayerSnapshot),
  audioState: () => commands.audioState().then(adaptAudioPlayerSnapshot),
  campaignLogRead: (campaignId: string) =>
    commands.campaignLogRead(campaignId),
  speakerReview: (campaignId: string, stem: string) =>
    commands.speakerReview(campaignId, stem),
  sessionNameSuggest: (campaignId: string, stem: string) =>
    commands.sessionNameSuggest(campaignId, stem),
  healthReport: () => commands.healthReport().then(adaptHealthReport),
  modelsInventory: () => commands.modelsInventory().then(adaptModelInventory),
  modelSetDefault: (request: ModelDefaultRequest) =>
    commands.modelSetDefault(request).then(() => undefined),
  appSettings: () => commands.appSettings().then(adaptAppSettings),
  appSettingsWrite: (request: AppSettingsWriteRequest) =>
    commands.appSettingsWrite(request).then(adaptAppSettings),
  exportDefaultDir: () => commands.exportDefaultDir(),
  onboardingState: () => commands.onboardingState(),
  onboardingComplete: (request: OnboardingCompleteRequest) =>
    commands.onboardingComplete(request),
  campaignCreate: (request: CampaignCreateRequest) =>
    commands.campaignCreate(request),
  campaignCreateOptions: () => commands.campaignCreateOptions(),
  campaignSettings: (campaignId: string) =>
    commands.campaignSettings(campaignId),
  campaignSettingsWrite: (request: CampaignSettingsWriteRequest) =>
    commands.campaignSettingsWrite(generatedCampaignSettingsRequest(request)),
  campaignRename: (request: CampaignRenameRequest) =>
    commands.campaignRename(request),
  searchQuery: (campaignId: string | null, query: string, sourceKinds: string[]) =>
    commands.searchQuery(campaignId, query, sourceKinds).then(adaptSearchResults),
  searchSources: () => commands.searchSources(),
  jobsList: () => commands.jobsList().then((jobs) => jobs.map(adaptDesktopJob)),
  jobsClearHistory: () => commands.jobsClearHistory().then(() => undefined),
  jobListen: (callback: (job: ReturnType<typeof adaptDesktopJob>) => void) =>
    listen<JobSnapshot>("job://updated", ({ payload }) => callback(adaptDesktopJob(payload))),
  jobSubmitDoctor: () => commands.jobSubmitDoctor(),
  jobSubmitRecord: (request: RecordRequest) =>
    commands.jobSubmitRecord(request),
  jobSubmitImport: (request: ImportAudioRequest) =>
    commands.jobSubmitImport(request),
  jobSubmitExport: (request: ExportRequest) =>
    commands.jobSubmitExport(request),
  jobSubmitProcess: (request: ProcessRequest) =>
    commands.jobSubmitProcess(request),
  jobSubmitLogRebuild: (campaignId: string) =>
    commands.jobSubmitLogRebuild(campaignId),
  jobSubmitReindex: (campaignId: string) =>
    commands.jobSubmitReindex(campaignId),
  jobSubmitNotes: (request: NotesRequest) =>
    commands.jobSubmitNotes(request),
  jobSubmitTranscribe: (request: TranscribeRequest) =>
    commands.jobSubmitTranscribe(request),
  jobSubmitModel: (request: ModelRequest) =>
    commands.jobSubmitModel(request),
  jobSubmitSpeakerMap: (request: SpeakerMapRequest) =>
    commands.jobSubmitSpeakerMap(request),
  jobSubmitSpeakerReset: (request: SpeakerResetRequest) =>
    commands.jobSubmitSpeakerReset(request),
  jobSubmitSessionRename: (request: SessionRenameRequest) =>
    commands.jobSubmitSessionRename(request),
  jobSubmitCandidateResolve: (request: CandidateResolveRequest) =>
    commands.jobSubmitCandidateResolve(request),
  jobCancel: (jobId: number) => commands.jobCancel(jobId).then(() => undefined),
  inboxWatchStart: (request: InboxWatchStartRequest) =>
    commands.inboxWatchStart({
      campaignId: request.campaignId,
      intervalSecs: request.intervalSecs ?? null,
    }),
  inboxWatchStop: () => commands.inboxWatchStop(),
  inboxWatchStatus: () => commands.inboxWatchStatus(),
  inboxWatchListen: (callback: (status: InboxWatchStatus) => void) =>
    events.inboxWatchStatus.listen(({ payload }) => callback(payload)),
  audioListen: (callback: (transition: AudioPlayerTransition) => void) =>
    events.audioTransition.listen(({ payload }) => callback(adaptAudioPlayerTransition(payload))),
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