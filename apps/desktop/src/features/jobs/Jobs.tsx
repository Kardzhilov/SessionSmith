import { useEffect, useState } from "react";
import {
  Activity,
  Ban,
  CheckCircle2,
  CircleAlert,
  CircleDashed,
  Clock3,
  Ear,
  Gauge,
  Layers3,
  LoaderCircle,
  RefreshCw,
  ScrollText,
  Timer,
  Trash2,
  Zap,
} from "lucide-react";
import type { DesktopJob, JobState } from "../../api/types";
import "./jobs.css";

export function JobsPage({
  jobs,
  error,
  onCancel,
  onClearHistory,
  onRefresh,
}: {
  jobs: DesktopJob[];
  error: string | null;
  onCancel: (jobId: number) => void;
  onClearHistory: () => Promise<void>;
  onRefresh: () => void;
}) {
  const active = jobs.filter((job) => !isTerminal(job.state));
  const recent = jobs.filter((job) => isTerminal(job.state)).slice().reverse();
  const succeeded = recent.filter((job) => job.state === "succeeded").length;
  const failed = recent.filter((job) => job.state === "failed").length;
  const [now, setNow] = useState(() => epochSeconds());
  const [confirmClear, setConfirmClear] = useState(false);
  const [clearing, setClearing] = useState(false);

  useEffect(() => {
    if (active.length === 0) return;
    const timer = window.setInterval(() => setNow(epochSeconds()), 1_000);
    return () => window.clearInterval(timer);
  }, [active.length]);

  async function clearHistory() {
    setClearing(true);
    try {
      await onClearHistory();
      setConfirmClear(false);
    } catch {
      // The parent reports command errors in the page status region.
    } finally {
      setClearing(false);
    }
  }

  return (
    <div className="jobs-page">
      <header className="page-header jobs-page__header">
        <div>
          <p className="eyebrow">Operations</p>
          <h1>Jobs</h1>
          <p className="page-subtitle">Live pipeline activity and durable history with timing and output.</p>
        </div>
        <button className="button button--quiet" type="button" onClick={onRefresh}>
          <RefreshCw size={16} aria-hidden="true" />Refresh
        </button>
      </header>

      {error && <p className="jobs-page__error" role="status"><CircleAlert size={17} aria-hidden="true" />{error}</p>}

      <section className="jobs-overview" aria-label="Job overview">
        <JobMetric icon={Activity} label="Active" value={active.length} tone={active.length > 0 ? "live" : "neutral"} />
        <JobMetric icon={CircleDashed} label="Queued" value={active.filter((job) => job.state === "queued").length} tone="neutral" />
        <JobMetric icon={CheckCircle2} label="Completed" value={succeeded} tone="success" />
        <JobMetric icon={CircleAlert} label="Needs attention" value={failed} tone={failed > 0 ? "danger" : "neutral"} />
      </section>

      <section className="jobs-live" aria-labelledby="live-jobs-heading">
        <div className="jobs-section-heading">
          <div><p className="eyebrow">Right now</p><h2 id="live-jobs-heading">Active work</h2></div>
          {active.length > 0 && <span className="jobs-live__signal"><i aria-hidden="true" />Receiving updates</span>}
        </div>
        {active.length > 0 ? (
          <div className="jobs-active-list" aria-live="polite">
            {active.map((job) => <ActiveJob key={job.id} job={job} now={now} onCancel={onCancel} />)}
          </div>
        ) : (
          <div className="jobs-idle">
            <div className="jobs-idle__visual" aria-hidden="true"><i /><Activity size={26} /></div>
            <div><strong>All quiet</strong><span>New processing, recording, export, and model jobs will appear here.</span></div>
          </div>
        )}
      </section>

      <section className="jobs-history" aria-labelledby="job-history-heading">
        <div className="jobs-section-heading">
          <div><p className="eyebrow">History</p><h2 id="job-history-heading">Job history</h2></div>
          <div className="jobs-history__actions">
            <span>{recent.length} recorded</span>
            {recent.length > 0 && !confirmClear && (
              <button className="button button--quiet button--compact" type="button" onClick={() => setConfirmClear(true)}>
                <Trash2 size={14} aria-hidden="true" />Clear history
              </button>
            )}
            {recent.length > 0 && confirmClear && (
              <div className="jobs-history__confirm" role="group" aria-label="Confirm clear job history">
                <span>Clear all saved job history?</span>
                <button className="button button--quiet button--compact" type="button" disabled={clearing} onClick={() => setConfirmClear(false)}>Keep history</button>
                <button className="button button--compact jobs-history__clear" type="button" disabled={clearing} onClick={() => void clearHistory()}>
                  {clearing ? <LoaderCircle className="is-spinning" size={14} aria-hidden="true" /> : <Trash2 size={14} aria-hidden="true" />}Clear all
                </button>
              </div>
            )}
          </div>
        </div>
        {recent.length > 0 ? (
          <div className="jobs-history__list">{recent.map((job) => <HistoryJob key={job.id} job={job} />)}</div>
        ) : (
          <div className="jobs-history__empty"><Clock3 size={18} aria-hidden="true" />No job history yet.</div>
        )}
      </section>
    </div>
  );
}

