import { Plus, Trash2 } from "lucide-react";
import type { AsrOverrides, BackendOverrides, CampaignPlayer, CampaignSettings, EditableCampaignIdentity, ModelOption, PresetOption, PromptOverrideValues, Replacement, SpeakerMapping, SystemSettings } from "../../api/types";

const editableSettingsDraftPrefix = "sessionsmith:campaign-settings-draft:";
const maxEditablePlayers = 100;
const maxEditableVocabulary = 250;
const maxEditableReplacements = 250;

export type EditableSettingsDraft = {
  revision: string;
  players: CampaignPlayer[];
  vocabulary: string[];
  replacements: Replacement[];
  identity: EditableCampaignIdentity;
  speakers: SpeakerMapping[];
  outputs: string[];
  system: SystemSettings;
  backend: BackendOverrides;
  asr: AsrOverrides;
  prompts: PromptOverrideValues;
};

export function EditableIdentity({
  identity,
  disabled,
  onChange,
}: {
  identity: EditableCampaignIdentity;
  disabled: boolean;
  onChange: (field: keyof EditableCampaignIdentity, value: string) => void;
}) {
  return (
    <div className="settings-edit-identity">
      <EditableField label="Game master" value={identity.gm} disabled={disabled} onChange={(value) => onChange("gm", value)} />
      <EditableField label="Setting" value={identity.setting} disabled={disabled} onChange={(value) => onChange("setting", value)} />
      <label className="settings-edit-field settings-edit-field--wide">
        <span>Campaign notes</span>
        <textarea value={identity.notes} disabled={disabled} maxLength={8000} rows={5} onChange={(event) => onChange("notes", event.target.value)} />
      </label>
    </div>
  );
}

export function EditableSpeakers({
  speakers,
  disabled,
  onChange,
  onAdd,
  onRemove,
}: {
  speakers: SpeakerMapping[];
  disabled: boolean;
  onChange: (index: number, field: keyof SpeakerMapping, value: string) => void;
  onAdd: () => void;
  onRemove: (index: number) => void;
}) {
  return (
    <div className="settings-edit-list">
      {speakers.map((speaker, index) => (
        <div className="settings-edit-replacement" key={`${index}-${speaker.label}`}>
          <input type="text" value={speaker.label} disabled={disabled} maxLength={40} spellCheck="false" aria-label={`Speaker label ${index + 1}`} onChange={(event) => onChange(index, "label", event.target.value)} />
          <span aria-hidden="true">as</span>
          <input type="text" value={speaker.name} disabled={disabled} maxLength={100} aria-label={`Speaker name ${index + 1}`} onChange={(event) => onChange(index, "name", event.target.value)} />
          <button className="icon-button" type="button" onClick={() => onRemove(index)} disabled={disabled} title={`Remove speaker default ${index + 1}`} aria-label={`Remove speaker default ${index + 1}`}>
            <Trash2 size={15} aria-hidden="true" />
          </button>
        </div>
      ))}
      <button className="button button--quiet settings-edit-add" type="button" onClick={onAdd} disabled={disabled || speakers.length >= 40}>
        <Plus size={16} aria-hidden="true" /> Add speaker default
      </button>
    </div>
  );
}

export function EditableSystem({
  system,
  presets,
  disabled,
  onChange,
}: {
  system: SystemSettings;
  presets: PresetOption[];
  disabled: boolean;
  onChange: (field: keyof SystemSettings, value: string) => void;
}) {
  const presetKnown = presets.some((preset) => preset.id === system.presetId);
  return (
    <div className="settings-edit-identity">
      <label className="settings-edit-field settings-edit-field--wide">
        <span>Preset</span>
        <select value={system.presetId} disabled={disabled} onChange={(event) => onChange("presetId", event.target.value)}>
          {!presetKnown && <option value={system.presetId}>Unknown preset: {system.presetId}</option>}
          {presets.map((preset) => <option key={preset.id} value={preset.id}>{preset.name}</option>)}
        </select>
      </label>
      <label className="settings-edit-field settings-edit-field--wide">
        <span>Campaign-specific instructions</span>
        <textarea value={system.overrides} disabled={disabled} maxLength={8000} rows={6} onChange={(event) => onChange("overrides", event.target.value)} />
      </label>
      {presets.find((preset) => preset.id === system.presetId)?.description ? <p className="settings-edit-description">{presets.find((preset) => preset.id === system.presetId)?.description}</p> : null}
    </div>
  );
}

