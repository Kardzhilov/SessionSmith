import { type ReactNode, useEffect, useState } from "react";
import {
  Bot,
  BrainCircuit,
  CircleAlert,
  CircleCheck,
  LoaderCircle,
  Pencil,
  RefreshCw,
  Save,
  Settings2,
  X,
} from "lucide-react";
import {
  clearEditableSettingsDraft,
  EditablePlayers,
  EditableReplacements,
  EditableVocabulary,
  emptyPlayer,
  persistEditableSettingsDraft,
  readEditableSettingsDraft,
  sameEditableSettings,
} from "./CampaignSettingsEditor";
import { desktop, errorMessage } from "./desktop";
import type {
  CampaignSettings,
  CampaignPlayer,
  CampaignSummary,
  EffectiveSetting,
  Replacement,
  SpeakerMapping,
} from "./types";

export function CampaignSettingsPage({ campaign }: { campaign: CampaignSummary | undefined }) {
  const [settings, setSettings] = useState<CampaignSettings | null>(null);
  const [loading, setLoading] = useState(Boolean(campaign));
  const [error, setError] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [editing, setEditing] = useState(false);
  const [draftPlayers, setDraftPlayers] = useState<CampaignPlayer[]>([]);
  const [draftVocabulary, setDraftVocabulary] = useState<string[]>([]);
  const [draftReplacements, setDraftReplacements] = useState<Replacement[]>([]);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const campaignId = campaign?.id;

  useEffect(() => {
    if (!campaignId) {
      setSettings(null);
      setError(null);
      setLoading(false);
      setEditing(false);
      setSaveError(null);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);
    setEditing(false);
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

  const draftDirty = settings !== null && !sameEditableSettings(
    settings,
    draftPlayers,
    draftVocabulary,
    draftReplacements,
  );
  const canSave = editing
    && draftDirty
    && !saving
    && draftPlayers.every((player) => player.player.trim() && player.character.trim())
    && draftVocabulary.every((term) => term.trim())
    && draftReplacements.every((replacement) => replacement.from.trim() && replacement.to.trim());

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
    });
  }, [campaignId, draftDirty, draftPlayers, draftReplacements, draftVocabulary, editing, settings]);

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

  function beginEditing() {
    if (!settings || !campaignId) {
      return;
    }
    const recovered = readEditableSettingsDraft(campaignId, settings.revision);
    setDraftPlayers(recovered?.players ?? settings.players);
    setDraftVocabulary(recovered?.vocabulary ?? settings.transcription.vocabulary);
    setDraftReplacements(recovered?.replacements ?? settings.transcription.replacements);
    setSaveError(null);
    setEditing(true);
  }

  function discardEditing() {
    if (campaignId) {
      clearEditableSettingsDraft(campaignId);
    }
    setDraftPlayers(settings?.players ?? []);
    setDraftVocabulary(settings?.transcription.vocabulary ?? []);
    setDraftReplacements(settings?.transcription.replacements ?? []);
    setSaveError(null);
    setEditing(false);
  }

  async function saveEditableSettings() {
    if (!settings || !campaignId || !canSave) {
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
        expectedRevision: settings.revision,
      });
      clearEditableSettingsDraft(campaignId);
      setSettings(nextSettings);
      setDraftPlayers(nextSettings.players);
      setDraftVocabulary(nextSettings.transcription.vocabulary);
      setDraftReplacements(nextSettings.transcription.replacements);
      setEditing(false);
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
          {editing ? (
            <>
              <button className="button button--quiet" type="button" onClick={discardEditing} disabled={saving}>
                <X size={16} aria-hidden="true" />
                Discard
              </button>
              <button className="button button--primary" type="button" onClick={() => void saveEditableSettings()} disabled={!canSave}>
                {saving ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Save size={16} aria-hidden="true" />}
                {saving ? "Saving" : "Save changes"}
              </button>
            </>
          ) : (
            <button className="button button--quiet" type="button" onClick={beginEditing}>
              <Pencil size={16} aria-hidden="true" />
              Edit roster & vocabulary
            </button>
          )}
          <button
            className="button button--quiet"
            type="button"
            onClick={() => setRefreshKey((key) => key + 1)}
            disabled={loading || saving || editing}
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
            <h2 id="campaign-identity-heading">{settings.campaign.name}</h2>
            {settings.campaign.notes ? (
              <p className="settings-identity__notes">{settings.campaign.notes}</p>
            ) : (
              <p className="settings-identity__notes settings-identity__notes--empty">No campaign notes.</p>
            )}
          </div>
        </div>
        <dl className="settings-metrics">
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
        </dl>
      </section>

      <SettingsSection
        title="Players"
        detail={`${settings.players.length} ${pluralize(settings.players.length, "player")} in the campaign context.`}
        icon={<Settings2 size={18} aria-hidden="true" />}
      >
        {editing ? (
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
      >
        <dl className="settings-fields settings-fields--two">
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
      </SettingsSection>

      <SettingsSection
        title="Notes backend"
        detail="Effective language-model settings, including redacted credential state."
        icon={<Bot size={18} aria-hidden="true" />}
      >
        <EffectiveSettingsGrid settings={settings.backend} />
      </SettingsSection>

      <SettingsSection
        title="Transcription"
        detail="Effective speech-to-text values and their configuration source."
        icon={<BrainCircuit size={18} aria-hidden="true" />}
      >
        <EffectiveSettingsGrid settings={settings.transcription.asr} />
      </SettingsSection>

      <SettingsSection
        title="Vocabulary and corrections"
        detail={settings.transcription.vocabPrompt ? "Vocabulary prompting is enabled." : "Vocabulary prompting is disabled."}
        icon={<BrainCircuit size={18} aria-hidden="true" />}
      >
        {editing ? (
          <div className="settings-edit-grid">
            <SettingsTextGroup title="Vocabulary">
              <EditableVocabulary
                terms={draftVocabulary}
                disabled={saving}
                onChange={updateVocabulary}
                onAdd={() => setDraftVocabulary((terms) => [...terms, ""])}
                onRemove={(index) => setDraftVocabulary((terms) => terms.filter((_, termIndex) => termIndex !== index))}
              />
            </SettingsTextGroup>
            <SettingsTextGroup title="Corrections">
              <EditableReplacements
                replacements={draftReplacements}
                disabled={saving}
                onChange={updateReplacement}
                onAdd={() => setDraftReplacements((replacements) => [...replacements, { from: "", to: "" }])}
                onRemove={(index) => setDraftReplacements((replacements) => replacements.filter((_, replacementIndex) => replacementIndex !== index))}
              />
            </SettingsTextGroup>
            <SettingsTextGroup title="Speaker labels">
              <SpeakerList speakers={settings.transcription.speakers} />
            </SettingsTextGroup>
          </div>
        ) : (
          <div className="settings-text-grid">
            <SettingsTextGroup title="Vocabulary">
              {settings.transcription.vocabulary.length > 0 ? (
                <TagList items={settings.transcription.vocabulary} />
              ) : (
                <EmptySettingsValue>No vocabulary terms are configured.</EmptySettingsValue>
              )}
            </SettingsTextGroup>
            <SettingsTextGroup title="Corrections">
              <ReplacementList replacements={settings.transcription.replacements} />
            </SettingsTextGroup>
            <SettingsTextGroup title="Speaker labels">
              <SpeakerList speakers={settings.transcription.speakers} />
            </SettingsTextGroup>
          </div>
        )}
      </SettingsSection>

      <SettingsSection
        title="Outputs and prompts"
        detail="Artifacts generated for each completed session."
        icon={<CircleCheck size={18} aria-hidden="true" />}
      >
        <div className="settings-text-grid settings-text-grid--two">
          <SettingsTextGroup title="Outputs">
            {settings.outputs.length > 0 ? (
              <TagList items={settings.outputs.map(formatIdentifier)} />
            ) : (
              <EmptySettingsValue>No output artifacts are selected.</EmptySettingsValue>
            )}
          </SettingsTextGroup>
          <SettingsTextGroup title="Prompt overrides">
            {settings.promptOverrides.length > 0 ? (
              <TagList items={settings.promptOverrides.map(formatIdentifier)} />
            ) : (
              <EmptySettingsValue>All prompts use their preset defaults.</EmptySettingsValue>
            )}
          </SettingsTextGroup>
        </div>
      </SettingsSection>
    </div>
  );
}

function SettingsSection({
  title,
  detail,
  icon,
  children,
}: {
  title: string;
  detail: string;
  icon: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="settings-section">
      <div className="section-header settings-section__heading">
        <div>
          <h2>{title}</h2>
          <p>{detail}</p>
        </div>
        <span className="settings-section__icon">{icon}</span>
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

function SettingsTextGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="settings-text-group">
      <h3>{title}</h3>
      {children}
    </div>
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
