import { useEffect, useState } from "react";
import { CircleAlert, FileOutput, LoaderCircle, Sparkles, X } from "lucide-react";
import type { ArtifactId } from "../../api/types";
import "./process-dialog.css";

const artifacts: Array<{ id: ArtifactId; label: string }> = [
  { id: "summary", label: "Summary" },
  { id: "bullets", label: "Bullets" },
  { id: "dm-notes", label: "DM notes" },
  { id: "recap", label: "Player recap" },
  { id: "story", label: "Story" },
  { id: "quotes", label: "Quotes" },
];

export function NotesDialog({
  open,
  stem,
  submitting,
  error,
  onClose,
  onSubmit,
}: {
  open: boolean;
  stem: string | null;
  submitting: boolean;
  error: string | null;
  onClose: () => void;
  onSubmit: (request: { artifactIds: ArtifactId[]; resume: boolean; force: boolean; candidate: boolean }) => void;
}) {
  const [selectedArtifacts, setSelectedArtifacts] = useState<Set<ArtifactId>>(
    new Set(artifacts.map((artifact) => artifact.id)),
  );
  const [resume, setResume] = useState(true);
  const [force, setForce] = useState(false);
  const [candidate, setCandidate] = useState(false);

  useEffect(() => {
    if (open) {
      setSelectedArtifacts(new Set(artifacts.map((artifact) => artifact.id)));
      setResume(true);
      setForce(false);
      setCandidate(false);
    }
  }, [open]);

  if (!open || !stem) {
    return null;
  }

  const selectedOutputIds = artifacts
    .filter((artifact) => selectedArtifacts.has(artifact.id))
    .map((artifact) => artifact.id);

  return (
    <div
      className="process-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !submitting) {
          onClose();
        }
      }}
    >
      <section className="process-dialog" role="dialog" aria-modal="true" aria-labelledby="notes-dialog-title">
        <header className="process-dialog__header">
          <div>
            <p className="eyebrow">Notes</p>
            <h2 id="notes-dialog-title">Generate notes</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            onClick={onClose}
            disabled={submitting}
            title="Close notes dialog"
            aria-label="Close notes dialog"
          >
            <X size={17} aria-hidden="true" />
          </button>
        </header>

        <form
          className="process-dialog__form"
          onSubmit={(event) => {
            event.preventDefault();
            if (selectedOutputIds.length > 0) {
              onSubmit({ artifactIds: selectedOutputIds, resume, force, candidate });
            }
          }}
        >
          <section className="process-dialog__section" aria-labelledby="notes-output-heading">
            <div className="process-dialog__section-heading">
              <div>
                <p className="eyebrow">Session</p>
                <h3 id="notes-output-heading">{stem}</h3>
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
                    onChange={() => {
                      setSelectedArtifacts((current) => {
                        const next = new Set(current);
                        if (next.has(artifact.id)) {
                          next.delete(artifact.id);
                        } else {
                          next.add(artifact.id);
                        }
                        return next;
                      });
                    }}
                  />
                  <FileOutput size={15} aria-hidden="true" />
                  <span>{artifact.label}</span>
                </label>
              ))}
            </div>
          </section>

          <section className="process-dialog__options" aria-label="Notes options">
            <label>
              <input
                type="checkbox"
                checked={resume}
                disabled={submitting}
                onChange={(event) => setResume(event.target.checked)}
              />
              <span>Resume completed work</span>
            </label>
            <label>
              <input
                type="checkbox"
                checked={candidate}
                disabled={submitting}
                onChange={(event) => setCandidate(event.target.checked)}
              />
              <span>Generate candidates</span>
            </label>
            <label>
              <input
                type="checkbox"
                checked={force}
                disabled={submitting}
                onChange={(event) => setForce(event.target.checked)}
              />
              <span>Regenerate selected outputs</span>
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
            <button className="button button--primary" type="submit" disabled={selectedOutputIds.length === 0 || submitting}>
              {submitting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Sparkles size={16} aria-hidden="true" />}
              {submitting ? "Starting" : "Generate notes"}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}