export function EditableBackend({
  backend,
  disabled,
  onChange,
}: {
  backend: BackendOverrides;
  disabled: boolean;
  onChange: <Field extends keyof BackendOverrides>(field: Field, value: BackendOverrides[Field]) => void;
}) {
  return (
    <div className="settings-edit-identity">
      <OptionalSelect label="Backend" value={backend.kind} disabled={disabled} options={[{ id: "ollama", label: "Ollama" }, { id: "openai", label: "OpenAI" }, { id: "anthropic", label: "Anthropic" }]} onChange={(value) => onChange("kind", value)} />
      <label className="settings-edit-field">
        <span>Model</span>
        <input type="text" value={backend.model ?? ""} disabled={disabled} maxLength={256} placeholder="Inherit global model" onChange={(event) => onChange("model", event.target.value || null)} />
      </label>
      <label className="settings-edit-field settings-edit-field--wide">
        <span>Endpoint</span>
        <input type="url" value={backend.baseUrl ?? ""} disabled={disabled} maxLength={2048} placeholder="Inherit global endpoint" onChange={(event) => onChange("baseUrl", event.target.value || null)} />
      </label>
    </div>
  );
}

export function EditableAsr({
  asr,
  models,
  disabled,
  onChange,
}: {
  asr: AsrOverrides;
  models: ModelOption[];
  disabled: boolean;
  onChange: <Field extends keyof AsrOverrides>(field: Field, value: AsrOverrides[Field]) => void;
}) {
  return (
    <div className="settings-edit-identity">
      <OptionalSelect label="Speech model" value={asr.model} disabled={disabled} options={models} onChange={(value) => onChange("model", value)} />
      <label className="settings-edit-field">
        <span>Threads</span>
        <input type="number" min={1} max={1024} value={asr.threads ?? ""} disabled={disabled} placeholder="Inherit" onChange={(event) => onChange("threads", event.target.value ? Number(event.target.value) : null)} />
      </label>
      <OptionalSelect label="Engine" value={asr.engine} disabled={disabled} options={[{ id: "auto", label: "Automatic" }, { id: "local", label: "Local" }, { id: "whisper-cli", label: "whisper-cli" }, { id: "whisperx", label: "WhisperX" }]} onChange={(value) => onChange("engine", value)} />
      <OptionalSelect label="Device" value={asr.device} disabled={disabled} options={[{ id: "auto", label: "Automatic" }, { id: "cuda", label: "CUDA" }, { id: "cpu", label: "CPU" }]} onChange={(value) => onChange("device", value)} />
      <TriStateSelect label="Speaker diarization" value={asr.diarize} disabled={disabled} onChange={(value) => onChange("diarize", value)} />
      <TriStateSelect label="Silence removal" value={asr.vad} disabled={disabled} onChange={(value) => onChange("vad", value)} />
    </div>
  );
}

const promptFields: Array<{ field: keyof PromptOverrideValues; label: string }> = [
  { field: "bullets", label: "Bullets prompt" },
  { field: "dmNotes", label: "DM notes prompt" },
  { field: "recap", label: "Recap prompt" },
  { field: "summary", label: "Summary prompt" },
  { field: "story", label: "Story prompt" },
  { field: "quotes", label: "Quotes prompt" },
];

