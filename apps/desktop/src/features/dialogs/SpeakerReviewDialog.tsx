import { useEffect, useId, useState } from "react";
import { CircleAlert, LoaderCircle, Play, RotateCcw, UserRoundCheck, X } from "lucide-react";
import { desktop, errorMessage } from "../../api/desktop";
import type { SpeakerMapping, SpeakerReview } from "../../api/types";
import "./process-dialog.css";

export function SpeakerReviewDialog({
  open,
  campaignId,
  stem,
  submitting,
  error,
  onClose,
  onSubmit,
  onReset,
}: {
  open: boolean;
  campaignId: string | null;
  stem: string | null;
  submitting: boolean;
  error: string | null;
  onClose: () => void;
  onSubmit: (mappings: SpeakerMapping[], defaultMappings: SpeakerMapping[]) => void;
  onReset: () => void;
}) {
  const [review, setReview] = useState<SpeakerReview | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [names, setNames] = useState<Record<string, string>>({});
  const [previewing, setPreviewing] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [resetPending, setResetPending] = useState(false);
  const [saveDefaults, setSaveDefaults] = useState<Record<string, boolean>>({});
  const suggestionsId = useId();

  useEffect(() => {
    if (!open || !campaignId || !stem) {
      return;
    }

    let cancelled = false;
    setLoading(true);
    setLoadError(null);
    setReview(null);
    setNames({});
    setPreviewError(null);
    setResetPending(false);
    setSaveDefaults({});
    void desktop
      .speakerReview(campaignId, stem)
      .then((nextReview) => {
        if (cancelled) {
          return;
        }
        setReview(nextReview);
        setNames(
          Object.fromEntries(
            nextReview.speakers.map((speaker) => [speaker.label, speaker.mappedTo ?? ""]),
          ),
        );
      })
      .catch((nextError) => {
        if (!cancelled) {
          setLoadError(errorMessage(nextError));
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
  }, [campaignId, open, stem]);

  if (!open || !campaignId || !stem) {
    return null;
  }

  const mappings = review?.speakers.flatMap((speaker) => {
    const name = names[speaker.label]?.trim();
    return name ? [{ label: speaker.label, name }] : [];
  }) ?? [];
  const defaultMappings = mappings.filter((mapping) => saveDefaults[mapping.label]);
  const displayedError = loadError ?? error;

  async function previewSample(key: string, startMs: number) {
    setPreviewing(key);
    setPreviewError(null);
    try {
      const loaded = await desktop.audioLoad(campaignId!, stem!);
      if (loaded.sourceId === null) {
        throw new Error("The session audio source could not be loaded.");
      }
      await desktop.audioSeek(loaded.sourceId, startMs);
      await desktop.audioPlay(loaded.sourceId);
    } catch (nextError) {
      setPreviewError(errorMessage(nextError));
    } finally {
      setPreviewing(null);
    }
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
      <section className="process-dialog speaker-review-dialog" role="dialog" aria-modal="true" aria-labelledby="speaker-review-title">
        <header className="process-dialog__header">
          <div>
            <p className="eyebrow">Transcript</p>
            <h2 id="speaker-review-title">Review speakers</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            onClick={onClose}
            disabled={submitting}
            title="Close speaker review"
            aria-label="Close speaker review"
          >
            <X size={17} aria-hidden="true" />
          </button>
        </header>

        {loading ? (
          <div className="speaker-review__state" aria-live="polite">
            <LoaderCircle className="is-spinning" size={20} aria-hidden="true" />
          </div>
        ) : displayedError ? (
          <p className="process-dialog__error speaker-review__error" role="alert">
            <CircleAlert size={16} aria-hidden="true" />
            {displayedError}
          </p>
        ) : review?.speakers.length === 0 ? (
          <div className="speaker-review__state">
            <p>No diarized speaker labels were found for {stem}.</p>
          </div>
        ) : review ? (
          <form
            className="process-dialog__form"
            onSubmit={(event) => {
              event.preventDefault();
              if (mappings.length > 0) {
                onSubmit(mappings, defaultMappings);
              }
            }}
          >
            <div className="speaker-review__summary">
              <div><strong>{review.stem}</strong><p>Names replace raw speaker labels in the transcript and improve generated notes.</p></div>
              <span>{mappings.length} mapped</span>
            </div>
            {previewError && <p className="process-dialog__error" role="alert">{previewError}</p>}
            <datalist id={suggestionsId}>
              {review.suggestedNames.map((name) => <option key={name} value={name} />)}
            </datalist>
            <div className="speaker-review__list">
              {review.speakers.map((speaker, index) => (
                <article className="speaker-review__entry" key={speaker.label}>
                  <div className="speaker-review__evidence">
                    <strong>{speaker.label}</strong>
                    {speaker.samples.slice(0, 2).map((sample, index) => {
                      const key = `${speaker.label}-${index}`;
                      return (
                        <div className="speaker-review__sample" key={key}>
                          {sample.startMs !== null ? (
                            <button className="icon-button" type="button" disabled={previewing !== null || submitting} onClick={() => void previewSample(key, sample.startMs!)} title={`Play from ${formatTimestamp(sample.startMs)}`} aria-label={`Play ${speaker.label} sample from ${formatTimestamp(sample.startMs)}`}>
                              {previewing === key ? <LoaderCircle className="is-spinning" size={14} /> : <Play size={14} />}
                            </button>
                          ) : null}
                          <p><time>{sample.startMs !== null ? formatTimestamp(sample.startMs) : "Untimed"}</time>{sample.text}</p>
                        </div>
                      );
                    })}
                  </div>
                  <div className="process-dialog__field speaker-review__field">
                    <label htmlFor={`${suggestionsId}-speaker-${index}`}>Assigned name</label>
                    <input
                      id={`${suggestionsId}-speaker-${index}`}
                      type="text"
                      list={suggestionsId}
                      value={names[speaker.label] ?? ""}
                      maxLength={100}
                      disabled={submitting}
                      onChange={(event) => setNames((current) => ({
                        ...current,
                        [speaker.label]: event.target.value,
                      }))}
                    />
                    <label className="speaker-review__default-toggle">
                      <input
                        type="checkbox"
                        checked={Boolean(saveDefaults[speaker.label])}
                        disabled={submitting || !names[speaker.label]?.trim()}
                        onChange={(event) => setSaveDefaults((current) => ({ ...current, [speaker.label]: event.target.checked }))}
                      />
                      Save as campaign default
                    </label>
                  </div>
                </article>
              ))}
            </div>
            <footer className="process-dialog__actions">
              {review.canReset && (resetPending ? (
                <div className="speaker-review__reset-confirm" role="group" aria-label="Confirm reset to raw speaker labels">
                  <span>Restore raw labels?</span>
                  <button className="button button--quiet" type="button" onClick={() => setResetPending(false)} disabled={submitting}>Keep map</button>
                  <button className="button button--danger" type="button" onClick={onReset} disabled={submitting}><RotateCcw size={15} /> Reset</button>
                </div>
              ) : (
                <button className="button button--quiet" type="button" onClick={() => setResetPending(true)} disabled={submitting} title="Restore SPEAKER_xx labels from the preserved raw transcript">
                  <RotateCcw size={15} /> Reset to raw labels
                </button>
              ))}
              <button className="button button--quiet" type="button" onClick={onClose} disabled={submitting}>
                Cancel
              </button>
              <button className="button button--primary" type="submit" disabled={mappings.length === 0 || submitting}>
                {submitting ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <UserRoundCheck size={16} aria-hidden="true" />}
                {submitting ? "Saving" : "Save map"}
              </button>
            </footer>
          </form>
        ) : null}
      </section>
    </div>
  );
}

function formatTimestamp(positionMs: number) {
  const totalSeconds = Math.floor(positionMs / 1_000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}