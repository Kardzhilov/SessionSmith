export type ArtifactId =
  | "bullets"
  | "dm-notes"
  | "recap"
  | "summary"
  | "story"
  | "quotes";

export type PipelineStage = "audio" | "transcript" | "notes" | "unknown";

export type CampaignSummary = {
  id: string;
  name: string;
  gm: string;
  setting: string;
  presetId: string;
  backendKind: string;
  sessionCount: number;
  hasCampaignLog: boolean;
  loadError: string | null;
};

export type AppBootstrap = {
  appName: string;
  version: string;
  workspacePath: string;
  campaigns: CampaignSummary[];
};

export type SessionSummary = {
  stem: string;
  hasAudio: boolean;
  hasTranscript: boolean;
  artifacts: ArtifactId[];
  stage: PipelineStage;
  modifiedAt: number | null;
};

export type InboxAudio = {
  name: string;
  path: string;
  sizeBytes: number;
  modifiedAt: number | null;
};

export type CampaignLibrary = {
  campaign: CampaignSummary;
  sessions: SessionSummary[];
  inbox: InboxAudio[];
};

export type ArtifactSummary = {
  id: ArtifactId;
  label: string;
  available: boolean;
  candidateAvailable: boolean;
  modifiedAt: number | null;
};

export type SavedArtifactSummary = {
  artifactId: ArtifactId;
  filename: string;
  label: string;
  modifiedAt: number | null;
};

export type TranscriptSummary = {
  totalLines: number;
};

export type SessionProvenance = {
  model: string;
  engine: string;
  language: string;
  vad: boolean;
  sessionDate: string | null;
  createdAt: number | null;
  sourceAudio: string | null;
  sourceFiles: string[];
  mappedSpeakers: number;
};

export type SessionWorkspace = {
  campaign: CampaignSummary;
  session: SessionSummary;
  artifacts: ArtifactSummary[];
  savedArtifacts: SavedArtifactSummary[];
  provenance: SessionProvenance | null;
  transcript: TranscriptSummary | null;
};

export type ArtifactDocument = {
  id: ArtifactId;
  label: string;
  markdown: string;
  candidate: boolean;
  modifiedAt: number | null;
  revision: string;
};

export type ArtifactWriteRequest = {
  campaignId: string;
  stem: string;
  artifactId: ArtifactId;
  markdown: string;
  expectedRevision: string;
};

export type ArtifactWriteResult = {
  modifiedAt: number | null;
  revision: string;
  indexWarning: string | null;
};

export type TranscriptLine = {
  lineNumber: number;
  text: string;
  t0: number | null;
  t1: number | null;
  speaker: string | null;
};

export type TranscriptPage = {
  offsetLine: number;
  totalLines: number;
  lines: TranscriptLine[];
};

export type AudioPlayerStatus = "unloaded" | "paused" | "playing" | "stopped" | "ended";

export type AudioPlayerSnapshot = {
  status: AudioPlayerStatus;
  label: string | null;
  positionMs: number;
  durationMs: number | null;
  error: string | null;
};

export type CampaignLogDocument = {
  markdown: string;
  modifiedAt: number | null;
};

export type SearchResult = {
  campaignId: string;
  campaignName: string;
  stem: string;
  artifactId: ArtifactId | null;
  artifactLabel: string;
  candidate: boolean;
  alternateName: string | null;
  snippet: string;
};

export type SpeakerReviewEntry = {
  label: string;
  samples: string[];
  mappedTo: string | null;
};

export type SpeakerReview = {
  stem: string;
  speakers: SpeakerReviewEntry[];
  suggestedNames: string[];
};

export type HealthCheckState = "ok" | "warn" | "fail";

export type HealthCheck = {
  id: string;
  label: string;
  state: HealthCheckState;
  detail: string;
  remedy: string | null;
};

export type HealthGpu = {
  vendor: string;
  name: string;
  vramGb: number;
};

export type HealthHardware = {
  os: string;
  cpuCores: number;
  ramGb: number;
  gpu: HealthGpu | null;
  recommendedAsrModel: string;
  recommendedLlmModel: string;
  recommendationReason: string;
};

export type HealthReport = {
  checks: HealthCheck[];
  hardware: HealthHardware;
};

export type ModelState = "available" | "installed" | "ready";

