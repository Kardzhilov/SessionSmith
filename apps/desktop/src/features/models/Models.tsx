import { type ReactNode, useDeferredValue, useEffect, useRef, useState } from "react";
import {
  Bot,
  BrainCircuit,
  Check,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  Download,
  LoaderCircle,
  RefreshCw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { desktop, errorMessage } from "../../api/desktop";
import type {
  ModelAction,
  ModelDefaultKind,
  ModelEntry,
  ModelInventory,
  ModelState,
} from "../../api/types";

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
            <Search size={15} aria-hidden="true" />
            Browse catalog ({installed.length + catalog.length})
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
        <ModelCatalogModal
          actionBusy={actionBusy}
          defaultKind={defaultKind}
          emptyDescription={catalogEmptyDescription}
          models={[...installed, ...catalog]}
          onAction={onAction}
          onCancelRemoval={onCancelRemoval}
          onClose={onToggleCatalog}
          onRequestRemoval={onRequestRemoval}
          onSetDefault={onSetDefault}
          removalTarget={removalTarget}
          title={`${title} catalog`}
        />
      )}
    </section>
  );
}

type CatalogSort = "name" | "size-desc" | "size-asc" | "params-desc" | "params-asc" | "released-desc" | "released-asc";

function ModelCatalogModal({
  title,
  models,
  emptyDescription,
  defaultKind,
  actionBusy,
  removalTarget,
  onClose,
  onAction,
  onSetDefault,
  onRequestRemoval,
  onCancelRemoval,
}: {
  title: string;
  models: CategorizedModel[];
  emptyDescription: string;
  defaultKind: ModelDefaultKind;
  actionBusy: boolean;
  removalTarget: RemovalTarget | null;
  onClose: () => void;
  onAction: (action: ModelAction, modelId: string) => void;
  onSetDefault: (kind: ModelDefaultKind, modelId: string) => void;
  onRequestRemoval: (family: ManagedFamily, modelId: string) => void;
  onCancelRemoval: () => void;
}) {
  const [query, setQuery] = useState("");
  const [engine, setEngine] = useState("all");
  const [state, setState] = useState("all");
  const [language, setLanguage] = useState("all");
  const [sort, setSort] = useState<CatalogSort>("name");
  const [selectedModelId, setSelectedModelId] = useState<string | null>(models[0]?.model.id ?? null);
  const [activeModelIndex, setActiveModelIndex] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const deferredQuery = useDeferredValue(query);

  useEffect(() => {
    previousFocusRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    window.requestAnimationFrame(() => searchRef.current?.focus());
    return () => previousFocusRef.current?.focus();
  }, [onClose]);

  const normalizedQuery = deferredQuery.trim().toLocaleLowerCase();
  const filtered = models
    .filter(({ model }) => engine === "all" || model.engine === engine)
    .filter(({ model }) => state === "all" || model.state === state)
    .filter(({ model }) => language === "all"
      || model.languages?.includes(language)
      || (language !== "Multilingual" && model.languages?.includes("Multilingual")))
    .filter(({ model }) => !normalizedQuery || [
      model.id,
      model.label,
      model.family,
      model.detail,
      model.languageSummary,
      model.license,
      model.note,
      ...(model.languages ?? []),
    ].some((value) => value?.toLocaleLowerCase().includes(normalizedQuery)))
    .sort((left, right) => compareCatalogModels(left.model, right.model, sort));
  const engines = [...new Set(models.map(({ model }) => model.engine))].sort();
  const states = [...new Set(models.map(({ model }) => model.state))];
  const languages = [...new Set(models.flatMap(({ model }) => model.languages ?? []))].sort();
  const selected = filtered.find(({ model }) => model.id === selectedModelId) ?? filtered[activeModelIndex] ?? filtered[0] ?? null;

  useEffect(() => {
    setActiveModelIndex(0);
  }, [deferredQuery, engine, language, sort, state]);

  useEffect(() => {
    if (filtered.length === 0) {
      setSelectedModelId(null);
      return;
    }
    if (!filtered.some(({ model }) => model.id === selectedModelId)) {
      setSelectedModelId(filtered[0].model.id);
    }
  }, [filtered, selectedModelId]);

  const moveSelection = (index: number) => {
    const next = Math.max(0, Math.min(index, filtered.length - 1));
    setActiveModelIndex(next);
    setSelectedModelId(filtered[next]?.model.id ?? null);
    window.requestAnimationFrame(() => document.getElementById(`model-catalog-row-${next}`)?.scrollIntoView({ block: "nearest" }));
  };

  const handleDialogKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === "Tab") {
      const focusable = dialogRef.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]');
      if (!focusable?.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
      return;
    }
    if (event.target instanceof HTMLSelectElement || filtered.length === 0) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveSelection(activeModelIndex + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      moveSelection(activeModelIndex - 1);
    } else if (event.key === "Home") {
      event.preventDefault();
      moveSelection(0);
    } else if (event.key === "End") {
      event.preventDefault();
      moveSelection(filtered.length - 1);
    } else if (event.key === "Enter" && document.activeElement === searchRef.current) {
      event.preventDefault();
      setSelectedModelId(filtered[activeModelIndex]?.model.id ?? null);
    }
  };

  return (
    <div className="model-catalog-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section ref={dialogRef} className="model-catalog" role="dialog" aria-modal="true" aria-labelledby="model-catalog-title" onKeyDown={handleDialogKeyDown}>
        <header className="model-catalog__header">
          <div><p className="eyebrow">Local models</p><h2 id="model-catalog-title">{title}</h2></div>
          <button className="icon-button" type="button" onClick={onClose} title="Close catalog" aria-label="Close catalog"><X size={17} /></button>
        </header>
        <div className="model-catalog__controls">
          <label className="model-catalog__search"><Search size={15} /><span className="sr-only">Search models</span><input ref={searchRef} type="search" value={query} placeholder="Search models" onChange={(event) => setQuery(event.target.value)} /></label>
          <label><span className="sr-only">Engine</span><select value={engine} onChange={(event) => setEngine(event.target.value)}><option value="all">All engines</option>{engines.map((value) => <option key={value} value={value}>{value}</option>)}</select></label>
          <label><span className="sr-only">Install state</span><select value={state} onChange={(event) => setState(event.target.value)}><option value="all">Any state</option>{states.map((value) => <option key={value} value={value}>{modelStateLabel(value)}</option>)}</select></label>
          {languages.length > 0 && <label><span className="sr-only">Language</span><select value={language} onChange={(event) => setLanguage(event.target.value)}><option value="all">All languages</option>{languages.map((value) => <option key={value} value={value}>{value}</option>)}</select></label>}
          <label><span className="sr-only">Sort models</span><select value={sort} onChange={(event) => setSort(event.target.value as CatalogSort)}><option value="name">Name</option><option value="size-desc">Size, largest</option><option value="size-asc">Size, smallest</option><option value="params-desc">Parameters, largest</option><option value="params-asc">Parameters, smallest</option><option value="released-desc">Release date, newest</option><option value="released-asc">Release date, oldest</option></select></label>
        </div>
        <div className="model-catalog__summary">{filtered.length} of {models.length} models</div>
        <div className="model-catalog__body">
          <div className="model-list model-catalog__list" role="list" aria-label="Catalog models">
            {filtered.length > 0 ? filtered.map(({ managedFamily, model }, index) => (
              <ModelRow
                actionBusy={actionBusy}
                defaultKind={defaultKind}
                id={`model-catalog-row-${index}`}
                key={model.id}
                managedFamily={managedFamily}
                model={model}
                onSelect={() => { setActiveModelIndex(index); setSelectedModelId(model.id); }}
                removalPending={removalTarget?.family === managedFamily && removalTarget.modelId === model.id}
                selected={selected?.model.id === model.id}
                onAction={onAction}
                onCancelRemoval={onCancelRemoval}
                onRequestRemoval={(modelId) => onRequestRemoval(managedFamily, modelId)}
                onSetDefault={onSetDefault}
              />
            )) : <div className="models-empty"><CircleDashed size={20} /><p>{emptyDescription}</p></div>}
          </div>
          {selected && <ModelDetail model={selected.model} />}
        </div>
      </section>
    </div>
  );
}

