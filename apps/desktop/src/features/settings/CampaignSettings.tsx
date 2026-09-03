import { type ReactNode, useEffect, useState } from "react";
import {
  Bot,
  BrainCircuit,
  CircleAlert,
  CircleCheck,
  FileOutput,
  LoaderCircle,
  Pencil,
  RefreshCw,
  Save,
  Settings2,
  X,
} from "lucide-react";
import {
  clearEditableSettingsDraft,
  EditableIdentity,
  EditableAsr,
  EditableBackend,
  EditableSpeakers,
  EditableSystem,
  EditablePlayers,
  EditablePrompts,
  EditableReplacements,
  EditableVocabulary,
  emptyPlayer,
  persistEditableSettingsDraft,
  readEditableSettingsDraft,
  sameEditableSettings,
} from "./CampaignSettingsEditor";
import { desktop, errorMessage } from "../../api/desktop";
import type {
  AsrOverrides,
  BackendOverrides,
  CampaignSettings,
  CampaignPlayer,
  CampaignSummary,
  EditableCampaignIdentity,
  EffectiveSetting,
  Replacement,
  PromptOverrideValues,
  SpeakerMapping,
  SystemSettings,
} from "../../api/types";

type EditableSettingsSection = "identity" | "players" | "vocabulary" | "corrections" | "speakers" | "outputs" | "system" | "backend" | "asr" | "prompts";

const outputArtifacts = [
  { id: "bullets", label: "Bullets" },
  { id: "dm-notes", label: "DM notes" },
  { id: "recap", label: "Player recap" },
  { id: "summary", label: "Summary" },
  { id: "story", label: "Story" },
  { id: "quotes", label: "Quotes" },
] as const;