export function EditablePrompts({ prompts, disabled, onChange }: { prompts: PromptOverrideValues; disabled: boolean; onChange: (field: keyof PromptOverrideValues, value: string | null) => void }) {
  return (
    <div className="settings-prompt-grid">
      {promptFields.map(({ field, label }) => (
        <label className="settings-edit-field" key={field}>
          <span>{label}</span>
          <textarea value={prompts[field] ?? ""} disabled={disabled} maxLength={8000} rows={7} placeholder="Use built-in prompt" onChange={(event) => onChange(field, event.target.value || null)} />
        </label>
      ))}
    </div>
  );
}

function OptionalSelect({ label, value, options, disabled, onChange }: { label: string; value: string | null; options: ModelOption[]; disabled: boolean; onChange: (value: string | null) => void }) {
  const known = value === null || options.some((option) => option.id === value);
  return (
    <label className="settings-edit-field">
      <span>{label}</span>
      <select value={value ?? ""} disabled={disabled} onChange={(event) => onChange(event.target.value || null)}>
        <option value="">Inherit global</option>
        {!known && value && <option value={value}>Unknown: {value}</option>}
        {options.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}
      </select>
    </label>
  );
}

function TriStateSelect({ label, value, disabled, onChange }: { label: string; value: boolean | null; disabled: boolean; onChange: (value: boolean | null) => void }) {
  return (
    <label className="settings-edit-field">
      <span>{label}</span>
      <select value={value === null ? "inherit" : value ? "on" : "off"} disabled={disabled} onChange={(event) => onChange(event.target.value === "inherit" ? null : event.target.value === "on")}>
        <option value="inherit">Inherit global</option><option value="on">On</option><option value="off">Off</option>
      </select>
    </label>
  );
}

export function EditablePlayers({
  players,
  disabled,
  onChange,
  onAdd,
  onRemove,
}: {
  players: CampaignPlayer[];
  disabled: boolean;
  onChange: (index: number, field: keyof CampaignPlayer, value: string) => void;
  onAdd: () => void;
  onRemove: (index: number) => void;
}) {
  return (
    <div className="settings-edit-player-list">
      {players.map((player, index) => (
        <article className="settings-edit-player" key={`${index}-${player.player}-${player.character}`}>
          <div className="settings-edit-player__header">
            <span>Player {index + 1}</span>
            <button
              className="icon-button"
              type="button"
              onClick={() => onRemove(index)}
              disabled={disabled}
              title={`Remove ${player.player || "player"}`}
              aria-label={`Remove ${player.player || "player"}`}
            >
              <Trash2 size={15} aria-hidden="true" />
            </button>
          </div>
          <div className="settings-edit-player__fields">
            <EditableField label="Player" value={player.player} disabled={disabled} onChange={(value) => onChange(index, "player", value)} required />
            <EditableField label="Character" value={player.character} disabled={disabled} onChange={(value) => onChange(index, "character", value)} required />
            <EditableField label="Ancestry" value={player.ancestry} disabled={disabled} onChange={(value) => onChange(index, "ancestry", value)} />
            <EditableField label="Class" value={player.class} disabled={disabled} onChange={(value) => onChange(index, "class", value)} />
          </div>
        </article>
      ))}
      <button className="button button--quiet settings-edit-add" type="button" onClick={onAdd} disabled={disabled || players.length >= maxEditablePlayers}>
        <Plus size={16} aria-hidden="true" />
        Add player
      </button>
    </div>
  );
}

export function EditableVocabulary({
  terms,
  disabled,
  onChange,
  onAdd,
  onRemove,
}: {
  terms: string[];
  disabled: boolean;
  onChange: (index: number, value: string) => void;
  onAdd: () => void;
  onRemove: (index: number) => void;
}) {
  return (
    <div className="settings-edit-list">
      {terms.map((term, index) => (
        <div className="settings-edit-row" key={`${index}-${term}`}>
          <input
            type="text"
            value={term}
            disabled={disabled}
            maxLength={240}
            spellCheck="false"
            aria-label={`Vocabulary term ${index + 1}`}
            onChange={(event) => onChange(index, event.target.value)}
          />
          <button
            className="icon-button"
            type="button"
            onClick={() => onRemove(index)}
            disabled={disabled}
            title={`Remove vocabulary term ${index + 1}`}
            aria-label={`Remove vocabulary term ${index + 1}`}
          >
            <Trash2 size={15} aria-hidden="true" />
          </button>
        </div>
      ))}
      <button className="button button--quiet settings-edit-add" type="button" onClick={onAdd} disabled={disabled || terms.length >= maxEditableVocabulary}>
        <Plus size={16} aria-hidden="true" />
        Add term
      </button>
    </div>
  );
}

