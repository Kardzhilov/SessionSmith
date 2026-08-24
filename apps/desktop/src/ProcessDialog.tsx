import { useEffect, useState } from "react";
import { ArrowDown, ArrowUp, AudioLines, CircleAlert, FileOutput, LoaderCircle, Play, X } from "lucide-react";
import { desktop, errorMessage } from "./desktop";
import type { ArtifactId, InboxAudio, ModelInventory } from "./types";
import "./process-dialog.css";

const artifacts: Array<{ id: ArtifactId; label: string }> = [
  { id: "summary", label: "Summary" },
  { id: "bullets", label: "Bullets" },
  { id: "dm-notes", label: "DM notes" },
  { id: "recap", label: "Player recap" },
  { id: "story", label: "Story" },
  { id: "quotes", label: "Quotes" },
];

export type ProcessDialogRequest = {
  mode: "run" | "transcribe";
  sourcePaths: string[];
  allInbox: boolean;
  artifactIds: ArtifactId[];
  resume: boolean;
  force: boolean;
  candidate: boolean;
  asrModel: string | null;
  language: string | null;
  sessionDate: string | null;
  diarize: boolean;
  vad: boolean;
  backendKind: string | null;
  llmModel: string | null;
  combine: boolean;
  sessionName: string | null;
};

