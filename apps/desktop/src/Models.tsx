import { type ReactNode, useEffect, useState } from "react";
import {
  Bot,
  BrainCircuit,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  Cpu,
  Download,
  LoaderCircle,
  RefreshCw,
  ServerCog,
  Trash2,
  X,
} from "lucide-react";
import { desktop, errorMessage } from "./desktop";
import type { ModelAction, ModelEntry, ModelInventory, ModelState } from "./types";

type ManagedFamily = "whisper" | "asr";

type RemovalTarget = {
  family: ManagedFamily;
  modelId: string;
};

export function ModelInventoryPage({
  refreshKey,
  modelRunning,
  jobError,
  onActionStarting,
  onJobStarted,
}: {
  refreshKey: number;
  modelRunning: boolean;
  jobError: string | null;
  onActionStarting: () => void;
  onJobStarted: () => Promise<void>;
}) {
  const [inventory, setInventory] = useState<ModelInventory | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [inventoryRefreshKey, setInventoryRefreshKey] = useState(0);
  const [showOllamaCatalog, setShowOllamaCatalog] = useState(false);
  const [actionSubmitting, setActionSubmitting] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [removalTarget, setRemovalTarget] = useState<RemovalTarget | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);

    void desktop
      .modelsInventory()
      .then((nextInventory) => {
        if (!cancelled) {
          setInventory(nextInventory);
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
  }, [inventoryRefreshKey, refreshKey]);

  async function submitModelAction(action: ModelAction, modelId: string) {
    setActionSubmitting(true);
    setActionError(null);
    onActionStarting();
    try {
      await desktop.jobSubmitModel({ action, modelId });
      setRemovalTarget(null);
      await onJobStarted();
    } catch (nextError) {
      setActionError(errorMessage(nextError));
    } finally {
      setActionSubmitting(false);
    }
  }

  if (!inventory && loading) {
    return <ModelsLoading />;
  }

  if (!inventory) {
    return (
      <section className="state-panel state-panel--error" aria-live="polite">
        <CircleAlert size={22} aria-hidden="true" />
        <div>
          <p className="eyebrow">Inventory unavailable</p>
          <h1>SessionSmith could not read local model state.</h1>
          <p>{error ?? "The model inventory did not return a result."}</p>
          <button className="button button--quiet" type="button" onClick={() => setInventoryRefreshKey((key) => key + 1)}>
            <RefreshCw size={16} aria-hidden="true" />
            Refresh inventory
          </button>
        </div>
      </section>
    );
  }

  const installedOllama = inventory.ollama.filter((model) => model.state === "installed");
  const catalogOllama = inventory.ollama.filter((model) => model.state !== "installed");

  return (
    <div className="models-page">
      <header className="page-header models-page__header">
        <div>
          <p className="eyebrow">Local runtime</p>
          <h1>Models</h1>
          <p className="page-subtitle">Installed speech and language models, with their configured defaults.</p>
        </div>
        <button
          className="button button--quiet"
          type="button"
          onClick={() => setInventoryRefreshKey((key) => key + 1)}
          disabled={loading}
        >
          <RefreshCw className={loading ? "is-spinning" : ""} size={16} aria-hidden="true" />
          Refresh inventory
        </button>
      </header>

      {error && (
        <p className="models-refresh-error" role="status">
          The latest refresh did not complete. Showing the previous inventory: {error}
        </p>
      )}
      {(actionError ?? jobError) && <p className="models-action-error" role="alert">{actionError ?? jobError}</p>}

      <section className={`models-service models-service--${inventory.ollamaService.reachable ? "ready" : "offline"}`}>
        <span className="models-service__icon" aria-hidden="true">
          <ServerCog size={20} />
        </span>
        <div>
          <p className="eyebrow">Ollama service</p>
          <h2>{inventory.ollamaService.reachable ? "Local server reachable" : "Local server unavailable"}</h2>
          <p>{inventory.ollamaService.detail}</p>
        </div>
        <span className="models-service__endpoint" title={inventory.ollamaService.endpoint}>
          {inventory.ollamaService.endpoint}
        </span>
      </section>

      <ModelGroup
        title="Whisper"
        detail="Offline speech-to-text models stored in the local GGML cache."
        icon={<Cpu size={18} aria-hidden="true" />}
        models={inventory.whisper}
        managedFamily="whisper"
        actionBusy={actionSubmitting || modelRunning}
        removalTarget={removalTarget}
        onAction={(action, modelId) => void submitModelAction(action, modelId)}
        onRequestRemoval={(modelId) => setRemovalTarget({ family: "whisper", modelId })}
        onCancelRemoval={() => setRemovalTarget(null)}
      />
      <ModelGroup
        title="ASR Engines"
        detail="Bridge and local engines available for high-accuracy transcription."
        icon={<BrainCircuit size={18} aria-hidden="true" />}
        models={inventory.asr}
        managedFamily="asr"
        actionBusy={actionSubmitting || modelRunning}
        removalTarget={removalTarget}
        onAction={(action, modelId) => void submitModelAction(action, modelId)}
        onRequestRemoval={(modelId) => setRemovalTarget({ family: "asr", modelId })}
        onCancelRemoval={() => setRemovalTarget(null)}
      />
      <section className="models-section" aria-labelledby="ollama-models-heading">
        <div className="section-header models-section__heading">
          <div>
            <h2 id="ollama-models-heading">Ollama <span>{installedOllama.length}</span></h2>
            <p>Language models installed in the local Ollama registry.</p>
          </div>
          <button
            className="button button--quiet button--compact"
            type="button"
            onClick={() => setShowOllamaCatalog((show) => !show)}
            aria-expanded={showOllamaCatalog}
          >
            {showOllamaCatalog ? <ChevronDown size={15} aria-hidden="true" /> : <ChevronRight size={15} aria-hidden="true" />}
            {showOllamaCatalog ? "Hide catalog" : `Browse catalog (${catalogOllama.length})`}
          </button>
        </div>
        {installedOllama.length > 0 ? (
          <div className="model-list">
            {installedOllama.map((model) => <ModelRow key={model.id} model={model} />)}
          </div>
        ) : (
          <div className="models-empty">
            <Bot size={20} aria-hidden="true" />
            <p>{inventory.ollamaService.reachable ? "No Ollama models are installed yet." : "Start Ollama to inspect local language models."}</p>
          </div>
        )}
        {showOllamaCatalog && (
          <div className="model-list model-list--catalog">
            {catalogOllama.map((model) => <ModelRow key={model.id} model={model} />)}
          </div>
        )}
      </section>
    </div>
  );
}