export function EditableReplacements({
  replacements,
  disabled,
  onChange,
  onAdd,
  onRemove,
}: {
  replacements: Replacement[];
  disabled: boolean;
  onChange: (index: number, field: keyof Replacement, value: string) => void;
  onAdd: () => void;
  onRemove: (index: number) => void;
}) {
  return (
    <div className="settings-edit-list">
      {replacements.map((replacement, index) => (
        <div className="settings-edit-replacement" key={`${index}-${replacement.from}-${replacement.to}`}>
          <input
            type="text"
            value={replacement.from}
            disabled={disabled}
            maxLength={240}
            spellCheck="false"
            aria-label={`Correction source ${index + 1}`}
            onChange={(event) => onChange(index, "from", event.target.value)}
          />
          <span aria-hidden="true">to</span>
          <input
            type="text"
            value={replacement.to}
            disabled={disabled}
            maxLength={240}
            spellCheck="false"
            aria-label={`Correction value ${index + 1}`}
            onChange={(event) => onChange(index, "to", event.target.value)}
          />
          <button
            className="icon-button"
            type="button"
            onClick={() => onRemove(index)}
            disabled={disabled}
            title={`Remove correction ${index + 1}`}
            aria-label={`Remove correction ${index + 1}`}
          >
            <Trash2 size={15} aria-hidden="true" />
          </button>
        </div>
      ))}
      <button className="button button--quiet settings-edit-add" type="button" onClick={onAdd} disabled={disabled || replacements.length >= maxEditableReplacements}>
        <Plus size={16} aria-hidden="true" />
        Add correction
      </button>
    </div>
  );
}

export function emptyPlayer(): CampaignPlayer {
  return { player: "", character: "", ancestry: "", class: "" };
}

export function sameEditableSettings(
  settings: CampaignSettings,
  players: CampaignPlayer[],
  vocabulary: string[],
  replacements: Replacement[],
) {
  return JSON.stringify(settings.players) === JSON.stringify(players)
    && JSON.stringify(settings.transcription.vocabulary) === JSON.stringify(vocabulary)
    && JSON.stringify(settings.transcription.replacements) === JSON.stringify(replacements);
}

export function persistEditableSettingsDraft(campaignId: string, draft: EditableSettingsDraft) {
  try {
    window.localStorage.setItem(editableSettingsDraftKey(campaignId), JSON.stringify(draft));
  } catch {
    // Local recovery is optional; a full local-storage quota should not block editing.
  }
}

export function readEditableSettingsDraft(campaignId: string, revision: string): EditableSettingsDraft | null {
  try {
    const raw = window.localStorage.getItem(editableSettingsDraftKey(campaignId));
    if (!raw) {
      return null;
    }
    const draft = JSON.parse(raw) as Partial<EditableSettingsDraft>;
    if (
      draft.revision !== revision
      || !Array.isArray(draft.players)
      || !Array.isArray(draft.vocabulary)
      || !Array.isArray(draft.replacements)
      || !isEditableIdentity(draft.identity)
      || !Array.isArray(draft.speakers)
      || !Array.isArray(draft.outputs)
      || !isSystemSettings(draft.system)
      || !isBackendOverrides(draft.backend)
      || !isAsrOverrides(draft.asr)
      || !isPromptOverrides(draft.prompts)
      || !draft.players.every(isCampaignPlayer)
      || !draft.vocabulary.every((term) => typeof term === "string")
      || !draft.replacements.every(isReplacement)
      || !draft.speakers.every(isSpeakerMapping)
      || !draft.outputs.every((output) => typeof output === "string")
    ) {
      window.localStorage.removeItem(editableSettingsDraftKey(campaignId));
      return null;
    }
    return {
      revision: draft.revision,
      players: draft.players,
      vocabulary: draft.vocabulary,
      replacements: draft.replacements,
      identity: draft.identity,
      speakers: draft.speakers,
      outputs: draft.outputs,
      system: draft.system,
      backend: draft.backend,
      asr: draft.asr,
      prompts: draft.prompts,
    };
  } catch {
    return null;
  }
}