export function ProcessDialog({
  open,
  audio,
  submitting,
  error,
  onClose,
  onSubmit,
}: {
  open: boolean;
  audio: InboxAudio[];
  submitting: boolean;
  error: string | null;
  onClose: () => void;
  onSubmit: (request: ProcessDialogRequest) => void;
}) {
  const [mode, setMode] = useState<ProcessDialogRequest["mode"]>("run");
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [orderedPaths, setOrderedPaths] = useState<string[]>([]);
  const [allInbox, setAllInbox] = useState(false);
  const [selectedArtifacts, setSelectedArtifacts] = useState<Set<ArtifactId>>(
    new Set(artifacts.map((artifact) => artifact.id)),
  );
  const [resume, setResume] = useState(true);
  const [force, setForce] = useState(false);
  const [candidate, setCandidate] = useState(false);
  const [asrModel, setAsrModel] = useState("");
  const [language, setLanguage] = useState("auto");
  const [sessionDate, setSessionDate] = useState("");
  const [diarize, setDiarize] = useState(false);
  const [vad, setVad] = useState(false);
  const [backendKind, setBackendKind] = useState("");
  const [llmModel, setLlmModel] = useState("");
  const [combine, setCombine] = useState(false);
  const [sessionName, setSessionName] = useState("");
  const [modelInventory, setModelInventory] = useState<ModelInventory | null>(null);
  const [modelInventoryError, setModelInventoryError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setMode("run");
      setSelectedPaths(new Set(audio.map((item) => item.path)));
      setOrderedPaths(audio.map((item) => item.path));
      setAllInbox(false);
      setSelectedArtifacts(new Set(artifacts.map((artifact) => artifact.id)));
      setResume(true);
      setForce(false);
      setCandidate(false);
      setAsrModel("");
      setLanguage("auto");
      setSessionDate("");
      setDiarize(false);
      setVad(false);
      setBackendKind("");
      setLlmModel("");
      setCombine(false);
      setSessionName("");
    }
  }, [audio, open]);

  useEffect(() => {
    if (!open) {
      return;
    }

    let cancelled = false;
    setModelInventory(null);
    setModelInventoryError(null);
    void desktop
      .modelsInventory()
      .then((inventory) => {
        if (!cancelled) {
          setModelInventory(inventory);
        }
      })
      .catch((nextError) => {
        if (!cancelled) {
          setModelInventoryError(errorMessage(nextError));
        }
      });

    return () => {
      cancelled = true;
    };
  }, [open]);

  if (!open) {
    return null;
  }

  const togglePath = (path: string) => {
    setSelectedPaths((current) => {
      const selected = current.has(path);
      setOrderedPaths((paths) => selected ? paths.filter((item) => item !== path) : [...paths, path]);
      return toggleSetValue(current, path);
    });
  };
  const toggleArtifact = (artifact: ArtifactId) => {
    setSelectedArtifacts((current) => toggleSetValue(current, artifact));
  };
  const audioByPath = new Map(audio.map((item) => [item.path, item]));
  const selectedAudio = orderedPaths
    .filter((path) => selectedPaths.has(path))
    .map((path) => audioByPath.get(path))
    .filter((item): item is InboxAudio => Boolean(item));
  const selectedOutputIds = artifacts
    .filter((artifact) => selectedArtifacts.has(artifact.id))
    .map((artifact) => artifact.id);
  const canUseAllInbox = audio.length > 0 && audio.length <= 20;
  const defaultCombinedName = selectedAudio.length > 0
    ? `${fileStem(selectedAudio[0].name)}_combined`
    : "session_combined";
  const canSubmit = selectedAudio.length > 0
    && (mode === "transcribe" || selectedOutputIds.length > 0)
    && (!combine || (selectedAudio.length >= 2 && sessionName.trim().length > 0));
  const asrModels = modelInventory ? [...modelInventory.whisper, ...modelInventory.asr] : [];

  function moveCombinedAudio(path: string, direction: -1 | 1) {
    setOrderedPaths((paths) => {
      const index = paths.indexOf(path);
      const target = index + direction;
      if (index < 0 || target < 0 || target >= paths.length) {
        return paths;
      }
      const next = [...paths];
      [next[index], next[target]] = [next[target], next[index]];
      return next;
    });
  }

  return (
    <div
      className="process-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !submitting) {
          onClose();
        }
      }}
    >
      <section className="process-dialog" role="dialog" aria-modal="true" aria-labelledby="process-dialog-title">
        <header className="process-dialog__header">
          <div>
            <p className="eyebrow">Pipeline</p>
            <h2 id="process-dialog-title">Process audio</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            onClick={onClose}
            disabled={submitting}
            title="Close processing dialog"
            aria-label="Close processing dialog"
          >
            <X size={17} aria-hidden="true" />
          </button>
        </header>

        <form
          className="process-dialog__form"
          onSubmit={(event) => {
            event.preventDefault();
            if (canSubmit) {
              onSubmit({
                mode,
                sourcePaths: allInbox ? [] : selectedAudio.map((item) => item.path),
                allInbox,
                artifactIds: mode === "run" ? selectedOutputIds : [],
                resume,
                force,
                candidate,
                asrModel: asrModel || null,
                language: language.trim() || null,
                sessionDate: sessionDate || null,
                diarize,
                vad,
                backendKind: mode === "run" && backendKind ? backendKind : null,
                llmModel: mode === "run" && llmModel.trim() ? llmModel.trim() : null,
                combine,
                sessionName: combine && sessionName.trim() ? sessionName.trim() : null,
              });
            }
          }}
        >
          <div className="process-dialog__mode" role="group" aria-label="Processing mode">
            <button
              className={mode === "run" ? "process-dialog__mode-button process-dialog__mode-button--active" : "process-dialog__mode-button"}
              type="button"
              disabled={submitting}
              onClick={() => setMode("run")}
              aria-pressed={mode === "run"}
            >
              Run pipeline
            </button>
            <button
              className={mode === "transcribe" ? "process-dialog__mode-button process-dialog__mode-button--active" : "process-dialog__mode-button"}
              type="button"
              disabled={submitting}
              onClick={() => setMode("transcribe")}
              aria-pressed={mode === "transcribe"}
            >
              Transcribe only
            </button>
          </div>

          <section className="process-dialog__section" aria-labelledby="process-inputs-heading">
            <div className="process-dialog__section-heading">
              <div>
                <p className="eyebrow">Input</p>
                <h3 id="process-inputs-heading">Inbox audio</h3>
              </div>
              <span>{selectedAudio.length} selected</span>
            </div>
            <label className="process-dialog__all-inbox">
              <input
                type="checkbox"
                checked={allInbox}
                disabled={submitting || !canUseAllInbox}
                onChange={(event) => {
                  const nextAllInbox = event.target.checked;
                  setAllInbox(nextAllInbox);
                  if (nextAllInbox) {
                    const inboxPaths = audio.map((item) => item.path);
                    setSelectedPaths(new Set(inboxPaths));
                    setOrderedPaths(inboxPaths);
                  }
                }}
              />
              <span>Use all current Inbox audio</span>
            </label>
            {!canUseAllInbox && audio.length > 20 && (
              <p className="process-dialog__all-inbox-hint">Select up to 20 files for one processing job.</p>
            )}
            <div className="process-dialog__audio-list">
              {audio.map((item) => (
                <label className="process-dialog__audio" key={item.path}>
                  <input
                    type="checkbox"
                    checked={selectedPaths.has(item.path)}
                    disabled={submitting || allInbox}
                    onChange={() => togglePath(item.path)}
                  />
                  <AudioLines size={16} aria-hidden="true" />
                  <span>{item.name}</span>
                  <small>{formatBytes(item.sizeBytes)}</small>
                </label>
              ))}
            </div>
          </section>

          {(selectedAudio.length > 1 || combine) && (
            <section className="process-dialog__combine" aria-label="Combine audio files">
              <label className="process-dialog__combine-toggle">
                <input
                  type="checkbox"
                  checked={combine}
                  disabled={submitting}
                  onChange={(event) => {
                    const nextCombine = event.target.checked;
                    setCombine(nextCombine);
                    if (nextCombine && !sessionName.trim()) {
                      setSessionName(defaultCombinedName);
                    }
                  }}
                />
                <span>Combine selected files into one session</span>
              </label>
              {combine && (
                <div className="process-dialog__combine-details">
                  <label className="process-dialog__field">
                    <span>Combined session name</span>
                    <input
                      type="text"
                      value={sessionName}
                      disabled={submitting}
                      maxLength={100}
                      spellCheck="false"
                      onChange={(event) => setSessionName(event.target.value)}
                    />
                  </label>
                  <ol className="process-dialog__combine-order" aria-label="Combined audio order">
                    {selectedAudio.map((item, index) => (
                      <li key={item.path}>
                        <span>{index + 1}. {item.name}</span>
                        <span className="process-dialog__combine-order-actions">
                          <button
                            className="icon-button"
                            type="button"
                            disabled={submitting || allInbox || index === 0}
                            onClick={() => moveCombinedAudio(item.path, -1)}
                            title={`Move ${item.name} earlier`}
                            aria-label={`Move ${item.name} earlier`}
                          >
                            <ArrowUp size={14} aria-hidden="true" />
                          </button>
                          <button
                            className="icon-button"
                            type="button"
                            disabled={submitting || allInbox || index === selectedAudio.length - 1}
                            onClick={() => moveCombinedAudio(item.path, 1)}
                            title={`Move ${item.name} later`}
                            aria-label={`Move ${item.name} later`}
                          >
                            <ArrowDown size={14} aria-hidden="true" />
                          </button>
                        </span>
                      </li>
                    ))}
                  </ol>
                </div>
              )}
            </section>
          )}

          {mode === "run" && (
            <section className="process-dialog__section" aria-labelledby="process-outputs-heading">
              <div className="process-dialog__section-heading">
                <div>
                  <p className="eyebrow">Outputs</p>
                  <h3 id="process-outputs-heading">Generate</h3>
                </div>
                <span>{selectedOutputIds.length} selected</span>
              </div>
              <div className="process-dialog__artifact-grid">
                {artifacts.map((artifact) => (
                  <label className="process-dialog__artifact" key={artifact.id}>
                    <input
                      type="checkbox"
                      checked={selectedArtifacts.has(artifact.id)}
                      disabled={submitting}
                      onChange={() => toggleArtifact(artifact.id)}
                    />
                    <FileOutput size={15} aria-hidden="true" />
                    <span>{artifact.label}</span>
                  </label>
                ))}
              </div>
            </section>
          )}

          <section className={mode === "run" ? "process-dialog__options" : "process-dialog__options process-dialog__options--single"} aria-label="Processing options">
            {mode === "run" && (
              <label>
                <input
                  type="checkbox"
                  checked={resume}
                  disabled={submitting}
                  onChange={(event) => setResume(event.target.checked)}
                />
                <span>Resume completed work</span>
              </label>
            )}
            {mode === "run" && (
              <label>
                <input
                  type="checkbox"
                  checked={candidate}
                  disabled={submitting}
                  onChange={(event) => setCandidate(event.target.checked)}
                />
                <span>Generate candidates</span>
              </label>
            )}
            <label>
              <input
                type="checkbox"
                checked={force}
                disabled={submitting}
                onChange={(event) => setForce(event.target.checked)}
              />
              <span>{mode === "run" ? "Force fresh transcription" : "Force transcription"}</span>
            </label>
          </section>

          <details className="process-dialog__advanced">
            <summary>Transcription options</summary>
            <div className="process-dialog__advanced-fields">
              <label className="process-dialog__field">
                <span>ASR model</span>
                <select value={asrModel} disabled={submitting} onChange={(event) => setAsrModel(event.target.value)}>
                  <option value="">Configured default</option>
                  {asrModels.map((model) => (
                    <option key={model.id} value={model.id}>{model.label}</option>
                  ))}
                </select>
              </label>
              <label className="process-dialog__field">
                <span>Language</span>
                <input
                  type="text"
                  value={language}
                  disabled={submitting}
                  maxLength={20}
                  placeholder="auto"
                  spellCheck="false"
                  onChange={(event) => setLanguage(event.target.value)}
                />
              </label>
              <label className="process-dialog__field">
                <span>Session date</span>
                <input
                  type="date"
                  value={sessionDate}
                  disabled={submitting}
                  onChange={(event) => setSessionDate(event.target.value)}
                />
              </label>
            </div>
            <div className="process-dialog__advanced-toggles">
              <label>
                <input
                  type="checkbox"
                  checked={diarize}
                  disabled={submitting}
                  onChange={(event) => setDiarize(event.target.checked)}
                />
                <span>Enable diarization</span>
              </label>
              <label>
                <input
                  type="checkbox"
                  checked={vad}
                  disabled={submitting}
                  onChange={(event) => setVad(event.target.checked)}
                />
                <span>Enable VAD pre-pass</span>
              </label>
            </div>
            {modelInventoryError && <p className="process-dialog__catalog-error">{modelInventoryError}</p>}
          </details>

          {mode === "run" && (
            <details className="process-dialog__advanced">
              <summary>Notes options</summary>
              <div className="process-dialog__advanced-fields process-dialog__advanced-fields--two">
                <label className="process-dialog__field">
                  <span>Backend</span>
                  <select value={backendKind} disabled={submitting} onChange={(event) => setBackendKind(event.target.value)}>
                    <option value="">Configured default</option>
                    <option value="ollama">Ollama</option>
                    <option value="openai">OpenAI</option>
                    <option value="anthropic">Anthropic</option>
                  </select>
                </label>
                <label className="process-dialog__field">
                  <span>Model</span>
                  <input
                    type="text"
                    value={llmModel}
                    disabled={submitting}
                    maxLength={256}
                    spellCheck="false"
                    onChange={(event) => setLlmModel(event.target.value)}
                  />
                </label>
              </div>
            </details>
          )}

          {error && (
            <p className="process-dialog__error" role="alert">
              <CircleAlert size={16} aria-hidden="true" />
              {error}
            </p>
          )}

          <footer className="process-dialog__actions">
            <button className="button button--quiet" type="button" onClick={onClose} disabled={submitting}>
              Cancel
            </button>
            <button
              className="button button--primary"
              type="submit"
              disabled={!canSubmit || submitting}
            >
              {submitting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Play size={16} aria-hidden="true" />}
              {submitting ? "Starting" : mode === "run" ? "Start processing" : "Start transcription"}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}

function toggleSetValue<T>(values: Set<T>, value: T) {
  const next = new Set(values);
  if (next.has(value)) {
    next.delete(value);
  } else {
    next.add(value);
  }
  return next;
}

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) {
    return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

function fileStem(name: string) {
  return name.replace(/\.[^.]+$/, "");
}