function ModelGroup({
  title,
  detail,
  icon,
  models,
  managedFamily,
  actionBusy,
  removalTarget,
  onAction,
  onRequestRemoval,
  onCancelRemoval,
}: {
  title: string;
  detail: string;
  icon: ReactNode;
  models: ModelEntry[];
  managedFamily: ManagedFamily;
  actionBusy: boolean;
  removalTarget: RemovalTarget | null;
  onAction: (action: ModelAction, modelId: string) => void;
  onRequestRemoval: (modelId: string) => void;
  onCancelRemoval: () => void;
}) {
  return (
    <section className="models-section">
      <div className="section-header">
        <div>
          <h2>{title} <span>{models.length}</span></h2>
          <p>{detail}</p>
        </div>
        <span className="models-section__icon">{icon}</span>
      </div>
      <div className="model-list">
        {models.map((model) => (
          <ModelRow
            actionBusy={actionBusy}
            key={model.id}
            managedFamily={managedFamily}
            model={model}
            removalPending={removalTarget?.family === managedFamily && removalTarget.modelId === model.id}
            onAction={onAction}
            onCancelRemoval={onCancelRemoval}
            onRequestRemoval={onRequestRemoval}
          />
        ))}
      </div>
    </section>
  );
}

function ModelRow({
  model,
  managedFamily,
  actionBusy,
  removalPending,
  onAction,
  onRequestRemoval,
  onCancelRemoval,
}: {
  model: ModelEntry;
  managedFamily?: ManagedFamily;
  actionBusy?: boolean;
  removalPending?: boolean;
  onAction?: (action: ModelAction, modelId: string) => void;
  onRequestRemoval?: (modelId: string) => void;
  onCancelRemoval?: () => void;
}) {
  const stateLabel = model.state === "ready" ? "Ready" : model.state === "installed" ? "Installed" : "Available";
  const isAvailable = model.state === "available";
  const installAction: ModelAction | null = managedFamily === "whisper"
    ? "downloadWhisper"
    : managedFamily === "asr"
      ? "prepareAsr"
      : null;
  const removeAction: ModelAction | null = managedFamily === "whisper"
    ? "deleteWhisper"
    : managedFamily === "asr"
      ? "deleteAsr"
      : null;
  const actionLabel = managedFamily === "whisper" ? "Download model" : "Prepare model";

  return (
    <article className={`model-row model-row--${model.state}`}>
      <span className="model-row__status" aria-label={stateLabel}>
        <ModelStateIcon state={model.state} />
      </span>
      <div className="model-row__body">
        <div className="model-row__title">
          <h3>{model.label}</h3>
          {model.isDefault && <span className="model-row__default">Default</span>}
        </div>
        <p>{model.detail}</p>
      </div>
      <dl className="model-row__meta">
        <div>
          <dt>Runtime</dt>
          <dd>{model.engine}</dd>
        </div>
        <div>
          <dt>Size</dt>
          <dd>{formatBytes(model.sizeBytes)}</dd>
        </div>
        <div>
          <dt>Released</dt>
          <dd>{model.released}</dd>
        </div>
      </dl>
      <div className="model-row__controls">
        <span className="model-row__state">{stateLabel}</span>
        {installAction && isAvailable && (
          <button
            className="icon-button model-row__action"
            type="button"
            disabled={actionBusy}
            onClick={() => onAction?.(installAction, model.id)}
            title={actionBusy ? "A local model action is already active" : actionLabel}
            aria-label={actionLabel}
          >
            {actionBusy ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Download size={16} aria-hidden="true" />}
          </button>
        )}
        {removeAction && !isAvailable && !removalPending && (
          <button
            className="icon-button model-row__action model-row__action--remove"
            type="button"
            disabled={actionBusy}
            onClick={() => onRequestRemoval?.(model.id)}
            title={actionBusy ? "A local model action is already active" : "Remove local model files"}
            aria-label="Remove local model files"
          >
            <Trash2 size={16} aria-hidden="true" />
          </button>
        )}
        {removeAction && removalPending && (
          <div className="model-row__confirmation" role="group" aria-label={`Remove ${model.label}`}>
            <span>Remove?</span>
            <button
              className="icon-button"
              type="button"
              disabled={actionBusy}
              onClick={onCancelRemoval}
              title="Keep local model files"
              aria-label="Keep local model files"
            >
              <X size={15} aria-hidden="true" />
            </button>
            <button
              className="icon-button model-row__action model-row__action--remove"
              type="button"
              disabled={actionBusy}
              onClick={() => onAction?.(removeAction, model.id)}
              title="Confirm removal of local model files"
              aria-label="Confirm removal of local model files"
            >
              <Trash2 size={15} aria-hidden="true" />
            </button>
          </div>
        )}
      </div>
    </article>
  );
}

function ModelStateIcon({ state }: { state: ModelState }) {
  if (state === "available") {
    return <CircleDashed size={19} aria-hidden="true" />;
  }
  return <CircleCheck size={19} aria-hidden="true" />;
}

function ModelsLoading() {
  return (
    <div className="models-page models-page--loading" aria-live="polite">
      <header className="page-header">
        <div>
          <p className="eyebrow">Reading local runtime</p>
          <h1>Models</h1>
        </div>
        <LoaderCircle className="is-spinning" size={22} aria-label="Loading model inventory" />
      </header>
      <div className="models-skeleton models-skeleton--service" />
      <div className="models-skeleton-list">
        <div className="models-skeleton" />
        <div className="models-skeleton" />
        <div className="models-skeleton" />
      </div>
    </div>
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024 * 1024) {
    return `${Math.max(1, Math.round(bytes / (1024 * 1024)))} MB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}