function isEditableIdentity(value: unknown): value is EditableCampaignIdentity {
  if (!value || typeof value !== "object") return false;
  const identity = value as Partial<EditableCampaignIdentity>;
  return typeof identity.gm === "string" && typeof identity.setting === "string" && typeof identity.notes === "string";
}

function isSpeakerMapping(value: unknown): value is SpeakerMapping {
  if (!value || typeof value !== "object") return false;
  const speaker = value as Partial<SpeakerMapping>;
  return typeof speaker.label === "string" && typeof speaker.name === "string";
}

function isSystemSettings(value: unknown): value is SystemSettings {
  if (!value || typeof value !== "object") return false;
  const system = value as Partial<SystemSettings>;
  return typeof system.presetId === "string" && typeof system.overrides === "string";
}

function isBackendOverrides(value: unknown): value is BackendOverrides {
  if (!value || typeof value !== "object") return false;
  const backend = value as Partial<BackendOverrides>;
  return optionalString(backend.kind) && optionalString(backend.baseUrl) && optionalString(backend.model);
}

function isAsrOverrides(value: unknown): value is AsrOverrides {
  if (!value || typeof value !== "object") return false;
  const asr = value as Partial<AsrOverrides>;
  return optionalString(asr.model)
    && (asr.threads === null || typeof asr.threads === "number")
    && optionalBoolean(asr.diarize)
    && optionalBoolean(asr.vad)
    && optionalString(asr.device)
    && optionalString(asr.engine);
}

function optionalString(value: unknown) {
  return value === null || typeof value === "string";
}

function optionalBoolean(value: unknown) {
  return value === null || typeof value === "boolean";
}

function isPromptOverrides(value: unknown): value is PromptOverrideValues {
  if (!value || typeof value !== "object") return false;
  const prompts = value as Partial<PromptOverrideValues>;
  return optionalString(prompts.bullets)
    && optionalString(prompts.dmNotes)
    && optionalString(prompts.recap)
    && optionalString(prompts.summary)
    && optionalString(prompts.story)
    && optionalString(prompts.quotes);
}

export function clearEditableSettingsDraft(campaignId: string) {
  try {
    window.localStorage.removeItem(editableSettingsDraftKey(campaignId));
  } catch {
    // Local recovery is optional.
  }
}

function EditableField({
  label,
  value,
  disabled,
  onChange,
  required = false,
}: {
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
  required?: boolean;
}) {
  return (
    <label className="settings-edit-field">
      <span>{label}</span>
      <input
        type="text"
        value={value}
        disabled={disabled}
        maxLength={240}
        required={required}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}

function editableSettingsDraftKey(campaignId: string) {
  return `${editableSettingsDraftPrefix}${campaignId}`;
}

function isCampaignPlayer(value: unknown): value is CampaignPlayer {
  if (!value || typeof value !== "object") {
    return false;
  }
  const player = value as CampaignPlayer;
  return typeof player.player === "string"
    && typeof player.character === "string"
    && typeof player.ancestry === "string"
    && typeof player.class === "string";
}

function isReplacement(value: unknown): value is Replacement {
  if (!value || typeof value !== "object") {
    return false;
  }
  const replacement = value as Replacement;
  return typeof replacement.from === "string" && typeof replacement.to === "string";
}