function JobMetric({ icon: Icon, label, value, tone }: { icon: typeof Activity; label: string; value: number; tone: "live" | "success" | "danger" | "neutral" }) {
  return <div className={`job-metric job-metric--${tone}`}><Icon size={18} aria-hidden="true" /><span>{label}</span><strong>{value}</strong></div>;
}

function ActiveJob({ job, now, onCancel }: { job: DesktopJob; now: number; onCancel: (jobId: number) => void }) {
  const percent = jobPercent(job);
  const logs = job.logTail.slice(-12);
  return (
    <article className={`active-job active-job--${job.state}`}>
      <JobActivityVisual job={job} />
      <div className="active-job__content">
        <header className="active-job__header">
          <div><span className="active-job__kind">{kindLabel(job.kind)} · Job {job.id}</span><h3>{job.title}</h3><p>{job.phase ?? job.summary ?? stateLabel(job.state)}</p></div>
          <span className={`job-state job-state--${job.state}`}><i aria-hidden="true" />{stateLabel(job.state)}</span>
        </header>
        <div className="active-job__progress">
          <div className="active-job__progress-copy">
            <strong>{job.progress?.label ?? (job.state === "queued" ? "Waiting for an available worker" : "Working")}</strong>
            <span>{progressDetail(job, percent)}</span>
          </div>
          {percent !== null ? (
            <div className="active-job__track" role="progressbar" aria-label={job.progress?.label ?? "Job progress"} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
              <i style={{ width: `${percent}%` }}><span /></i>
            </div>
          ) : <div className="active-job__track active-job__track--indeterminate" role="progressbar" aria-label={job.progress?.label ?? "Job in progress"}><i /></div>}
        </div>
        <dl className="active-job__stats">
          <JobDatum icon={Timer} label="Elapsed" value={durationBetween(job.startedAt, now)} />
          <JobDatum icon={Clock3} label="Estimated left" value={estimatedRemaining(job) ?? "Calculating"} />
          <JobDatum icon={Gauge} label="Rate" value={formatRate(job)} />
          <JobDatum icon={Layers3} label="Active tasks" value={String(job.activeChildren)} />
        </dl>
        {logs.length > 0 && (
          <div className="active-job__log">
            <div><ScrollText size={15} aria-hidden="true" /><strong>Live output</strong><span>Latest {logs.length} updates</span></div>
            <ol>{logs.map((line, index) => <li key={`${job.id}-${job.logTail.length - logs.length + index}`}><i aria-hidden="true" />{line}</li>)}</ol>
          </div>
        )}
      </div>
      {job.canCancel && <button className="button button--quiet active-job__cancel" type="button" onClick={() => onCancel(job.id)}><Ban size={16} aria-hidden="true" />Cancel job</button>}
    </article>
  );
}

function JobActivityVisual({ job }: { job: DesktopJob }) {
  const activity = jobActivity(job);
  if (activity === "transcribing") {
    return (
      <div className="active-job__motion active-job__motion--transcribing" aria-label="Audio waves entering an ear">
        <span className="active-job__waves" aria-hidden="true">
          <svg viewBox="0 0 48 30" preserveAspectRatio="none">
            <g className="active-job__wave-shape">
              <path className="active-job__wave-glow" d="M-48 15Q-44 3-40 15T-32 15T-24 15T-16 15T-8 15T0 15T8 15T16 15T24 15T32 15T40 15T48 15T56 15T64 15T72 15T80 15T88 15T96 15" />
              <path d="M-48 15Q-44 3-40 15T-32 15T-24 15T-16 15T-8 15T0 15T8 15T16 15T24 15T32 15T40 15T48 15T56 15T64 15T72 15T80 15T88 15T96 15" />
            </g>
          </svg>
        </span>
        <Ear className="active-job__ear" size={34} aria-hidden="true" />
      </div>
    );
  }
  if (activity === "writing") {
    return (
      <div className="active-job__motion active-job__motion--writing" aria-label="A hand writing a document">
        <span className="active-job__paper" aria-hidden="true"><i /><i /><i /></span>
        <span className="active-job__writing-hand" aria-hidden="true">✍︎</span>
      </div>
    );
  }
  return (
    <div className="active-job__motion" aria-hidden="true">
      <i className="active-job__ring active-job__ring--outer" /><i className="active-job__ring active-job__ring--inner" />
      <span className="active-job__state-icon"><JobStateIcon state={job.state} /></span>
    </div>
  );
}

function JobDatum({ icon: Icon, label, value }: { icon: typeof Timer; label: string; value: string }) {
  return <div><Icon size={15} aria-hidden="true" /><dt>{label}</dt><dd>{value}</dd></div>;
}