function compareCatalogModels(left: ModelEntry, right: ModelEntry, sort: CatalogSort) {
  const tieBreak = left.label.localeCompare(right.label) || left.id.localeCompare(right.id);
  if (sort === "size-desc") return right.sizeBytes - left.sizeBytes || tieBreak;
  if (sort === "size-asc") return left.sizeBytes - right.sizeBytes || tieBreak;
  if (sort === "params-desc") return (right.params ?? -1) - (left.params ?? -1) || tieBreak;
  if (sort === "params-asc") return (left.params ?? Number.MAX_SAFE_INTEGER) - (right.params ?? Number.MAX_SAFE_INTEGER) || tieBreak;
  if (sort === "released-desc") return right.released.localeCompare(left.released) || tieBreak;
  if (sort === "released-asc") return left.released.localeCompare(right.released) || tieBreak;
  return tieBreak;
}

function ModelDetail({ model }: { model: ModelEntry }) {
  const stateLabel = model.state === "ready" ? "Ready" : model.state === "installed" ? "Installed" : "Available";
  return (
    <div className="model-catalog__detail" aria-live="polite">
      <p className="eyebrow">Selected model</p>
      <h3>{model.label}</h3>
      <p>{model.note ?? model.detail}</p>
      <dl>
        <div><dt>Runtime</dt><dd>{model.engine}</dd></div>
        <div><dt>State</dt><dd>{stateLabel}{model.isDefault ? " · Default" : ""}</dd></div>
        <div><dt>Size</dt><dd>{formatBytes(model.sizeBytes)}</dd></div>
        <div><dt>Parameters</dt><dd>{model.params ? formatParameters(model.params) : "Not listed"}</dd></div>
        <div><dt>Released</dt><dd>{model.released || "Not listed"}</dd></div>
        <div><dt>Languages</dt><dd>{formatLanguages(model)}</dd></div>
        <div><dt>License</dt><dd>{model.license ?? "Not listed"}</dd></div>
      </dl>
      {model.note && model.note !== model.detail && <p className="model-catalog__detail-note">{model.detail}</p>}
    </div>
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
  id,
  selected,
  onSelect,
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
  id?: string;
  selected?: boolean;
  onSelect?: () => void;
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
    <div id={id} className={`model-row model-row--${model.state}${selected ? " model-row--selected" : ""}`} role={onSelect ? "listitem" : "article"} aria-current={onSelect && selected ? "true" : undefined} onClick={onSelect}>
      <span className="model-row__status" role="img" aria-label={stateLabel}>
        <ModelStateIcon state={model.state} />
      </span>
      <div className="model-row__body">
        <div className="model-row__title">
          <h3>{model.label}</h3>
          {model.isDefault && <span className="model-row__default">Default</span>}
        </div>
        <p>{model.detail}</p>
        {(model.params || model.languages?.length || model.license) && (
          <p className="model-row__catalog-detail">
            {model.params ? formatParameters(model.params) : null}
            {model.languages?.length ? ` · ${formatLanguages(model)}` : null}
            {model.license ? ` · ${model.license}` : null}
          </p>
        )}
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
    </div>
  );
}

function formatLanguages(model: ModelEntry) {
  return model.languageSummary ?? model.languages?.join(", ") ?? "Not listed";
}

function modelStateLabel(state: ModelState) {
  return state === "ready" ? "Ready" : state === "installed" ? "Installed" : "Available";
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

function formatParameters(params: number) {
  return params >= 1_000_000_000
    ? `${(params / 1_000_000_000).toFixed(params % 1_000_000_000 === 0 ? 0 : 1)}B parameters`
    : `${Math.round(params / 1_000_000)}M parameters`;
}
