import {
  Ban,
  CircleAlert,
  CircleCheck,
  CircleDashed,
  Clock3,
  LoaderCircle,
  X,
} from "lucide-react";
import type { DesktopJob, JobState } from "./types";
import "./jobs.css";

export function JobsFlyout({
  jobs,
  open,
  error,
  onClose,
  onCancel,
}: {
  jobs: DesktopJob[];
  open: boolean;
  error: string | null;
  onClose: () => void;
  onCancel: (jobId: number) => void;
}) {
  if (!open) {
    return null;
  }

  const active = jobs.filter((job) => !isTerminal(job.state));
  const recent = jobs.filter((job) => isTerminal(job.state)).slice().reverse().slice(0, 8);

  return (
    <aside className="jobs-flyout" aria-label="Background jobs">
      <header className="jobs-flyout__header">
        <div>
          <p className="eyebrow">Background work</p>
          <h2>Jobs <span>{jobs.length}</span></h2>
        </div>
        <button
          className="icon-button"
          type="button"
          onClick={onClose}
          title="Close jobs"
          aria-label="Close jobs"
        >
          <X size={17} aria-hidden="true" />
        </button>
      </header>

      {error && <p className="jobs-flyout__error" role="status">{error}</p>}

      {active.length > 0 && (
        <section className="jobs-flyout__section" aria-labelledby="active-jobs-heading">
          <h3 id="active-jobs-heading">Active <span>{active.length}</span></h3>
          <div className="jobs-flyout__list">
            {active.map((job) => <JobRow job={job} key={job.id} onCancel={onCancel} />)}
          </div>
        </section>
      )}

      <section className="jobs-flyout__section" aria-labelledby="recent-jobs-heading">
        <h3 id="recent-jobs-heading">Recent <span>{recent.length}</span></h3>
        {recent.length > 0 ? (
          <div className="jobs-flyout__list jobs-flyout__list--recent">
            {recent.map((job) => <JobRow job={job} key={job.id} onCancel={onCancel} />)}
          </div>
        ) : (
          <div className="jobs-flyout__empty">
            <Clock3 size={18} aria-hidden="true" />
            <p>No completed jobs yet.</p>
          </div>
        )}
      </section>
    </aside>
  );
}

function JobRow({ job, onCancel }: { job: DesktopJob; onCancel: (jobId: number) => void }) {
  const progress = job.progress;
  const percent = progress && progress.total > 0
    ? Math.min(100, Math.round((progress.position / progress.total) * 100))
    : null;
  const latestLog = job.logTail.slice(-3);

  return (
    <article className={`job-row job-row--${job.state}`}>
      <span className="job-row__icon" aria-label={stateLabel(job.state)}>
        <JobStateIcon state={job.state} />
      </span>
      <div className="job-row__body">
        <div className="job-row__title">
          <h4>{job.title}</h4>
          <span>{stateLabel(job.state)}</span>
        </div>
        <p className="job-row__phase">{job.phase ?? job.summary ?? kindLabel(job.kind)}</p>
        {progress && (
          <div className="job-row__progress" aria-label={progressLabel(progress.label, percent)}>
            <div className="job-row__progress-track" aria-hidden="true">
              <i style={percent === null ? undefined : { width: `${percent}%` }} />
            </div>
            <span>{progressLabel(progress.label, percent)}</span>
          </div>
        )}
        {latestLog.length > 0 && (
          <div className="job-row__log" aria-label="Recent job output">
            {latestLog.map((line, index) => <span key={`${job.id}-${index}`}>{line}</span>)}
          </div>
        )}
      </div>
      {job.canCancel && (
        <button
          className="icon-button job-row__cancel"
          type="button"
          onClick={() => onCancel(job.id)}
          title={`Cancel ${job.title}`}
          aria-label={`Cancel ${job.title}`}
        >
          <Ban size={16} aria-hidden="true" />
        </button>
      )}
    </article>
  );
}

function JobStateIcon({ state }: { state: JobState }) {
  if (state === "succeeded") {
    return <CircleCheck size={18} aria-hidden="true" />;
  }
  if (state === "failed") {
    return <CircleAlert size={18} aria-hidden="true" />;
  }
  if (state === "cancelled" || state === "cancelling") {
    return <Ban size={17} aria-hidden="true" />;
  }
  if (state === "running") {
    return <LoaderCircle className="is-spinning" size={18} aria-hidden="true" />;
  }
  return <CircleDashed size={18} aria-hidden="true" />;
}

function isTerminal(state: JobState) {
  return state === "succeeded" || state === "failed" || state === "cancelled";
}

function stateLabel(state: JobState) {
  if (state === "succeeded") {
    return "Completed";
  }
  if (state === "failed") {
    return "Failed";
  }
  if (state === "cancelled") {
    return "Cancelled";
  }
  if (state === "cancelling") {
    return "Cancelling";
  }
  if (state === "running") {
    return "Running";
  }
  return "Queued";
}

function kindLabel(kind: DesktopJob["kind"]) {
  if (kind === "import") {
    return "Audio import";
  }
  if (kind === "doctor") {
    return "System check";
  }
  if (kind === "rebuildLog") {
    return "Campaign log";
  }
  if (kind === "reindex") {
    return "Search index";
  }
  if (kind === "export") {
    return "Campaign export";
  }
  if (kind === "candidateResolve") {
    return "Candidate resolution";
  }
  if (kind === "speakerMap") {
    return "Speaker mapping";
  }
  return kind.charAt(0).toUpperCase() + kind.slice(1);
}

function progressLabel(label: string, percent: number | null) {
  return percent === null ? label : `${label} ${percent}%`;
}