import type * as Generated from "./generated/bindings";
import type {
  Appearance,
  AppSettings,
  ArtifactDocument,
  ArtifactId,
  AudioPlayerSnapshot,
  AudioPlayerStatus,
  AudioPlayerTransition,
  CampaignLibrary,
  DateFormat,
  DesktopJob,
  HealthCheckState,
  HealthReport,
  JobKind,
  JobState,
  ModelEntry,
  ModelInventory,
  ModelState,
  PipelineStage,
  SearchResult,
  SessionSummary,
  SessionWorkspace,
} from "./types";

function invalidValue(field: string, value: string): never {
  throw new Error(`The desktop backend returned an invalid ${field}: ${JSON.stringify(value)}.`);
}

export function adaptArtifactId(value: string): ArtifactId {
  switch (value) {
    case "bullets":
    case "dm-notes":
    case "recap":
    case "summary":
    case "story":
    case "quotes":
      return value;
    default:
      return invalidValue("artifact id", value);
  }
}

function adaptPipelineStage(value: string): PipelineStage {
  switch (value) {
    case "audio":
    case "transcript":
    case "notes":
    case "unknown":
      return value;
    default:
      return invalidValue("pipeline stage", value);
  }
}

function adaptAudioPlayerStatus(value: string): AudioPlayerStatus {
  switch (value) {
    case "unloaded":
    case "paused":
    case "playing":
    case "stopped":
    case "ended":
      return value;
    default:
      return invalidValue("audio player status", value);
  }
}

function adaptHealthCheckState(value: string): HealthCheckState {
  switch (value) {
    case "ok":
    case "warn":
    case "fail":
      return value;
    default:
      return invalidValue("health check state", value);
  }
}

function adaptModelState(value: string): ModelState {
  switch (value) {
    case "available":
    case "installed":
    case "ready":
      return value;
    default:
      return invalidValue("model state", value);
  }
}

function adaptDateFormat(value: string): DateFormat {
  switch (value) {
    case "dmy":
    case "mdy":
    case "ymd":
    case "iso":
      return value;
    default:
      return invalidValue("date format", value);
  }
}

function adaptAppearance(value: string): Appearance {
  switch (value) {
    case "system":
    case "light":
    case "dark":
      return value;
    default:
      return invalidValue("appearance", value);
  }
}

function adaptJobKind(value: string): JobKind {
  switch (value) {
    case "import":
    case "run":
    case "transcribe":
    case "notes":
    case "doctor":
    case "rebuildLog":
    case "reindex":
    case "candidateResolve":
    case "speakerMap":
    case "sessionRename":
    case "model":
    case "export":
    case "record":
      return value;
    default:
      return invalidValue("job kind", value);
  }
}

function adaptJobState(value: string): JobState {
  switch (value) {
    case "queued":
    case "running":
    case "cancelling":
    case "succeeded":
    case "failed":
    case "cancelled":
      return value;
    default:
      return invalidValue("job state", value);
  }
}

function adaptSessionSummary(session: Generated.SessionSummary): SessionSummary {
  return {
    ...session,
    artifacts: session.artifacts.map(adaptArtifactId),
    stage: adaptPipelineStage(session.stage),
  };
}

export function adaptCampaignLibrary(library: Generated.CampaignLibrary): CampaignLibrary {
  return {
    ...library,
    sessions: library.sessions.map(adaptSessionSummary),
  };
}

export function adaptSessionWorkspace(workspace: Generated.SessionWorkspace): SessionWorkspace {
  return {
    ...workspace,
    session: adaptSessionSummary(workspace.session),
    artifacts: workspace.artifacts.map((artifact) => ({
      ...artifact,
      id: adaptArtifactId(artifact.id),
    })),
    savedArtifacts: workspace.savedArtifacts.map((artifact) => ({
      ...artifact,
      artifactId: adaptArtifactId(artifact.artifactId),
    })),
  };
}

export function adaptArtifactDocument(document: Generated.ArtifactDocument): ArtifactDocument {
  return { ...document, id: adaptArtifactId(document.id) };
}

export function adaptAudioPlayerSnapshot(snapshot: Generated.AudioPlayerSnapshot): AudioPlayerSnapshot {
  return { ...snapshot, status: adaptAudioPlayerStatus(snapshot.status) };
}

export function adaptAudioPlayerTransition(
  transition: Generated.AudioPlayerTransition,
): AudioPlayerTransition {
  return { ...transition, snapshot: adaptAudioPlayerSnapshot(transition.snapshot) };
}

export function adaptSearchResults(results: Generated.SearchResult[]): SearchResult[] {
  return results.map((result) => ({
    ...result,
    artifactId: result.artifactId === null ? null : adaptArtifactId(result.artifactId),
  }));
}

export function adaptHealthReport(report: Generated.HealthReport): HealthReport {
  return {
    ...report,
    checks: report.checks.map((check) => ({
      ...check,
      state: adaptHealthCheckState(check.state),
    })),
  };
}

function adaptModelEntry(model: Generated.ModelEntry): ModelEntry {
  return {
    ...model,
    state: adaptModelState(model.state),
    languages: model.languages === null ? null : [...model.languages],
  };
}

export function adaptModelInventory(inventory: Generated.ModelInventory): ModelInventory {
  return {
    ...inventory,
    whisper: inventory.whisper.map(adaptModelEntry),
    asr: inventory.asr.map(adaptModelEntry),
    ollama: inventory.ollama.map(adaptModelEntry),
  };
}

export function adaptAppSettings(settings: Generated.AppSettings): AppSettings {
  return {
    ...settings,
    dateFormat: adaptDateFormat(settings.dateFormat),
    appearance: adaptAppearance(settings.appearance),
  };
}

export function adaptDesktopJob(job: Generated.JobSnapshot): DesktopJob {
  return {
    ...job,
    kind: adaptJobKind(job.kind),
    state: adaptJobState(job.state),
  };
}