function HistoryJob({ job }: { job: DesktopJob }) {
  const elapsed = durationBetween(job.startedAt, job.finishedAt);
  return (
    <details className={`history-job history-job--${job.state}`}>
      <summary>
        <span className="history-job__icon"><JobStateIcon state={job.state} /></span>
        <span className="history-job__identity"><strong>{job.title}</strong><small>{kindLabel(job.kind)} · {job.summary ?? job.phase ?? stateLabel(job.state)}</small></span>
        <span className="history-job__time">{elapsed}</span><span className={`job-state job-state--${job.state}`}>{stateLabel(job.state)}</span>
      </summary>
      <div className="history-job__details">
        <dl><div><dt>Started</dt><dd>{formatDateTime(job.startedAt)}</dd></div><div><dt>Finished</dt><dd>{formatDateTime(job.finishedAt)}</dd></div><div><dt>Duration</dt><dd>{elapsed}</dd></div><div><dt>Job ID</dt><dd>{job.id}</dd></div></dl>
        {job.summary && <p>{job.summary}</p>}
        {job.logTail.length > 0 && <pre aria-label="Job output">{job.logTail.join("\n")}</pre>}
      </div>
    </details>
  );
}

function JobStateIcon({ state }: { state: JobState }) {
  if (state === "succeeded") return <CheckCircle2 size={18} aria-hidden="true" />;
  if (state === "failed") return <CircleAlert size={18} aria-hidden="true" />;
  if (state === "cancelled" || state === "cancelling") return <Ban size={18} aria-hidden="true" />;
  if (state === "running") return <Zap size={19} aria-hidden="true" />;
  return <LoaderCircle size={18} aria-hidden="true" />;
}

function jobPercent(job: DesktopJob) {
  if (!job.progress || job.progress.total <= 0) return null;
  return Math.max(0, Math.min(100, Math.round((job.progress.position / job.progress.total) * 100)));
}

function progressDetail(job: DesktopJob, percent: number | null) {
  if (!job.progress) return job.state === "queued" ? "Queued" : "Progress will appear when this phase reports measurable work";
  if (job.progress.total <= 0) return `${formatNumber(job.progress.position)} completed`;
  return `${formatNumber(job.progress.position)} of ${formatNumber(job.progress.total)} · ${percent}%`;
}

function estimatedRemaining(job: DesktopJob) {
  const progress = job.progress;
  if (!progress || progress.total <= 0 || !progress.rate || progress.rate <= 0) return null;
  return formatDuration(Math.max(0, (progress.total - progress.position) / progress.rate));
}

function formatRate(job: DesktopJob) {
  const rate = job.progress?.rate;
  return rate && rate > 0 ? `${formatNumber(rate, 1)}/sec` : "Measuring";
}

function durationBetween(start: number | null, end: number | null) {
  if (!start) return "Not started";
  return formatDuration(Math.max(0, (end ?? epochSeconds()) - start));
}

function formatDuration(seconds: number) {
  const rounded = Math.round(seconds);
  if (rounded < 60) return `${rounded}s`;
  const minutes = Math.floor(rounded / 60);
  if (minutes < 60) return `${minutes}m ${rounded % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function formatDateTime(value: number | null) { return value ? new Date(value * 1_000).toLocaleString() : "Unavailable"; }
function formatNumber(value: number, maximumFractionDigits = 0) { return new Intl.NumberFormat(undefined, { maximumFractionDigits }).format(value); }
function epochSeconds() { return Math.floor(Date.now() / 1_000); }
function isTerminal(state: JobState) { return state === "succeeded" || state === "failed" || state === "cancelled"; }
function jobActivity(job: DesktopJob): "transcribing" | "writing" | "other" {
  if (job.kind === "transcribe") return "transcribing";
  if (job.kind === "notes" || job.kind === "rebuildLog") return "writing";
  if (job.kind !== "run") return "other";
  const phase = job.phase?.toLowerCase() ?? "";
  return phase.includes("outline") || phase.includes("notes") || phase.includes("campaign log")
    ? "writing"
    : "transcribing";
}
function stateLabel(state: JobState) {
  if (state === "succeeded") return "Completed";
  if (state === "failed") return "Failed";
  if (state === "cancelled") return "Cancelled";
  if (state === "cancelling") return "Cancelling";
  if (state === "running") return "Running";
  return "Queued";
}

function kindLabel(kind: DesktopJob["kind"]) {
  const labels: Partial<Record<DesktopJob["kind"], string>> = {
    import: "Audio import", doctor: "System check", rebuildLog: "Campaign log", reindex: "Search index", export: "Campaign export",
    candidateResolve: "Candidate resolution", speakerMap: "Speaker mapping", sessionRename: "Session rename", model: "Model management",
    record: "Recording", run: "Session processing", transcribe: "Transcription", notes: "Note generation",
  };
  return labels[kind] ?? kind.charAt(0).toUpperCase() + kind.slice(1);
}