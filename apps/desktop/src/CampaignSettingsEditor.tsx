import { Plus, Trash2 } from "lucide-react";
import type { CampaignPlayer, CampaignSettings, Replacement } from "./types";

const editableSettingsDraftPrefix = "sessionsmith:campaign-settings-draft:";
const maxEditablePlayers = 100;
const maxEditableVocabulary = 250;
const maxEditableReplacements = 250;

export type EditableSettingsDraft = {
  revision: string;
  players: CampaignPlayer[];
  vocabulary: string[];
  replacements: Replacement[];
};

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
      || !draft.players.every(isCampaignPlayer)
      || !draft.vocabulary.every((term) => typeof term === "string")
      || !draft.replacements.every(isReplacement)
    ) {
      window.localStorage.removeItem(editableSettingsDraftKey(campaignId));
      return null;
    }
    return {
      revision: draft.revision,
      players: draft.players,
      vocabulary: draft.vocabulary,
      replacements: draft.replacements,
    };
  } catch {
    return null;
  }
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