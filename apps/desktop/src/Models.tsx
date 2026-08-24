import { type ReactNode, useEffect, useState } from "react";
import {
  Bot,
  BrainCircuit,
  Check,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  Download,
  LoaderCircle,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react";
import { desktop, errorMessage } from "./desktop";
import type {
  ModelAction,
  ModelDefaultKind,
  ModelEntry,
  ModelInventory,
  ModelState,
} from "./types";

type ManagedFamily = "whisper" | "asr" | "ollama";

type CategorizedModel = {
  managedFamily: ManagedFamily;
  model: ModelEntry;
};

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
  const [showTranscriptionCatalog, setShowTranscriptionCatalog] = useState(false);
  const [showLlmCatalog, setShowLlmCatalog] = useState(false);
  const [actionSubmitting, setActionSubmitting] = useState(false);
  const [defaultSubmitting, setDefaultSubmitting] = useState(false);
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

  async function setModelDefault(kind: ModelDefaultKind, modelId: string) {
    setDefaultSubmitting(true);
    setActionError(null);
    onActionStarting();
    try {
      await desktop.modelSetDefault({ kind, modelId });
      setInventoryRefreshKey((key) => key + 1);
    } catch (nextError) {
      setActionError(errorMessage(nextError));
    } finally {
      setDefaultSubmitting(false);
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

  const transcriptionModels: CategorizedModel[] = [
    ...inventory.whisper.map((model) => ({ managedFamily: "whisper" as const, model })),
    ...inventory.asr.map((model) => ({ managedFamily: "asr" as const, model })),
  ];
  const installedTranscription = transcriptionModels.filter(({ model }) => model.state !== "available");
  const catalogTranscription = transcriptionModels.filter(({ model }) => model.state === "available");
  const llmModels = inventory.ollama.map((model) => ({ managedFamily: "ollama" as const, model }));
  const installedLlm = llmModels.filter(({ model }) => model.state === "installed");
  const catalogLlm = llmModels.filter(({ model }) => model.state !== "installed");
  const actionBusy = actionSubmitting || defaultSubmitting || modelRunning;

  return (
    <div className="models-page">
      <header className="page-header models-page__header">
        <div>
          <p className="eyebrow">Local runtime</p>
          <h1>Models</h1>
          <p className="page-subtitle">Active local transcription and language models.</p>
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

      <ModelCategory
        id="transcription-models"
        title="Transcription"
        detail="Installed speech-to-text models."
        icon={<BrainCircuit size={18} aria-hidden="true" />}
        installed={installedTranscription}
        catalog={catalogTranscription}
        showCatalog={showTranscriptionCatalog}
        onToggleCatalog={() => setShowTranscriptionCatalog((show) => !show)}
        emptyDescription="No transcription models are installed."
        catalogEmptyDescription="All curated transcription models are installed."
        defaultKind="transcription"
        actionBusy={actionBusy}
        removalTarget={removalTarget}
        onAction={(action, modelId) => void submitModelAction(action, modelId)}
        onSetDefault={(kind, modelId) => void setModelDefault(kind, modelId)}
        onRequestRemoval={(family, modelId) => setRemovalTarget({ family, modelId })}
        onCancelRemoval={() => setRemovalTarget(null)}
      />
      <ModelCategory
        id="llm-models"
        title="LLMs"
        detail={inventory.ollamaService.reachable
          ? `Installed Ollama models at ${inventory.ollamaService.endpoint}.`
          : "Ollama is unavailable."}
        icon={<Bot size={18} aria-hidden="true" />}
        installed={installedLlm}
        catalog={catalogLlm}
        showCatalog={showLlmCatalog}
        onToggleCatalog={() => setShowLlmCatalog((show) => !show)}
        emptyDescription={inventory.ollamaService.reachable
          ? "No local language models are installed."
          : "Start Ollama to inspect installed language models."}
        catalogEmptyDescription="All curated language models are installed."
        defaultKind="llm"
        notice={inventory.ollamaService.reachable ? null : inventory.ollamaService.detail}
        actionBusy={actionBusy}
        removalTarget={removalTarget}
        onAction={(action, modelId) => void submitModelAction(action, modelId)}
        onSetDefault={(kind, modelId) => void setModelDefault(kind, modelId)}
        onRequestRemoval={(family, modelId) => setRemovalTarget({ family, modelId })}
        onCancelRemoval={() => setRemovalTarget(null)}
      />
    </div>
  );
}

function ModelCategory({
  id,
  title,
  detail,
  icon,
  installed,
  catalog,
  showCatalog,
  onToggleCatalog,
  emptyDescription,
  catalogEmptyDescription,
  defaultKind,
  notice,
  actionBusy,
  removalTarget,
  onAction,
  onSetDefault,
  onRequestRemoval,
  onCancelRemoval,
}: {
  id: string;
  title: string;
  detail: string;
  icon: ReactNode;
  installed: CategorizedModel[];
  catalog: CategorizedModel[];
  showCatalog: boolean;
  onToggleCatalog: () => void;
  emptyDescription: string;
  catalogEmptyDescription: string;
  defaultKind: ModelDefaultKind;
  notice?: string | null;
  actionBusy: boolean;
  removalTarget: RemovalTarget | null;
  onAction: (action: ModelAction, modelId: string) => void;
  onSetDefault: (kind: ModelDefaultKind, modelId: string) => void;
  onRequestRemoval: (family: ManagedFamily, modelId: string) => void;
  onCancelRemoval: () => void;
}) {
  return (
    <section className="models-section" aria-labelledby={id}>
      <div className="section-header">
        <div>
          <h2 id={id}>{title} <span>{installed.length}</span></h2>
          <p>{detail}</p>
        </div>
        <div className="models-section__actions">
          <span className="models-section__icon">{icon}</span>
          <button
            className="button button--quiet button--compact"
            type="button"
            onClick={onToggleCatalog}
            aria-expanded={showCatalog}
          >
            {showCatalog ? <ChevronDown size={15} aria-hidden="true" /> : <ChevronRight size={15} aria-hidden="true" />}
            {showCatalog ? "Hide catalog" : `Browse catalog (${catalog.length})`}
          </button>
        </div>
      </div>
      {notice && <p className="models-section__notice" role="status">{notice}</p>}
      {installed.length > 0 ? (
        <div className="model-list">
          {installed.map(({ managedFamily, model }) => (
            <ModelRow
              actionBusy={actionBusy}
              defaultKind={defaultKind}
              key={model.id}
              managedFamily={managedFamily}
              model={model}
              removalPending={removalTarget?.family === managedFamily && removalTarget.modelId === model.id}
              onAction={onAction}
              onCancelRemoval={onCancelRemoval}
              onRequestRemoval={(modelId) => onRequestRemoval(managedFamily, modelId)}
              onSetDefault={onSetDefault}
            />
          ))}
        </div>
      ) : (
        <div className="models-empty">
          <CircleDashed size={20} aria-hidden="true" />
          <p>{emptyDescription}</p>
        </div>
      )}
      {showCatalog && (
        <div className="model-list model-list--catalog">
          {catalog.length > 0 ? catalog.map(({ managedFamily, model }) => (
          <ModelRow
            actionBusy={actionBusy}
            defaultKind={defaultKind}
            key={model.id}
            managedFamily={managedFamily}
            model={model}
            removalPending={removalTarget?.family === managedFamily && removalTarget.modelId === model.id}
            onAction={onAction}
            onCancelRemoval={onCancelRemoval}
            onRequestRemoval={(modelId) => onRequestRemoval(managedFamily, modelId)}
            onSetDefault={onSetDefault}
          />
          )) : (
            <div className="models-empty">
              <CircleCheck size={20} aria-hidden="true" />
              <p>{catalogEmptyDescription}</p>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function ModelRow({
  model,
  managedFamily,
  defaultKind,
  actionBusy,
  removalPending,
  onAction,
  onSetDefault,
  onRequestRemoval,
  onCancelRemoval,
}: {
  model: ModelEntry;
  managedFamily?: ManagedFamily;
  defaultKind?: ModelDefaultKind;
  actionBusy?: boolean;
  removalPending?: boolean;
  onAction?: (action: ModelAction, modelId: string) => void;
  onSetDefault?: (kind: ModelDefaultKind, modelId: string) => void;
  onRequestRemoval?: (modelId: string) => void;
  onCancelRemoval?: () => void;
}) {
  const stateLabel = model.state === "ready" ? "Ready" : model.state === "installed" ? "Installed" : "Available";
  const isAvailable = model.state === "available";
  const installAction: ModelAction | null = managedFamily === "whisper"
    ? "downloadWhisper"
    : managedFamily === "asr"
      ? "prepareAsr"
      : managedFamily === "ollama"
        ? "pullOllama"
      : null;
  const removeAction: ModelAction | null = managedFamily === "whisper"
    ? "deleteWhisper"
    : managedFamily === "asr"
      ? "deleteAsr"
      : managedFamily === "ollama"
        ? "deleteOllama"
      : null;
  const actionLabel = managedFamily === "asr" ? "Prepare model" : "Download model";
  const defaultActionLabel = defaultKind === "transcription" ? "Use for transcription" : "Use for notes";
  const removeAllowed = managedFamily !== "ollama" || model.cataloged;

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
        {defaultKind && !isAvailable && !model.isDefault && (
          <button
            className="button button--quiet button--compact model-row__default-action"
            type="button"
            disabled={actionBusy}
            onClick={() => onSetDefault?.(defaultKind, model.id)}
          >
            <Check size={15} aria-hidden="true" />
            {defaultActionLabel}
          </button>
        )}
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
        {removeAction && !isAvailable && removeAllowed && !model.isDefault && !removalPending && (
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
