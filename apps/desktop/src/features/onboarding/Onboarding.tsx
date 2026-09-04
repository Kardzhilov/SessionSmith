import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Bot,
  Check,
  CircleAlert,
  HeartPulse,
  LoaderCircle,
  Plus,
  Settings2,
  Trash2,
  UsersRound,
  X,
} from "lucide-react";
import { desktop, errorMessage } from "../../api/desktop";
import type {
  CampaignCreateOptions,
  CampaignCreateRequest,
  CampaignCreateResult,
  HealthReport,
  ModelInventory,
  OnboardingState,
} from "../../api/types";
import "./onboarding.css";

type SetupDestination = "health" | "models" | "settings";
type StepId = "health" | "models" | "campaign" | "backend";

const steps: Array<{ id: StepId; label: string; icon: typeof HeartPulse }> = [
  { id: "health", label: "Health", icon: HeartPulse },
  { id: "models", label: "Models", icon: Bot },
  { id: "campaign", label: "Campaign", icon: UsersRound },
  { id: "backend", label: "Backend", icon: Settings2 },
];

const emptyPlayer = { player: "", character: "", ancestry: "", class: "" };

export function OnboardingScreen({
  state,
  health,
  healthLoading,
  healthError,
  initialCampaignId,
  initialStep = "health",
  onRefreshHealth,
  onRunChecks,
  onHandoff,
  onCampaignCreated,
  onComplete,
  onClose,
}: {
  state: OnboardingState;
  health: HealthReport | null;
  healthLoading: boolean;
  healthError: string | null;
  initialCampaignId: string | null;
  initialStep?: StepId;
  onRefreshHealth: () => Promise<void>;
  onRunChecks: () => void;
  onHandoff: (destination: SetupDestination) => void;
  onCampaignCreated: (result: CampaignCreateResult) => Promise<void>;
  onComplete: (outcome: "finished" | "skipped") => Promise<void>;
  onClose?: () => void;
}) {
  const [step, setStep] = useState<StepId>(initialStep);
  const [options, setOptions] = useState<CampaignCreateOptions | null>(null);
  const [inventory, setInventory] = useState<ModelInventory | null>(null);
  const [campaignId, setCampaignId] = useState(initialCampaignId);
  const [draft, setDraft] = useState<CampaignCreateRequest>({
    name: "",
    gm: "",
    setting: "",
    notes: "",
    presetId: "generic",
    players: [{ ...emptyPlayer }],
  });
  const [loadingOptions, setLoadingOptions] = useState(true);
  const [creating, setCreating] = useState(false);
  const [completing, setCompleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmSkip, setConfirmSkip] = useState(false);
  const [healthRefreshStatus, setHealthRefreshStatus] = useState<"idle" | "refreshing" | "complete">("idle");
  const headingRef = useRef<HTMLHeadingElement | null>(null);

  useEffect(() => {
    headingRef.current?.focus();
    void Promise.all([
      desktop.campaignCreateOptions().then((result) => {
        setOptions(result);
        if (result.presets.length > 0 && !result.presets.some((preset) => preset.id === draft.presetId)) {
          setDraft((current) => ({ ...current, presetId: result.presets[0].id }));
        }
      }),
      desktop.modelsInventory().then(setInventory),
    ]).catch((nextError) => setError(errorMessage(nextError))).finally(() => setLoadingOptions(false));
  }, []);

  useEffect(() => {
    if (initialCampaignId) setCampaignId(initialCampaignId);
  }, [initialCampaignId]);

  useEffect(() => {
    const handleKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (confirmSkip) setConfirmSkip(false);
      else if (onClose) onClose();
      else setConfirmSkip(true);
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [confirmSkip, onClose]);

  const currentIndex = steps.findIndex((item) => item.id === step);
  const installedModels = inventory
    ? [...inventory.whisper, ...inventory.asr, ...inventory.ollama].filter((model) => model.state !== "available").length
    : 0;
  const failingChecks = health?.checks.filter((check) => check.state === "fail").length ?? 0;

  const createCampaign = async () => {
    setCreating(true);
    setError(null);
    try {
      const result = await desktop.campaignCreate(draft);
      setCampaignId(result.campaignId);
      await onCampaignCreated(result);
      setStep("backend");
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setCreating(false);
    }
  };

  const complete = async (outcome: "finished" | "skipped") => {
    setCompleting(true);
    setError(null);
    try {
      await onComplete(outcome);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setCompleting(false);
    }
  };

  const refreshHealth = async () => {
    setHealthRefreshStatus("refreshing");
    await onRefreshHealth();
    setHealthRefreshStatus("complete");
  };

  const updatePlayer = (index: number, field: keyof typeof emptyPlayer, value: string) => {
    setDraft((current) => ({
      ...current,
      players: current.players.map((player, playerIndex) => (
        playerIndex === index ? { ...player, [field]: value } : player
      )),
    }));
  };

  const addPlayer = () => {
    setDraft((current) => ({ ...current, players: [...current.players, { ...emptyPlayer }] }));
  };

  const removePlayer = (index: number) => {
    setDraft((current) => ({
      ...current,
      players: current.players.filter((_, playerIndex) => playerIndex !== index),
    }));
  };

  return (
    <main className="onboarding-shell" aria-labelledby="onboarding-title">
      <header className="onboarding-header">
        <div>
          <span className="eyebrow">Operational setup</span>
          <h1 id="onboarding-title" ref={headingRef} tabIndex={-1}>Set up SessionSmith</h1>
        </div>
        {onClose && (
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close setup" title="Close setup">
            <X size={17} aria-hidden="true" />
          </button>
        )}
      </header>

      <nav className="onboarding-steps" aria-label="Setup steps">
        {steps.map((item, index) => {
          const Icon = item.icon;
          const complete = item.id === "campaign" ? Boolean(campaignId) : index < currentIndex;
          return (
            <button className={item.id === step ? "onboarding-step onboarding-step--active" : "onboarding-step"} type="button" onClick={() => setStep(item.id)} aria-current={item.id === step ? "step" : undefined} key={item.id}>
              <span>{complete ? <Check size={15} aria-hidden="true" /> : <Icon size={15} aria-hidden="true" />}</span>
              {item.label}
            </button>
          );
        })}
      </nav>

      <section className="onboarding-workspace">
        {step === "health" && (
          <SetupSection title="Dependency and health status" detail={healthLoading ? "Inspecting this machine" : failingChecks > 0 ? `${failingChecks} required ${failingChecks === 1 ? "check needs" : "checks need"} attention` : "Required checks are ready"} icon={HeartPulse}>
            <div className="setup-status-list">
              {health?.checks.map((check) => <div className={`setup-status setup-status--${check.state}`} key={check.id}><span>{check.state === "ok" ? "Ready" : check.state === "warn" ? "Optional" : "Required"}</span><strong>{check.label}</strong><small>{check.detail}</small></div>)}
              {!health && <p>{healthError ?? "Health information is loading."}</p>}
            </div>
            <div className="setup-actions">
              {healthRefreshStatus === "complete" && (
                <span className={healthError ? "setup-refresh-status setup-refresh-status--error" : "setup-refresh-status"} role="status">
                  {healthError ? <CircleAlert size={15} aria-hidden="true" /> : <Check size={15} aria-hidden="true" />}
                  {healthError ? `Refresh finished with an error: ${healthError}` : "Health status refreshed just now"}
                </span>
              )}
              <button className="button button--quiet" type="button" onClick={() => void refreshHealth()} disabled={healthLoading || healthRefreshStatus === "refreshing"}>
                {healthRefreshStatus === "refreshing" && <LoaderCircle className="is-spinning" size={15} aria-hidden="true" />}
                {healthRefreshStatus === "refreshing" ? "Refreshing" : "Refresh"}
              </button>
              <button className="button button--quiet" type="button" onClick={() => onHandoff("health")}>Open Health</button>
              <button className="button button--primary" type="button" onClick={onRunChecks}>Run checks</button>
            </div>
          </SetupSection>
        )}

        {step === "models" && (
          <SetupSection title="Speech and notes models" detail={loadingOptions ? "Reading local model inventory" : `${installedModels} installed or ready`} icon={Bot}>
            <div className="setup-model-summary">
              <span><strong>{inventory?.whisper.filter((model) => model.state !== "available").length ?? 0}</strong> Whisper</span>
              <span><strong>{inventory?.asr.filter((model) => model.state !== "available").length ?? 0}</strong> ASR</span>
              <span><strong>{inventory?.ollama.filter((model) => model.state !== "available").length ?? 0}</strong> Ollama</span>
            </div>
            <div className="setup-actions"><button className="button button--primary" type="button" onClick={() => onHandoff("models")}>Select or install models</button></div>
          </SetupSection>
        )}

        {step === "campaign" && (
          <SetupSection title={campaignId ? "Campaign ready" : "Create a campaign"} detail={campaignId ? "A usable campaign configuration is available." : "Configuration and output paths are created by the desktop host."} icon={UsersRound}>
            {campaignId ? (
              <div className="setup-complete-line"><Check size={18} aria-hidden="true" /><span>Campaign ID: <strong>{campaignId}</strong></span></div>
            ) : (
              <form className="campaign-create-form" onSubmit={(event) => { event.preventDefault(); void createCampaign(); }}>
                <label>Campaign name<input autoFocus required maxLength={100} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
                <label>Game system<select value={draft.presetId} onChange={(event) => setDraft({ ...draft, presetId: event.target.value })} disabled={loadingOptions}>{options?.presets.map((preset) => <option value={preset.id} key={preset.id}>{preset.name}</option>)}</select></label>
                <label>GM<input maxLength={100} value={draft.gm} onChange={(event) => setDraft({ ...draft, gm: event.target.value })} /></label>
                <label>Setting<input maxLength={240} value={draft.setting} onChange={(event) => setDraft({ ...draft, setting: event.target.value })} /></label>
                <fieldset className="campaign-create-form__players">
                  <legend>Players and characters</legend>
                  {draft.players.map((player, index) => (
                    <div className="campaign-create-player" key={index}>
                      <div className="campaign-create-player__heading">
                        <strong>Player {index + 1}</strong>
                        {draft.players.length > 1 && (
                          <button className="icon-button" type="button" onClick={() => removePlayer(index)} title={`Remove player ${index + 1}`} aria-label={`Remove player ${index + 1}`}>
                            <Trash2 size={15} aria-hidden="true" />
                          </button>
                        )}
                      </div>
                      <label>Player<input maxLength={100} value={player.player} onChange={(event) => updatePlayer(index, "player", event.target.value)} /></label>
                      <label>Character<input maxLength={100} value={player.character} onChange={(event) => updatePlayer(index, "character", event.target.value)} /></label>
                      <label>Ancestry<input maxLength={100} value={player.ancestry} onChange={(event) => updatePlayer(index, "ancestry", event.target.value)} /></label>
                      <label>Class<input maxLength={100} value={player.class} onChange={(event) => updatePlayer(index, "class", event.target.value)} /></label>
                    </div>
                  ))}
                  <button className="button button--quiet campaign-create-form__add-player" type="button" onClick={addPlayer}>
                    <Plus size={16} aria-hidden="true" />
                    Add player
                  </button>
                </fieldset>
                <label className="campaign-create-form__notes">Campaign notes<textarea rows={3} maxLength={8000} value={draft.notes} onChange={(event) => setDraft({ ...draft, notes: event.target.value })} /></label>
                <div className="setup-actions"><button className="button button--primary" type="submit" disabled={creating || !draft.name.trim()}>{creating ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <UsersRound size={16} aria-hidden="true" />}{creating ? "Creating" : "Create campaign"}</button></div>
              </form>
            )}
          </SetupSection>
        )}

        {step === "backend" && (
          <SetupSection title="Notes backend" detail={campaignId ? "Configure backend kind, model, and endpoint in Campaign Settings." : "Create or select a campaign before configuring its backend."} icon={Settings2}>
            <div className="setup-secret-note"><CircleAlert size={17} aria-hidden="true" /><span>Secret values are never displayed in setup. API keys remain host-owned configuration.</span></div>
            <div className="setup-actions"><button className="button button--primary" type="button" disabled={!campaignId} onClick={() => onHandoff("settings")}>Configure campaign backend</button></div>
          </SetupSection>
        )}
      </section>

      {error && <div className="onboarding-error" role="alert"><CircleAlert size={16} aria-hidden="true" />{error}</div>}

      <footer className="onboarding-footer">
        <button className="button button--quiet" type="button" disabled={currentIndex === 0} onClick={() => setStep(steps[currentIndex - 1].id)}><ArrowLeft size={16} aria-hidden="true" />Back</button>
        <div className="onboarding-footer__completion">
          {confirmSkip ? (
            <div className="skip-confirm" role="alert"><span>Skip records version {state.currentVersion} as skipped. Setup remains available in App Settings.</span><button className="button button--quiet" type="button" onClick={() => setConfirmSkip(false)}>Keep setting up</button><button className="button button--primary" type="button" disabled={completing} onClick={() => void complete("skipped")}>Confirm skip</button></div>
          ) : (
            <button className="button button--quiet" type="button" onClick={() => setConfirmSkip(true)}>Skip setup</button>
          )}
          <span className="finish-semantics">Finish records setup version {state.currentVersion} as complete on this device.</span>
          <button className="button button--primary" type="button" disabled={!campaignId || completing} title={!campaignId ? "Create or select a campaign before finishing setup." : "Mark this onboarding version complete."} onClick={() => void complete("finished")}>{completing ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Check size={16} aria-hidden="true" />}Finish setup</button>
          {currentIndex < steps.length - 1 && <button className="icon-button" type="button" onClick={() => setStep(steps[currentIndex + 1].id)} aria-label="Next setup step" title="Next step"><ArrowRight size={17} aria-hidden="true" /></button>}
        </div>
      </footer>
    </main>
  );
}

function SetupSection({ title, detail, icon: Icon, children }: { title: string; detail: string; icon: typeof HeartPulse; children: React.ReactNode }) {
  return <div className="setup-section"><header><span><Icon size={20} aria-hidden="true" /></span><div><h2>{title}</h2><p>{detail}</p></div></header>{children}</div>;
}