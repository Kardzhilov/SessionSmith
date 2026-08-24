import { useDeferredValue, useEffect, useRef, useState } from "react";
import { CircleAlert, LoaderCircle, RefreshCw, Search, X } from "lucide-react";
import { desktop, errorMessage } from "./desktop";
import type { SearchResult } from "./types";
import "./search-dialog.css";

export function SearchDialog({
  open,
  campaignId,
  campaignName,
  canReindex,
  reindexing,
  onClose,
  onReindex,
  onOpenResult,
}: {
  open: boolean;
  campaignId: string | null;
  campaignName: string | null;
  canReindex: boolean;
  reindexing: boolean;
  onClose: () => void;
  onReindex: () => void;
  onOpenResult: (result: SearchResult) => void;
}) {
  const [query, setQuery] = useState("");
  const [allCampaigns, setAllCampaigns] = useState(false);
  const [results, setResults] = useState<SearchResult[]>([]);
  const [activeResultIndex, setActiveResultIndex] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const deferredQuery = useDeferredValue(query);

  useEffect(() => {
    if (!open) {
      return;
    }
    setQuery("");
    setAllCampaigns(false);
    setResults([]);
    setActiveResultIndex(0);
    setError(null);
    window.requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  useEffect(() => {
    const term = deferredQuery.trim();
    if (!open || term.length < 2 || (!allCampaigns && !campaignId)) {
      setLoading(false);
      setResults([]);
      setError(null);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);
    void desktop
      .searchQuery(allCampaigns ? null : campaignId, term)
      .then((nextResults) => {
        if (!cancelled) {
          setResults(nextResults);
        }
      })
      .catch((nextError) => {
        if (!cancelled) {
          setResults([]);
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
  }, [allCampaigns, campaignId, deferredQuery, open]);

  useEffect(() => {
    setActiveResultIndex(0);
  }, [results]);

  if (!open) {
    return null;
  }

  const hasTerm = query.trim().length >= 2;

  return (
    <div
      className="search-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <section className="search-dialog" role="dialog" aria-modal="true" aria-labelledby="search-dialog-title">
        <header className="search-dialog__header">
          <Search size={18} aria-hidden="true" />
          <div className="search-dialog__input-wrap">
            <label className="sr-only" htmlFor="desktop-search-query">Search indexed notes</label>
            <input
              ref={inputRef}
              id="desktop-search-query"
              type="search"
              value={query}
              placeholder="Search indexed notes"
              aria-controls="desktop-search-results"
              aria-activedescendant={results.length > 0 ? `desktop-search-result-${activeResultIndex}` : undefined}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.preventDefault();
                  onClose();
                } else if (event.key === "ArrowDown" && results.length > 0) {
                  event.preventDefault();
                  setActiveResultIndex((index) => Math.min(index + 1, results.length - 1));
                } else if (event.key === "ArrowUp" && results.length > 0) {
                  event.preventDefault();
                  setActiveResultIndex((index) => Math.max(index - 1, 0));
                } else if (event.key === "Enter" && results[activeResultIndex]) {
                  event.preventDefault();
                  onOpenResult(results[activeResultIndex]);
                }
              }}
            />
          </div>
          <button className="icon-button" type="button" onClick={onClose} title="Close search" aria-label="Close search">
            <X size={17} aria-hidden="true" />
          </button>
        </header>

        <div className="search-dialog__scope">
          <span id="search-dialog-title">{allCampaigns ? "All campaigns" : campaignName ?? "Current campaign"}</span>
          <label>
            <input
              type="checkbox"
              checked={allCampaigns}
              onChange={(event) => setAllCampaigns(event.target.checked)}
            />
            <span>All campaigns</span>
          </label>
        </div>

        <div className="search-dialog__results" id="desktop-search-results" role="listbox" aria-live="polite">
          {loading ? (
            <div className="search-dialog__state">
              <LoaderCircle className="is-spinning" size={19} aria-hidden="true" />
            </div>
          ) : error ? (
            <p className="search-dialog__error" role="alert">
              <CircleAlert size={16} aria-hidden="true" />
              {error}
            </p>
          ) : hasTerm && results.length === 0 ? (
            <div className="search-dialog__state">No indexed matches</div>
          ) : (
            results.map((result, index) => (
              <button
                className={index === activeResultIndex ? "search-result search-result--active" : "search-result"}
                id={`desktop-search-result-${index}`}
                key={`${result.campaignId}-${result.stem}-${result.artifactId ?? result.artifactLabel}-${result.snippet}`}
                type="button"
                role="option"
                aria-selected={index === activeResultIndex}
                onMouseMove={() => setActiveResultIndex(index)}
                onClick={() => onOpenResult(result)}
              >
                <span className="search-result__meta">
                  {allCampaigns && <span>{result.campaignName}</span>}
                  <span>{result.stem}</span>
                  <span>{result.artifactLabel}</span>
                </span>
                <HighlightedSnippet snippet={result.snippet} />
              </button>
            ))
          )}
        </div>
        <footer className="search-dialog__footer">
          <button className="button button--quiet" type="button" onClick={onReindex} disabled={!canReindex || reindexing}>
            <RefreshCw className={reindexing ? "is-spinning" : ""} size={15} aria-hidden="true" />
            {reindexing ? "Indexing" : "Rebuild index"}
          </button>
        </footer>
      </section>
    </div>
  );
}

function HighlightedSnippet({ snippet }: { snippet: string }) {
  return (
    <span className="search-result__snippet">
      {snippet.split(/(«[^»]*»)/g).map((part, index) => (
        part.startsWith("«") && part.endsWith("»")
          ? <mark key={index}>{part.slice(1, -1)}</mark>
          : <span key={index}>{part}</span>
      ))}
    </span>
  );
}