import { useEffect, useState } from "react";
import { CircleAlert, FileOutput, LoaderCircle, X } from "lucide-react";
import type { ExportFormat, SessionSummary } from "../../api/types";
import "./process-dialog.css";

export type ExportDialogRequest = {
  stems: string[];
  all: boolean;
  format: ExportFormat;
  playerSafe: boolean;
};

type ExportScope = "all" | "selected";

export function ExportDialog({
  open,
  campaignName,
  sessions,
  initialStems,
  submitting,
  error,
  onClose,
  onSubmit,
}: {
  open: boolean;
  campaignName: string | null;
  sessions: SessionSummary[];
  initialStems: string[] | null;
  submitting: boolean;
  error: string | null;
  onClose: () => void;
  onSubmit: (request: ExportDialogRequest) => void;
}) {
  const [scope, setScope] = useState<ExportScope>("all");
  const [selectedStems, setSelectedStems] = useState<Set<string>>(new Set());
  const [format, setFormat] = useState<ExportFormat>("html");
  const [playerSafe, setPlayerSafe] = useState(false);

  useEffect(() => {
    if (!open) {
      return;
    }
    const exportableSessions = sessions.filter((session) => session.artifacts.length > 0);
    const availableStems = new Set(exportableSessions.map((session) => session.stem));
    const selected = initialStems?.filter((stem) => availableStems.has(stem))
      ?? exportableSessions.map((session) => session.stem);
    setScope(initialStems && selected.length > 0 ? "selected" : "all");
    setSelectedStems(new Set(selected));
    setFormat("html");
    setPlayerSafe(false);
  }, [initialStems, open, sessions]);

  if (!open) {
    return null;
  }

  const exportableSessions = sessions.filter((session) => session.artifacts.length > 0);
  const canSubmit = exportableSessions.length > 0 && (scope === "all" || selectedStems.size > 0);

  function toggleSession(stem: string) {
    setSelectedStems((current) => {
      const next = new Set(current);
      if (next.has(stem)) {
        next.delete(stem);
      } else {
        next.add(stem);
      }
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
      <section className="process-dialog export-dialog" role="dialog" aria-modal="true" aria-labelledby="export-dialog-title">
        <header className="process-dialog__header">
          <div>
            <p className="eyebrow">{campaignName ?? "Campaign"}</p>
            <h2 id="export-dialog-title">Export notes</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            onClick={onClose}
            disabled={submitting}
            title="Close export dialog"
            aria-label="Close export dialog"
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
                stems: scope === "selected" ? [...selectedStems].sort() : [],
                all: scope === "all",
                format,
                playerSafe,
              });
            }
          }}
        >
          <section className="process-dialog__section" aria-labelledby="export-format-heading">
            <div className="process-dialog__section-heading">
              <div>
                <p className="eyebrow">Format</p>
                <h3 id="export-format-heading">Export as</h3>
              </div>
            </div>
            <div className="process-dialog__mode" role="group" aria-label="Export format">
              <button
                className={format === "html" ? "process-dialog__mode-button process-dialog__mode-button--active" : "process-dialog__mode-button"}
                type="button"
                disabled={submitting}
                onClick={() => setFormat("html")}
                aria-pressed={format === "html"}
              >
                HTML
              </button>
              <button
                className={format === "obsidian" ? "process-dialog__mode-button process-dialog__mode-button--active" : "process-dialog__mode-button"}
                type="button"
                disabled={submitting}
                onClick={() => setFormat("obsidian")}
                aria-pressed={format === "obsidian"}
              >
                Obsidian
              </button>
            </div>
          </section>

          <section className="process-dialog__section" aria-labelledby="export-scope-heading">
            <div className="process-dialog__section-heading">
              <div>
                <p className="eyebrow">Scope</p>
                <h3 id="export-scope-heading">Sessions</h3>
              </div>
              <span>{scope === "all" ? "All available" : `${selectedStems.size} selected`}</span>
            </div>
            <div className="process-dialog__mode" role="group" aria-label="Export scope">
              <button
                className={scope === "all" ? "process-dialog__mode-button process-dialog__mode-button--active" : "process-dialog__mode-button"}
                type="button"
                disabled={submitting}
                onClick={() => setScope("all")}
                aria-pressed={scope === "all"}
              >
                All sessions
              </button>
              <button
                className={scope === "selected" ? "process-dialog__mode-button process-dialog__mode-button--active" : "process-dialog__mode-button"}
                type="button"
                disabled={submitting}
                onClick={() => setScope("selected")}
                aria-pressed={scope === "selected"}
              >
                Choose sessions
              </button>
            </div>

            {scope === "selected" && (
              <div className="process-dialog__audio-list export-dialog__session-list">
                {exportableSessions.map((session) => (
                  <label className="process-dialog__audio export-dialog__session" key={session.stem}>
                    <input
                      type="checkbox"
                      checked={selectedStems.has(session.stem)}
                      disabled={submitting}
                      onChange={() => toggleSession(session.stem)}
                    />
                    <FileOutput size={16} aria-hidden="true" />
                    <span>{session.stem}</span>
                    <small>{`${session.artifacts.length} note${session.artifacts.length === 1 ? "" : "s"}`}</small>
                  </label>
                ))}
              </div>
            )}
          </section>

          <section className="process-dialog__options process-dialog__options--single" aria-label="Export options">
            <label>
              <input
                type="checkbox"
                checked={playerSafe}
                disabled={submitting}
                onChange={(event) => setPlayerSafe(event.target.checked)}
              />
              <span>Exclude GM-only notes</span>
            </label>
          </section>

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
            <button className="button button--primary" type="submit" disabled={!canSubmit || submitting}>
              {submitting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <FileOutput size={16} aria-hidden="true" />}
              {submitting ? "Starting" : "Start export"}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}