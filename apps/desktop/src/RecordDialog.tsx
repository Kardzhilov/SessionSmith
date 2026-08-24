import { useEffect, useState } from "react";
import { CircleAlert, LoaderCircle, Mic, X } from "lucide-react";
import "./record-dialog.css";

export function RecordDialog({
  open,
  submitting,
  error,
  onClose,
  onSubmit,
}: {
  open: boolean;
  submitting: boolean;
  error: string | null;
  onClose: () => void;
  onSubmit: (name: string) => void;
}) {
  const [name, setName] = useState("session");

  useEffect(() => {
    if (open) {
      setName((currentName) => currentName || "session");
    }
  }, [open]);

  if (!open) {
    return null;
  }

  const trimmedName = name.trim();

  return (
    <div
      className="record-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !submitting) {
          onClose();
        }
      }}
    >
      <section className="record-dialog" role="dialog" aria-modal="true" aria-labelledby="record-dialog-title">
        <header className="record-dialog__header">
          <div>
            <p className="eyebrow">Audio capture</p>
            <h2 id="record-dialog-title">Start recording</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            onClick={onClose}
            disabled={submitting}
            title="Close recording dialog"
            aria-label="Close recording dialog"
          >
            <X size={17} aria-hidden="true" />
          </button>
        </header>

        <form
          className="record-dialog__form"
          onSubmit={(event) => {
            event.preventDefault();
            if (trimmedName) {
              onSubmit(trimmedName);
            }
          }}
        >
          <label className="record-dialog__field">
            <span>Session name</span>
            <input
              autoFocus
              value={name}
              onChange={(event) => setName(event.target.value)}
              disabled={submitting}
              maxLength={120}
              required
            />
          </label>
          <p className="record-dialog__detail">Uses the system default audio input and saves a WAV file to the Inbox.</p>

          {error && (
            <p className="record-dialog__error" role="alert">
              <CircleAlert size={16} aria-hidden="true" />
              {error}
            </p>
          )}

          <footer className="record-dialog__actions">
            <button className="button button--quiet" type="button" onClick={onClose} disabled={submitting}>
              Cancel
            </button>
            <button className="button button--primary" type="submit" disabled={!trimmedName || submitting}>
              {submitting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Mic size={16} aria-hidden="true" />}
              {submitting ? "Starting" : "Start recording"}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}