export function CampaignSettingsPage({
  campaign,
  onCampaignRenamed,
}: {
  campaign: CampaignSummary | undefined;
  onCampaignRenamed: (campaignId: string) => void;
}) {
  const [settings, setSettings] = useState<CampaignSettings | null>(null);
  const [loading, setLoading] = useState(Boolean(campaign));
  const [error, setError] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [editing, setEditing] = useState<EditableSettingsSection | null>(null);
  const [draftPlayers, setDraftPlayers] = useState<CampaignPlayer[]>([]);
  const [draftVocabulary, setDraftVocabulary] = useState<string[]>([]);
  const [draftReplacements, setDraftReplacements] = useState<Replacement[]>([]);
  const [draftIdentity, setDraftIdentity] = useState<EditableCampaignIdentity>({ gm: "", setting: "", notes: "" });
  const [draftSpeakers, setDraftSpeakers] = useState<SpeakerMapping[]>([]);
  const [draftOutputs, setDraftOutputs] = useState<string[]>([]);
  const [draftSystem, setDraftSystem] = useState<SystemSettings>({ presetId: "generic", overrides: "" });
  const [draftBackend, setDraftBackend] = useState<BackendOverrides>({ kind: null, baseUrl: null, model: null });
  const [draftAsr, setDraftAsr] = useState<AsrOverrides>({ model: null, threads: null, diarize: null, vad: null, device: null, engine: null });
  const [draftPrompts, setDraftPrompts] = useState<PromptOverrideValues>({ bullets: null, dmNotes: null, recap: null, summary: null, story: null, quotes: null });
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [renameName, setRenameName] = useState("");
  const [renameConfirmation, setRenameConfirmation] = useState("");
  const campaignId = campaign?.id;

  useEffect(() => {
    if (!campaignId) {
      setSettings(null);
      setError(null);
      setLoading(false);
      setEditing(null);
      setSaveError(null);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);
    setEditing(null);
    setSaveError(null);

    void desktop
      .campaignSettings(campaignId)
      .then((nextSettings) => {
        if (!cancelled) {
          setSettings(nextSettings);
        }
      })
      .catch((nextError) => {
        if (!cancelled) {
          setError(errorMessage(nextError));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [campaignId, refreshKey]);

  const draftDirty = settings !== null && editing !== null && (editing === "identity"
    ? JSON.stringify(draftIdentity) !== JSON.stringify({
        gm: settings.campaign.gm,
        setting: settings.campaign.setting,
        notes: settings.campaign.notes,
      })
    : editing === "speakers"
      ? JSON.stringify(draftSpeakers) !== JSON.stringify(settings.transcription.speakers)
      : editing === "outputs"
        ? JSON.stringify(draftOutputs) !== JSON.stringify(settings.outputs)
        : editing === "system"
          ? JSON.stringify(draftSystem) !== JSON.stringify(settings.system)
          : editing === "backend"
            ? JSON.stringify(draftBackend) !== JSON.stringify(settings.backendOverrides)
            : editing === "asr"
              ? JSON.stringify(draftAsr) !== JSON.stringify(settings.asrOverrides)
              : editing === "prompts"
                ? JSON.stringify(draftPrompts) !== JSON.stringify(settings.promptValues)
                : !sameEditableSettings(
        settings,
        editing === "players" ? draftPlayers : settings.players,
        editing === "vocabulary" ? draftVocabulary : settings.transcription.vocabulary,
        editing === "corrections" ? draftReplacements : settings.transcription.replacements,
      ));
  const canSave = editing !== null
    && draftDirty
    && !saving
    && (editing !== "players" || draftPlayers.every((player) => player.player.trim() && player.character.trim()))
    && (editing !== "vocabulary" || draftVocabulary.every((term) => term.trim()))
    && (editing !== "corrections" || draftReplacements.every((replacement) => replacement.from.trim() && replacement.to.trim()));
  const speakerLabels = draftSpeakers.map((speaker) => speaker.label.trim());
  const speakersValid = draftSpeakers.every((speaker) => /^SPEAKER_\d+$/.test(speaker.label.trim()) && speaker.name.trim())
    && new Set(speakerLabels).size === speakerLabels.length;
  const sectionCanSave = canSave
    && (editing !== "speakers" || speakersValid)
    && (editing !== "system" || settings.presets.some((preset) => preset.id === draftSystem.presetId))
    && (editing !== "backend" || validBackendOverrides(draftBackend))
    && (editing !== "asr" || validAsrOverrides(draftAsr, settings.asrModels.map((model) => model.id)));

  useEffect(() => {
    if (!editing || !campaignId || !settings) {
      return;
    }
    if (!draftDirty) {
      clearEditableSettingsDraft(campaignId);
      return;
    }
    persistEditableSettingsDraft(campaignId, {
      revision: settings.revision,
      players: draftPlayers,
      vocabulary: draftVocabulary,
      replacements: draftReplacements,
      identity: draftIdentity,
      speakers: draftSpeakers,
      outputs: draftOutputs,
      system: draftSystem,
      backend: draftBackend,
      asr: draftAsr,
      prompts: draftPrompts,
    });
  }, [campaignId, draftAsr, draftBackend, draftDirty, draftIdentity, draftOutputs, draftPlayers, draftPrompts, draftReplacements, draftSpeakers, draftSystem, draftVocabulary, editing, settings]);

  useEffect(() => {
    if (!editing || !draftDirty) {
      return;
    }
    const warnBeforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", warnBeforeUnload);
    return () => window.removeEventListener("beforeunload", warnBeforeUnload);
  }, [draftDirty, editing]);

  function beginEditing(section: EditableSettingsSection) {
    if (!settings || !campaignId) {
      return;
    }
    const recovered = readEditableSettingsDraft(campaignId, settings.revision);
    setDraftPlayers(recovered?.players ?? settings.players);
    setDraftVocabulary(recovered?.vocabulary ?? settings.transcription.vocabulary);
    setDraftReplacements(recovered?.replacements ?? settings.transcription.replacements);
    setDraftIdentity(recovered?.identity ?? {
      gm: settings.campaign.gm,
      setting: settings.campaign.setting,
      notes: settings.campaign.notes,
    });
    setDraftSpeakers(recovered?.speakers ?? settings.transcription.speakers);
    setDraftOutputs(recovered?.outputs ?? settings.outputs);
    setDraftSystem(recovered?.system ?? settings.system);
    setDraftBackend(recovered?.backend ?? settings.backendOverrides);
    setDraftAsr(recovered?.asr ?? settings.asrOverrides);
    setDraftPrompts(recovered?.prompts ?? settings.promptValues);
    setSaveError(null);
    setEditing(section);
  }

  function discardEditing() {
    if (campaignId) {
      clearEditableSettingsDraft(campaignId);
    }
    setDraftPlayers(settings?.players ?? []);
    setDraftVocabulary(settings?.transcription.vocabulary ?? []);
    setDraftReplacements(settings?.transcription.replacements ?? []);
    setDraftIdentity({
      gm: settings?.campaign.gm ?? "",
      setting: settings?.campaign.setting ?? "",
      notes: settings?.campaign.notes ?? "",
    });
    setDraftSpeakers(settings?.transcription.speakers ?? []);
    setDraftOutputs(settings?.outputs ?? []);
    setDraftSystem(settings?.system ?? { presetId: "generic", overrides: "" });
    setDraftBackend(settings?.backendOverrides ?? { kind: null, baseUrl: null, model: null });
    setDraftAsr(settings?.asrOverrides ?? { model: null, threads: null, diarize: null, vad: null, device: null, engine: null });
    setDraftPrompts(settings?.promptValues ?? { bullets: null, dmNotes: null, recap: null, summary: null, story: null, quotes: null });
    setSaveError(null);
    setEditing(null);
  }

  async function saveEditableSettings() {
    if (!settings || !campaignId || !sectionCanSave) {
      return;
    }
    setSaving(true);
    setSaveError(null);
    try {
      const nextSettings = await desktop.campaignSettingsWrite({
        campaignId,
        players: draftPlayers,
        vocabulary: draftVocabulary,
        replacements: draftReplacements,
        identity: editing === "identity" ? draftIdentity : undefined,
        speakers: editing === "speakers" ? draftSpeakers : undefined,
        outputs: editing === "outputs" ? draftOutputs : undefined,
        system: editing === "system" ? draftSystem : undefined,
        backend: editing === "backend" ? draftBackend : undefined,
        asr: editing === "asr" ? draftAsr : undefined,
        prompts: editing === "prompts" ? draftPrompts : undefined,
        expectedRevision: settings.revision,
      });
      clearEditableSettingsDraft(campaignId);
      setSettings(nextSettings);
      setDraftPlayers(nextSettings.players);
      setDraftVocabulary(nextSettings.transcription.vocabulary);
      setDraftReplacements(nextSettings.transcription.replacements);
      setDraftIdentity({
        gm: nextSettings.campaign.gm,
        setting: nextSettings.campaign.setting,
        notes: nextSettings.campaign.notes,
      });
      setDraftSpeakers(nextSettings.transcription.speakers);
      setDraftOutputs(nextSettings.outputs);
      setDraftSystem(nextSettings.system);
      setDraftBackend(nextSettings.backendOverrides);
      setDraftAsr(nextSettings.asrOverrides);
      setDraftPrompts(nextSettings.promptValues);
      setEditing(null);
    } catch (nextError) {
      setSaveError(errorMessage(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function renameCampaign() {
    if (!settings || !campaignId) {
      return;
    }
    const newName = renameName.trim();
    if (!newName || newName === settings.campaign.name || renameConfirmation !== newName) {
      return;
    }
    setSaving(true);
    setSaveError(null);
    try {
      const nextSettings = await desktop.campaignRename({
        campaignId,
        newName,
        expectedRevision: settings.revision,
        confirmation: renameConfirmation,
      });
      clearEditableSettingsDraft(campaignId);
      setSettings(nextSettings);
      setRenaming(false);
      setRenameConfirmation("");
      onCampaignRenamed(nextSettings.campaign.id);
    } catch (nextError) {
      setSaveError(errorMessage(nextError));
    } finally {
      setSaving(false);
    }
  }

  function updatePlayer(index: number, field: keyof CampaignPlayer, value: string) {
    setDraftPlayers((players) => players.map((player, playerIndex) => (
      playerIndex === index ? { ...player, [field]: value } : player
    )));
  }

  function updateVocabulary(index: number, value: string) {
    setDraftVocabulary((terms) => terms.map((term, termIndex) => termIndex === index ? value : term));
  }

  function updateReplacement(index: number, field: keyof Replacement, value: string) {
    setDraftReplacements((replacements) => replacements.map((replacement, replacementIndex) => (
      replacementIndex === index ? { ...replacement, [field]: value } : replacement
    )));
  }

  function updateIdentity(field: keyof EditableCampaignIdentity, value: string) {
    setDraftIdentity((identity) => ({ ...identity, [field]: value }));
  }

  function updateSpeaker(index: number, field: keyof SpeakerMapping, value: string) {
    setDraftSpeakers((speakers) => speakers.map((speaker, speakerIndex) => speakerIndex === index ? { ...speaker, [field]: value } : speaker));
  }

  function updateSystem(field: keyof SystemSettings, value: string) {
    setDraftSystem((system) => ({ ...system, [field]: value }));
  }

  function updateBackend<Field extends keyof BackendOverrides>(field: Field, value: BackendOverrides[Field]) {
    setDraftBackend((backend) => ({ ...backend, [field]: value }));
  }

  function updateAsr<Field extends keyof AsrOverrides>(field: Field, value: AsrOverrides[Field]) {
    setDraftAsr((asr) => ({ ...asr, [field]: value }));
  }

  function updatePrompt(field: keyof PromptOverrideValues, value: string | null) {
    setDraftPrompts((prompts) => ({ ...prompts, [field]: value }));
  }

  function sectionAction(section: EditableSettingsSection | null) {
    if (section && editing === section) {
      return (
        <div className="settings-section__actions">
          <button className="button button--quiet button--compact" type="button" onClick={discardEditing} disabled={saving}>
            <X size={15} aria-hidden="true" /> Discard
          </button>
          <button className="button button--primary button--compact" type="button" onClick={() => void saveEditableSettings()} disabled={!sectionCanSave}>
            {saving ? <LoaderCircle className="is-spinning" size={15} aria-hidden="true" /> : <Save size={15} aria-hidden="true" />}
            {saving ? "Saving" : "Save"}
          </button>
        </div>
      );
    }
    const switchingBlocked = editing !== null || renaming;
    const title = !section
      ? "Editing arrives in a later update"
      : switchingBlocked
        ? "Save or discard the active section first"
        : `Edit ${section}`;
    return (
      <button
        className="button button--quiet button--compact"
        type="button"
        disabled={!section || switchingBlocked}
        title={title}
        onClick={() => section && beginEditing(section)}
      >
        <Pencil size={15} aria-hidden="true" /> Edit
      </button>
    );
  }

  if (!campaign) {
    return (
      <section className="state-panel" aria-live="polite">
        <Settings2 size={24} aria-hidden="true" />
        <div>
          <p className="eyebrow">Campaign settings</p>
          <h1>Select a campaign to inspect its configuration.</h1>
        </div>
      </section>
    );
  }

  if (!settings && loading) {
    return <SettingsLoading />;
  }

  if (!settings) {
    return (
      <section className="state-panel state-panel--error" aria-live="polite">
        <CircleAlert size={22} aria-hidden="true" />
        <div>
          <p className="eyebrow">Configuration unavailable</p>
          <h1>SessionSmith could not read this campaign's settings.</h1>
          <p>{error ?? "The campaign settings did not return a result."}</p>
          <button className="button button--quiet" type="button" onClick={() => setRefreshKey((key) => key + 1)}>
            <RefreshCw size={16} aria-hidden="true" />
            Refresh settings
          </button>
        </div>
      </section>
    );
  }

  return (
    <div className="settings-page">
      <header className="page-header settings-page__header">
        <div>
          <p className="eyebrow">Configuration snapshot</p>
          <h1>Campaign settings</h1>
          <p className="page-subtitle">Effective values, campaign context, and transcription terminology.</p>
        </div>
        <div className="page-actions">
          <button
            className="button button--quiet"
            type="button"
            onClick={() => setRefreshKey((key) => key + 1)}
            disabled={loading || saving || editing !== null}
          >
            <RefreshCw className={loading ? "is-spinning" : ""} size={16} aria-hidden="true" />
            Refresh settings
          </button>
        </div>
      </header>

      {error && (
        <p className="settings-refresh-error" role="status">
          The latest refresh did not complete. Showing the previous configuration: {error}
        </p>
      )}

      {saveError && <p className="settings-save-error" role="alert">{saveError}</p>}

      <section className="settings-identity" aria-labelledby="campaign-identity-heading">
        <div className="settings-identity__intro">
          <span className="settings-identity__icon" aria-hidden="true">
            <Settings2 size={20} />
          </span>
          <div>
            <p className="eyebrow">Campaign</p>
            {renaming ? (
              <div className="settings-campaign-rename">
                <label className="settings-edit-field">
                  <span>New campaign name</span>
                  <input type="text" value={renameName} disabled={saving} maxLength={100} autoFocus onChange={(event) => setRenameName(event.target.value)} />
                </label>
                <label className="settings-edit-field">
                  <span>Type the new name to confirm</span>
                  <input type="text" value={renameConfirmation} disabled={saving} maxLength={100} onChange={(event) => setRenameConfirmation(event.target.value)} />
                </label>
                <div className="settings-section__actions">
                  <button className="button button--quiet button--compact" type="button" disabled={saving} onClick={() => { setRenaming(false); setRenameConfirmation(""); }}>
                    <X size={15} aria-hidden="true" /> Cancel
                  </button>
                  <button className="button button--primary button--compact" type="button" disabled={saving || !renameName.trim() || renameName.trim() === settings.campaign.name || renameConfirmation !== renameName.trim()} onClick={() => void renameCampaign()}>
                    {saving ? <LoaderCircle className="is-spinning" size={15} aria-hidden="true" /> : <Save size={15} aria-hidden="true" />}
                    {saving ? "Renaming" : "Rename"}
                  </button>
                </div>
              </div>
            ) : (
              <div className="settings-campaign-title">
                <h2 id="campaign-identity-heading">{settings.campaign.name}</h2>
                <button className="button button--quiet button--compact" type="button" disabled={editing !== null || saving} onClick={() => { setRenameName(settings.campaign.name); setRenameConfirmation(""); setSaveError(null); setRenaming(true); }}>
                  <Pencil size={15} aria-hidden="true" /> Rename
                </button>
              </div>
            )}
            {settings.campaign.notes ? (
              <p className="settings-identity__notes">{settings.campaign.notes}</p>
            ) : (
              <p className="settings-identity__notes settings-identity__notes--empty">No campaign notes.</p>
            )}
          </div>
        </div>
        <div className="settings-identity__aside">
          {sectionAction("identity")}
          {editing === "identity" ? (
            <EditableIdentity identity={draftIdentity} disabled={saving} onChange={updateIdentity} />
          ) : <dl className="settings-metrics">
          <div>
            <dt>Game master</dt>
            <dd>{settings.campaign.gm || "Not set"}</dd>
          </div>
          <div>
            <dt>Setting</dt>
            <dd>{settings.campaign.setting || "Not set"}</dd>
          </div>
          <div>
            <dt>Preset</dt>
            <dd>{settings.system.presetId}</dd>
          </div>
          </dl>}
        </div>
      </section>

      <SettingsSection
        title="Players"
        detail={`${settings.players.length} ${pluralize(settings.players.length, "player")} in the campaign context.`}
        icon={<Settings2 size={18} aria-hidden="true" />}
        action={sectionAction("players")}
      >
        {editing === "players" ? (
          <EditablePlayers
            players={draftPlayers}
            disabled={saving}
            onChange={updatePlayer}
            onAdd={() => setDraftPlayers((players) => [...players, emptyPlayer()])}
            onRemove={(index) => setDraftPlayers((players) => players.filter((_, playerIndex) => playerIndex !== index))}
          />
        ) : settings.players.length > 0 ? (
          <div className="settings-player-list">
            {settings.players.map((player) => (
              <article className="settings-player" key={`${player.player}-${player.character}`}>
                <h3>{player.player || "Unnamed player"}</h3>
                <dl>
                  <div>
                    <dt>Character</dt>
                    <dd>{player.character || "Not set"}</dd>
                  </div>
                  <div>
                    <dt>Ancestry</dt>
                    <dd>{player.ancestry || "Not set"}</dd>
                  </div>
                  <div>
                    <dt>Class</dt>
                    <dd>{player.class || "Not set"}</dd>
                  </div>
                </dl>
              </article>
            ))}
          </div>
        ) : (
          <EmptySettingsValue>No players are configured for this campaign.</EmptySettingsValue>
        )}
      </SettingsSection>

      <SettingsSection
        title="Game system"
        detail="Preset context and campaign-specific instructions."
        icon={<Settings2 size={18} aria-hidden="true" />}
        action={sectionAction("system")}
      >
        {editing === "system" ? (
          <EditableSystem system={draftSystem} presets={settings.presets} disabled={saving} onChange={updateSystem} />
        ) : <><dl className="settings-fields settings-fields--two">
          <div>
            <dt>Preset</dt>
            <dd>{settings.system.presetId}</dd>
          </div>
          <div>
            <dt>System overrides</dt>
            <dd>{settings.system.overrides ? "Configured" : "None"}</dd>
          </div>
        </dl>
        {settings.system.overrides ? (
          <pre className="settings-note-block">{settings.system.overrides}</pre>
        ) : null}
        </>}
      </SettingsSection>

      <SettingsSection
        title="Notes backend"
        detail="Effective language-model settings, including redacted credential state."
        icon={<Bot size={18} aria-hidden="true" />}
        action={sectionAction("backend")}
      >
        {editing === "backend" ? <EditableBackend backend={draftBackend} disabled={saving} onChange={updateBackend} /> : <EffectiveSettingsGrid settings={settings.backend} />}
      </SettingsSection>

      <SettingsSection
        title="Transcription"
        detail="Effective speech-to-text values and their configuration source."
        icon={<BrainCircuit size={18} aria-hidden="true" />}
        action={sectionAction("asr")}
      >
        {editing === "asr" ? <EditableAsr asr={draftAsr} models={settings.asrModels} disabled={saving} onChange={updateAsr} /> : <EffectiveSettingsGrid settings={settings.transcription.asr} />}
      </SettingsSection>

      <SettingsSection
        title="Vocabulary"
        detail={settings.transcription.vocabPrompt ? "Vocabulary prompting is enabled." : "Vocabulary prompting is disabled."}
        icon={<BrainCircuit size={18} aria-hidden="true" />}
        action={sectionAction("vocabulary")}
      >
        {editing === "vocabulary" ? (
          <EditableVocabulary
            terms={draftVocabulary}
            disabled={saving}
            onChange={updateVocabulary}
            onAdd={() => setDraftVocabulary((terms) => [...terms, ""])}
            onRemove={(index) => setDraftVocabulary((terms) => terms.filter((_, termIndex) => termIndex !== index))}
          />
        ) : settings.transcription.vocabulary.length > 0 ? (
          <TagList items={settings.transcription.vocabulary} />
        ) : (
          <EmptySettingsValue>No vocabulary terms are configured.</EmptySettingsValue>
        )}
      </SettingsSection>

      <SettingsSection title="Corrections" detail="Automatic transcript replacements." icon={<BrainCircuit size={18} aria-hidden="true" />} action={sectionAction("corrections")}>
        {editing === "corrections" ? (
          <EditableReplacements
            replacements={draftReplacements}
            disabled={saving}
            onChange={updateReplacement}
            onAdd={() => setDraftReplacements((replacements) => [...replacements, { from: "", to: "" }])}
            onRemove={(index) => setDraftReplacements((replacements) => replacements.filter((_, replacementIndex) => replacementIndex !== index))}
          />
        ) : <ReplacementList replacements={settings.transcription.replacements} />}
      </SettingsSection>

      <SettingsSection title="Speaker defaults" detail="Default labels used when identifying session speakers." icon={<BrainCircuit size={18} aria-hidden="true" />} action={sectionAction("speakers")}>
        {editing === "speakers" ? (
          <EditableSpeakers
            speakers={draftSpeakers}
            disabled={saving}
            onChange={updateSpeaker}
            onAdd={() => setDraftSpeakers((speakers) => [...speakers, { label: nextSpeakerLabel(speakers), name: "" }])}
            onRemove={(index) => setDraftSpeakers((speakers) => speakers.filter((_, speakerIndex) => speakerIndex !== index))}
          />
        ) : <SpeakerList speakers={settings.transcription.speakers} />}
      </SettingsSection>

      <SettingsSection
        title="Outputs"
        detail="Artifacts generated for each completed session."
        icon={<CircleCheck size={18} aria-hidden="true" />}
        action={sectionAction("outputs")}
      >
        {editing === "outputs" ? (
          <div className="process-dialog__artifact-grid settings-output-grid">
            {outputArtifacts.map((artifact) => (
              <label className="process-dialog__artifact" key={artifact.id}>
                <input
                  type="checkbox"
                  checked={draftOutputs.includes(artifact.id)}
                  disabled={saving}
                  onChange={() => setDraftOutputs((outputs) => outputs.includes(artifact.id) ? outputs.filter((output) => output !== artifact.id) : [...outputs, artifact.id])}
                />
                <FileOutput size={15} aria-hidden="true" />
                <span>{artifact.label}</span>
              </label>
            ))}
          </div>
        ) : settings.outputs.length > 0 ? <TagList items={settings.outputs.map(formatIdentifier)} /> : <EmptySettingsValue>No output artifacts are selected.</EmptySettingsValue>}
      </SettingsSection>

      <SettingsSection title="Prompt overrides" detail="Campaign-specific replacements for built-in artifact prompts." icon={<Bot size={18} aria-hidden="true" />} action={sectionAction("prompts")}>
        {editing === "prompts" ? (
          <EditablePrompts prompts={draftPrompts} disabled={saving} onChange={updatePrompt} />
        ) : settings.promptOverrides.length > 0 ? <TagList items={settings.promptOverrides.map(formatIdentifier)} /> : <EmptySettingsValue>All prompts use their built-in defaults.</EmptySettingsValue>}
      </SettingsSection>
    </div>
  );
}

function SettingsSection({
  title,
  detail,
  icon,
  action,
  children,
}: {
  title: string;
  detail: string;
  icon: ReactNode;
  action: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="settings-section">
      <div className="section-header settings-section__heading">
        <div>
          <h2>{title}</h2>
          <p>{detail}</p>
        </div>
        <div className="settings-section__tools">
          {action}
          <span className="settings-section__icon">{icon}</span>
        </div>
      </div>
      {children}
    </section>
  );
}

function EffectiveSettingsGrid({ settings }: { settings: EffectiveSetting[] }) {
  return (
    <dl className="settings-fields">
      {settings.map((setting) => (
        <div key={setting.id}>
          <dt>
            {setting.label}
            <span className={`settings-source settings-source--${setting.source.toLowerCase().replace(/\s+/g, "-")}`}>
              {setting.source}
            </span>
          </dt>
          <dd>{setting.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function TagList({ items }: { items: string[] }) {
  return (
    <div className="settings-tags">
      {items.map((item) => <span key={item}>{item}</span>)}
    </div>
  );
}

function ReplacementList({ replacements }: { replacements: Replacement[] }) {
  if (replacements.length === 0) {
    return <EmptySettingsValue>No transcript corrections are configured.</EmptySettingsValue>;
  }

  return (
    <dl className="settings-mapping-list">
      {replacements.map((replacement) => (
        <div key={`${replacement.from}-${replacement.to}`}>
          <dt>{replacement.from}</dt>
          <dd>{replacement.to}</dd>
        </div>
      ))}
    </dl>
  );
}

function SpeakerList({ speakers }: { speakers: SpeakerMapping[] }) {
  if (speakers.length === 0) {
    return <EmptySettingsValue>No speaker labels are mapped.</EmptySettingsValue>;
  }

  return (
    <dl className="settings-mapping-list">
      {speakers.map((speaker) => (
        <div key={`${speaker.label}-${speaker.name}`}>
          <dt>{speaker.label}</dt>
          <dd>{speaker.name}</dd>
        </div>
      ))}
    </dl>
  );
}

function EmptySettingsValue({ children }: { children: ReactNode }) {
  return <p className="settings-empty">{children}</p>;
}

function SettingsLoading() {
  return (
    <div className="settings-page settings-page--loading" aria-live="polite">
      <header className="page-header">
        <div>
          <p className="eyebrow">Reading campaign configuration</p>
          <h1>Campaign settings</h1>
        </div>
        <LoaderCircle className="is-spinning" size={22} aria-label="Loading campaign settings" />
      </header>
      <div className="settings-skeleton settings-skeleton--overview" />
      <div className="settings-skeleton-list">
        <div className="settings-skeleton" />
        <div className="settings-skeleton" />
        <div className="settings-skeleton" />
      </div>
    </div>
  );
}

function formatIdentifier(identifier: string) {
  return identifier
    .split("-")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

function pluralize(count: number, singular: string) {
  return count === 1 ? singular : `${singular}s`;
}

function nextSpeakerLabel(speakers: SpeakerMapping[]) {
  const labels = new Set(speakers.map((speaker) => speaker.label.trim()));
  let index = 0;
  while (labels.has(`SPEAKER_${String(index).padStart(2, "0")}`)) index += 1;
  return `SPEAKER_${String(index).padStart(2, "0")}`;
}

function validBackendOverrides(backend: BackendOverrides) {
  if (!backend.baseUrl) return true;
  try {
    const url = new URL(backend.baseUrl);
    return (url.protocol === "http:" || url.protocol === "https:") && !url.username && !url.password;
  } catch {
    return false;
  }
}

function validAsrOverrides(asr: AsrOverrides, modelIds: string[]) {
  return (asr.model === null || modelIds.includes(asr.model))
    && (asr.threads === null || (Number.isInteger(asr.threads) && asr.threads >= 1 && asr.threads <= 1024));
}