export type ModelEntry = {
  id: string;
  label: string;
  family: string | null;
  cataloged: boolean;
  engine: string;
  state: ModelState;
  isDefault: boolean;
  sizeBytes: number;
  released: string;
  detail: string;
};

export type OllamaService = {
  reachable: boolean;
  endpoint: string;
  detail: string;
};

export type ModelInventory = {
  whisper: ModelEntry[];
  asr: ModelEntry[];
  ollama: ModelEntry[];
  ollamaService: OllamaService;
};

export type CampaignIdentity = {
  id: string;
  name: string;
  gm: string;
  setting: string;
  notes: string;
};

export type CampaignPlayer = {
  player: string;
  character: string;
  ancestry: string;
  class: string;
};

export type CampaignSettingsWriteRequest = {
  campaignId: string;
  players: CampaignPlayer[];
  vocabulary: string[];
  replacements: Replacement[];
  expectedRevision: string;
};

export type SystemSettings = {
  presetId: string;
  overrides: string;
};

export type EffectiveSetting = {
  id: string;
  label: string;
  value: string;
  source: string;
};

export type Replacement = {
  from: string;
  to: string;
};

export type SpeakerMapping = {
  label: string;
  name: string;
};

export type TranscriptionSettings = {
  asr: EffectiveSetting[];
  vocabulary: string[];
  replacements: Replacement[];
  speakers: SpeakerMapping[];
  vocabPrompt: boolean;
};

export type CampaignSettings = {
  revision: string;
  campaign: CampaignIdentity;
  players: CampaignPlayer[];
  system: SystemSettings;
  backend: EffectiveSetting[];
  transcription: TranscriptionSettings;
  outputs: string[];
  promptOverrides: string[];
};

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

export type JobProgress = {
  label: string;
  position: number;
  total: number;
  rate: number | null;
};

export type DesktopJob = {
  id: number;
  kind: JobKind;
  title: string;
  state: JobState;
  startedAt: number | null;
  finishedAt: number | null;
  summary: string | null;
  activeChildren: number;
  phase: string | null;
  progress: JobProgress | null;
  logTail: string[];
  canCancel: boolean;
};

export type JobSubmission = {
  id: number;
};

export type RecordRequest = {
  name: string;
};

export type ImportAudioRequest = {
  campaignId: string;
  sourcePaths: string[];
};

export type ExportFormat = "html" | "obsidian";

export type ExportRequest = {
  campaignId: string;
  stems: string[];
  all: boolean;
  format: ExportFormat;
  playerSafe: boolean;
};

export type ProcessRequest = {
  campaignId: string;
  sourcePaths: string[];
  allInbox: boolean;
  artifactIds: ArtifactId[];
  resume: boolean;
  force: boolean;
  candidate: boolean;
  asrModel: string | null;
  language: string | null;
  sessionDate: string | null;
  diarize: boolean;
  vad: boolean;
  backendKind: string | null;
  llmModel: string | null;
  combine: boolean;
  sessionName: string | null;
};

export type NotesRequest = {
  campaignId: string;
  stem: string;
  artifactIds: ArtifactId[];
  resume: boolean;
  force: boolean;
  candidate: boolean;
};

export type TranscribeRequest = {
  campaignId: string;
  sourcePaths: string[];
  allInbox: boolean;
  force: boolean;
  asrModel: string | null;
  language: string | null;
  sessionDate: string | null;
  diarize: boolean;
  vad: boolean;
  combine: boolean;
  sessionName: string | null;
};

export type ModelAction =
  | "downloadWhisper"
  | "deleteWhisper"
  | "prepareAsr"
  | "deleteAsr"
  | "pullOllama"
  | "deleteOllama";

export type ModelRequest = {
  action: ModelAction;
  modelId: string;
};

export type ModelDefaultKind = "transcription" | "llm";

export type ModelDefaultRequest = {
  kind: ModelDefaultKind;
  modelId: string;
};

export type SpeakerMapRequest = {
  campaignId: string;
  stem: string;
  mappings: SpeakerMapping[];
};

export type CandidateAction = "keepCandidate" | "keepBoth" | "discardCandidate";

export type CandidateResolveRequest = {
  campaignId: string;
  stem: string;
  artifactId: ArtifactId;
  action: CandidateAction;
};