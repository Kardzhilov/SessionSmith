import type * as Generated from "./generated/bindings";

export type {
  AppBootstrap,
  AsrOverrides,
  ArtifactWriteResult,
  AudioTransitionKind,
  BackendOverrides,
  CampaignCreateOptions,
  CampaignCreateRequest,
  CampaignCreateResult,
  CampaignSummary,
  CandidateAction,
  CampaignIdentity,
  CampaignLogDocument,
  CampaignRenameRequest,
  CampaignSettings,
  EffectiveSetting,
  EditableCampaignIdentity,
  ExportFormat,
  HealthGpu,
  HealthHardware,
  ImportAudioRequest,
  InboxAudio,
  InboxWatchStatus,
  JobProgress,
  JobSubmission,
  ModelAction,
  ModelDefaultKind,
  ModelDefaultRequest,
  ModelOption,
  ModelRequest,
  OnboardingCompleteRequest,
  OnboardingState,
  OllamaService,
  PresetOption,
  RecordRequest,
  Replacement,
  SearchSource,
  SessionProvenance,
  SessionRenameRequest,
  SpeakerMapping,
  SpeakerResetRequest,
  SpeakerReview,
  SpeakerReviewEntry,
  SpeakerReviewSample,
  SystemSettings,
  ThemePalette,
  TranscriptLine,
  TranscriptPage,
  TranscriptSummary,
  TranscriptionSettings,
} from "./generated/bindings";

export type ArtifactId =
  | "bullets"
  | "dm-notes"
  | "recap"
  | "summary"
  | "story"
  | "quotes";

export type PipelineStage = "audio" | "transcript" | "notes" | "unknown";
export type AudioPlayerStatus = "unloaded" | "paused" | "playing" | "stopped" | "ended";
export type HealthCheckState = "ok" | "warn" | "fail";
export type ModelState = "available" | "installed" | "ready";
export type DateFormat = "dmy" | "mdy" | "ymd" | "iso";
export type Appearance = "system" | "light" | "dark";

export type JobKind =
  | "import"
  | "run"
  | "transcribe"
  | "notes"
  | "doctor"
  | "rebuildLog"
  | "reindex"
  | "candidateResolve"
  | "speakerMap"
  | "sessionRename"
  | "model"
  | "export"
  | "record";

export type JobState =
  | "queued"
  | "running"
  | "cancelling"
  | "succeeded"
  | "failed"
  | "cancelled";

export type CampaignPlayer = Generated.Player;
export type PromptOverrideValues = Generated.EditablePromptOverrides;

export type SessionSummary = Omit<Generated.SessionSummary, "artifacts" | "stage"> & {
  artifacts: ArtifactId[];
  stage: PipelineStage;
};

export type CampaignLibrary = Omit<Generated.CampaignLibrary, "sessions"> & {
  sessions: SessionSummary[];
};

export type ArtifactSummary = Omit<Generated.ArtifactSummary, "id"> & {
  id: ArtifactId;
};

export type SavedArtifactSummary = Omit<Generated.SavedArtifactSummary, "artifactId"> & {
  artifactId: ArtifactId;
};

export type SessionWorkspace = Omit<
  Generated.SessionWorkspace,
  "session" | "artifacts" | "savedArtifacts"
> & {
  session: SessionSummary;
  artifacts: ArtifactSummary[];
  savedArtifacts: SavedArtifactSummary[];
};

export type ArtifactDocument = Omit<Generated.ArtifactDocument, "id"> & {
  id: ArtifactId;
};

export type AudioPlayerSnapshot = Omit<Generated.AudioPlayerSnapshot, "status"> & {
  status: AudioPlayerStatus;
};

export type AudioPlayerTransition = Omit<Generated.AudioPlayerTransition, "snapshot"> & {
  snapshot: AudioPlayerSnapshot;
};

export type SearchResult = Omit<Generated.SearchResult, "artifactId"> & {
  artifactId: ArtifactId | null;
};

export type HealthCheck = Omit<Generated.HealthCheck, "state"> & {
  state: HealthCheckState;
};

export type HealthReport = Omit<Generated.HealthReport, "checks"> & {
  checks: HealthCheck[];
};

export type ModelEntry = Omit<Generated.ModelEntry, "state" | "languages"> & {
  state: ModelState;
  languages: string[] | null;
};

export type ModelInventory = Omit<Generated.ModelInventory, "whisper" | "asr" | "ollama"> & {
  whisper: ModelEntry[];
  asr: ModelEntry[];
  ollama: ModelEntry[];
};

export type AppSettings = Omit<Generated.AppSettings, "dateFormat" | "appearance"> & {
  dateFormat: DateFormat;
  appearance: Appearance;
};

export type DesktopJob = Omit<Generated.JobSnapshot, "kind" | "state"> & {
  kind: JobKind;
  state: JobState;
};

export type ArtifactWriteRequest = {
  campaignId: string;
  stem: string;
  artifactId: ArtifactId;
  markdown: string;
  expectedRevision: string;
};

export type InboxWatchStartRequest = Omit<Generated.InboxWatchStartRequest, "intervalSecs"> & {
  intervalSecs?: number;
};

export type AppSettingsWriteRequest = Omit<
  Generated.AppSettingsWriteRequest,
  "dateFormat" | "appearance"
> & {
  dateFormat: DateFormat;
  appearance: Appearance;
};

export type CampaignSettingsWriteRequest = Omit<
  Generated.CampaignSettingsWriteRequest,
  "identity" | "speakers" | "outputs" | "system" | "backend" | "asr" | "prompts"
> & {
  identity?: Generated.EditableCampaignIdentity;
  speakers?: Generated.EditableSpeakerMapping[];
  outputs?: string[];
  system?: Generated.EditableSystemSettings;
  backend?: Generated.BackendOverrides;
  asr?: Generated.AsrOverrides;
  prompts?: Generated.EditablePromptOverrides;
};

export type ExportRequest = Omit<Generated.ExportRequest, "stems" | "all" | "playerSafe"> & {
  stems: string[];
  all: boolean;
  playerSafe: boolean;
};

export type ProcessRequest = Omit<
  Generated.ProcessRequest,
  "allInbox" | "artifactIds" | "diarize" | "vad" | "combine"
> & {
  allInbox: boolean;
  artifactIds: ArtifactId[];
  diarize: boolean;
  vad: boolean;
  combine: boolean;
};

export type NotesRequest = Omit<Generated.NotesRequest, "artifactIds"> & {
  artifactIds: ArtifactId[];
};

export type TranscribeRequest = Omit<
  Generated.TranscribeRequest,
  "allInbox" | "diarize" | "vad" | "combine"
> & {
  allInbox: boolean;
  diarize: boolean;
  vad: boolean;
  combine: boolean;
};

export type CandidateResolveRequest = Omit<Generated.CandidateResolveRequest, "artifactId"> & {
  artifactId: ArtifactId;
};

export type SpeakerMapRequest = Omit<Generated.SpeakerMapRequest, "mappings"> & {
  mappings: Generated.SpeakerMapping[];
};