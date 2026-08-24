import { useEffect, useId, useState } from "react";
import { CircleAlert, LoaderCircle, UserRoundCheck, X } from "lucide-react";
import { desktop, errorMessage } from "./desktop";
import type { SpeakerMapping, SpeakerReview } from "./types";
import "./process-dialog.css";

export function SpeakerReviewDialog({
  open,
  campaignId,
  stem,
  submitting,
  error,
  onClose,
  onSubmit,
}: {
  open: boolean;
  campaignId: string | null;
  stem: string | null;
  submitting: boolean;
  error: string | null;
  onClose: () => void;
  onSubmit: (mappings: SpeakerMapping[]) => void;
}) {
  const [review, setReview] = useState<SpeakerReview | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [names, setNames] = useState<Record<string, string>>({});
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
  const displayedError = loadError ?? error;

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
                onSubmit(mappings);
              }
            }}
          >
            <div className="speaker-review__summary">
              <span>{review.stem}</span>
              <span>{mappings.length} mapped</span>
            </div>
            <datalist id={suggestionsId}>
              {review.suggestedNames.map((name) => <option key={name} value={name} />)}
            </datalist>
            <div className="speaker-review__list">
              {review.speakers.map((speaker) => (
                <article className="speaker-review__entry" key={speaker.label}>
                  <div className="speaker-review__evidence">
                    <strong>{speaker.label}</strong>
                    {speaker.samples.slice(0, 2).map((sample, index) => (
                      <p key={`${speaker.label}-${index}`}>{sample}</p>
                    ))}
                  </div>
                  <label className="process-dialog__field speaker-review__field">
                    <span>Assigned name</span>
                    <input
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
                  </label>
                </article>
              ))}
            </div>
            <footer className="process-dialog__actions">
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