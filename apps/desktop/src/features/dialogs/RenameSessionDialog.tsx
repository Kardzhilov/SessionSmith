import { useEffect, useState } from "react";
import { Check, LoaderCircle, Sparkles, X } from "lucide-react";
import { desktop, errorMessage } from "../../api/desktop";
import "./process-dialog.css";

export function RenameSessionDialog({
  open,
  campaignId,
  stem,
  submitting,
  error,
  onClose,
  onClearError,
  onSubmit,
}: {
  open: boolean;
  campaignId: string | null;
  stem: string | null;
  submitting: boolean;
  error: string | null;
  onClose: () => void;
  onClearError: () => void;
  onSubmit: (newStem: string) => void;
}) {
  const [name, setName] = useState("");
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [suggesting, setSuggesting] = useState(false);
  const [suggestionError, setSuggestionError] = useState<string | null>(null);

  useEffect(() => {
    if (open && stem) {
      setName(stem);
      setSuggestions([]);
      setSuggestionError(null);
    }
  }, [open, stem]);

  if (!open || !campaignId || !stem) {
    return null;
  }

  const validationError = validateSessionName(name, stem);

  async function suggestNames() {
    setSuggesting(true);
    setSuggestionError(null);
    onClearError();
    try {
      setSuggestions(await desktop.sessionNameSuggest(campaignId!, stem!));
    } catch (nextError) {
      setSuggestionError(errorMessage(nextError));
    } finally {
      setSuggesting(false);
    }
  }

  return (
    <div
      className="process-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !submitting && !suggesting) onClose();
      }}
    >
      <section className="process-dialog rename-dialog" role="dialog" aria-modal="true" aria-labelledby="rename-dialog-title">
        <header className="process-dialog__header">
          <div>
            <p className="eyebrow">Session</p>
            <h2 id="rename-dialog-title">Rename {stem}</h2>
          </div>
          <button className="icon-button" type="button" onClick={onClose} disabled={submitting || suggesting} title="Close rename dialog" aria-label="Close rename dialog">
            <X size={17} aria-hidden="true" />
          </button>
        </header>
        <form
          className="process-dialog__form"
          onSubmit={(event) => {
            event.preventDefault();
            if (!validationError) onSubmit(name.trim());
          }}
        >
          <label className="rename-dialog__field">
            <span>Session name</span>
            <input
              autoFocus
              value={name}
              disabled={submitting}
              onChange={(event) => {
                setName(event.target.value);
                onClearError();
              }}
              aria-invalid={Boolean(validationError || error)}
            />
          </label>
          <div className="rename-dialog__suggest-row">
            <button className="button button--quiet" type="button" onClick={() => void suggestNames()} disabled={suggesting || submitting}>
              {suggesting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Sparkles size={16} aria-hidden="true" />}
              {suggesting ? "Suggesting" : "Suggest names"}
            </button>
            <span>Uses the configured notes model.</span>
          </div>
          {suggestions.length > 0 && (
            <div className="rename-dialog__suggestions" aria-label="Suggested session names">
              {suggestions.map((suggestion) => (
                <button
                  className={name === suggestion ? "rename-dialog__suggestion rename-dialog__suggestion--selected" : "rename-dialog__suggestion"}
                  type="button"
                  key={suggestion}
                  onClick={() => {
                    setName(suggestion);
                    onClearError();
                  }}
                >
                  {name === suggestion && <Check size={14} aria-hidden="true" />}
                  {suggestion}
                </button>
              ))}
            </div>
          )}
          {(error || suggestionError || validationError) && (
            <p className="dialog-error" role="alert">{error ?? suggestionError ?? validationError}</p>
          )}
          <footer className="process-dialog__actions">
            <button className="button button--quiet" type="button" onClick={onClose} disabled={submitting || suggesting}>Cancel</button>
            <button className="button button--primary" type="submit" disabled={submitting || suggesting || Boolean(validationError)}>
              {submitting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <Check size={16} aria-hidden="true" />}
              {submitting ? "Renaming" : "Rename session"}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}

function validateSessionName(value: string, currentStem: string) {
  const name = value.trim();
  if (!name) return "Enter a session name.";
  if (name === currentStem) return "Enter a different session name.";
  if (name.startsWith(".")) return "Session names cannot start with a dot.";
  if (name.endsWith(".diarized")) return "Session names cannot end with .diarized.";
  if (name.includes("/") || name.includes("\\")) return "Session names cannot contain path separators.";
  if ([...name].some((character) => /[\u0000-\u001f\u007f]/.test(character))) return "Session names cannot contain control characters.";
  if (new TextEncoder().encode(name).length > 100) return "Session names cannot exceed 100 bytes.";
  return null;
}