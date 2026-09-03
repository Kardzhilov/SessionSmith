import type {
  CampaignSettings,
  CampaignSummary,
  HealthReport,
  ModelEntry,
  ModelInventory,
  OnboardingState,
  SpeakerReview,
  TranscriptPage,
} from "../api/types";

export const healthReport: HealthReport = {
  checks: [
    { id: "ffmpeg", label: "FFmpeg", state: "fail", detail: "FFmpeg was not found.", remedy: "install-ffmpeg" },
    { id: "backend", label: "Notes backend", state: "ok", detail: "Ollama is reachable.", remedy: null },
  ],
  hardware: {
    os: "Linux",
    cpuCores: 8,
    ramGb: 16,
    gpu: null,
    recommendedAsrModel: "small.en",
    recommendedLlmModel: "qwen3:8b",
    recommendationReason: "Balanced defaults for this machine.",
  },
};

const modelBase: Omit<ModelEntry, "id" | "label" | "state" | "isDefault"> = {
  family: "Whisper",
  cataloged: true,
  engine: "whisper.cpp",
  sizeBytes: 500_000_000,
  released: "2025-01-01",
  detail: "A local speech model.",
  params: 244_000_000,
  languages: ["English"],
  languageSummary: "English",
  license: "MIT",
  note: null,
};

export const modelInventory: ModelInventory = {
  whisper: [
    { ...modelBase, id: "small.en", label: "Small English", state: "ready", isDefault: true },
    { ...modelBase, id: "medium.en", label: "Medium English", state: "available", isDefault: false },
  ],
  asr: [],
  ollama: [],
  ollamaService: { reachable: true, endpoint: "http://localhost:11434", detail: "Ollama is reachable." },
};

export const campaignSummary: CampaignSummary = {
  id: "thursday-game",
  name: "Thursday Game",
  gm: "Morgan",
  setting: "The Shattered Coast",
  presetId: "dnd5e",
  backendKind: "ollama",
  sessionCount: 3,
  hasCampaignLog: true,
  loadError: null,
};

export const campaignSettings: CampaignSettings = {
  revision: "revision-1",
  campaign: {
    id: campaignSummary.id,
    name: campaignSummary.name,
    gm: campaignSummary.gm,
    setting: campaignSummary.setting,
    notes: "Thursday table.",
  },
  players: [{ player: "Avery", character: "Kestrel", ancestry: "Human", class: "Rogue" }],
  system: { presetId: "dnd5e", overrides: "" },
  presets: [{ id: "dnd5e", name: "D&D 5e", description: "Fifth edition fantasy." }],
  backend: [{ id: "kind", label: "Backend", value: "ollama", source: "campaign" }],
  backendOverrides: { kind: "ollama", baseUrl: null, model: "qwen3:8b" },
  transcription: {
    asr: [{ id: "model", label: "Model", value: "small.en", source: "campaign" }],
    vocabulary: ["Kestrel"],
    replacements: [{ from: "castrol", to: "Kestrel" }],
    speakers: [{ label: "SPEAKER_00", name: "Avery" }],
    vocabPrompt: true,
  },
  asrOverrides: { model: "small.en", threads: null, diarize: true, vad: null, device: null, engine: null },
  asrModels: [{ id: "small.en", label: "Small English" }],
  outputs: ["summary", "recap"],
  promptOverrides: [],
  promptValues: { bullets: null, dmNotes: null, recap: null, summary: null, story: null, quotes: null },
};

export const onboardingState: OnboardingState = {
  revision: "onboarding-1",
  currentVersion: 1,
  completedVersion: 0,
  outcome: "pending",
  required: true,
};

export const speakerReview: SpeakerReview = {
  stem: "2026-08-27",
  speakers: [{
    label: "SPEAKER_00",
    mappedTo: null,
    samples: [{ text: "We should take the eastern road.", startMs: 12_000, endMs: 15_000 }],
  }],
  suggestedNames: ["Avery"],
  canReset: true,
};

export const transcriptPage: TranscriptPage = {
  offsetLine: 0,
  totalLines: 2,
  lines: [
    { lineNumber: 1, text: "We should take the eastern road.", t0: 12, t1: 15, speaker: "SPEAKER_00" },
    { lineNumber: 2, text: "Agreed.", t0: 16, t1: 17, speaker: "SPEAKER_01" },
  ],
};