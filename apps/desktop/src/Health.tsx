import {
  CircleAlert,
  CircleCheck,
  CircleDashed,
  Cpu,
  LoaderCircle,
  MemoryStick,
  MonitorCog,
  RefreshCw,
  Sparkles,
} from "lucide-react";
import type { HealthCheck, HealthCheckState, HealthReport } from "./types";

export function HealthPage({
  report,
  loading,
  error,
  onRefresh,
  onRunChecks,
  onOpenModels,
  checking,
}: {
  report: HealthReport | null;
  loading: boolean;
  error: string | null;
  onRefresh: () => void;
  onRunChecks: () => void;
  onOpenModels: () => void;
  checking: boolean;
}) {
  if (!report && loading) {
    return <HealthLoading />;
  }

  if (!report) {
    return (
      <section className="state-panel state-panel--error" aria-live="polite">
        <CircleAlert size={22} aria-hidden="true" />
        <div>
          <p className="eyebrow">Checks unavailable</p>
          <h1>SessionSmith could not inspect this machine.</h1>
          <p>{error ?? "The health report did not return a result."}</p>
          <div className="health-page__actions">
            <button className="button button--quiet" type="button" onClick={onRefresh}>
              <RefreshCw size={16} aria-hidden="true" />
              Refresh
            </button>
            <button className="button button--primary" type="button" onClick={onRunChecks} disabled={checking}>
              {checking ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <CircleDashed size={16} aria-hidden="true" />}
              Run checks
            </button>
          </div>
        </div>
      </section>
    );
  }

  const summary = summarize(report.checks);
  const gpuLabel = report.hardware.gpu
    ? `${report.hardware.gpu.vendor} ${report.hardware.gpu.name}`
    : "CPU processing";
  const gpuDetail = report.hardware.gpu
    ? report.hardware.gpu.vramGb > 0
      ? `${report.hardware.gpu.vramGb} GB VRAM`
      : "Memory not reported"
    : "No supported GPU detected";

  return (
    <div className="health-page">
      <header className="page-header health-page__header">
        <div>
          <p className="eyebrow">This machine</p>
          <h1>Health</h1>
          <p className="page-subtitle">{summary.label}</p>
        </div>
        <div className="health-page__actions">
          <button
            className="icon-button"
            type="button"
            onClick={onRefresh}
            disabled={loading}
            title="Refresh health report"
            aria-label="Refresh health report"
          >
            <RefreshCw className={loading ? "is-spinning" : ""} size={17} aria-hidden="true" />
          </button>
          <button className="button button--primary" type="button" onClick={onRunChecks} disabled={checking}>
            {checking ? <LoaderCircle className="is-spinning" size={16} aria-hidden="true" /> : <CircleDashed size={16} aria-hidden="true" />}
            Run checks
          </button>
        </div>
      </header>

      {error && (
        <p className="health-refresh-error" role="status">
          The latest refresh did not complete. Showing the previous report: {error}
        </p>
      )}

      <section className="health-summary" aria-label="Hardware profile">
        <div className="health-summary__intro">
          <span className={`health-status health-status--${summary.state}`} aria-hidden="true">
            <StatusIcon state={summary.state} />
          </span>
          <div>
            <p className="eyebrow">System profile</p>
            <h2>{report.hardware.os}</h2>
            <p>{report.hardware.recommendationReason}</p>
          </div>
        </div>
        <dl className="health-metrics">
          <div>
            <dt><Cpu size={16} aria-hidden="true" /> Processor</dt>
            <dd>{report.hardware.cpuCores} {pluralize(report.hardware.cpuCores, "core")}</dd>
          </div>
          <div>
            <dt><MemoryStick size={16} aria-hidden="true" /> Memory</dt>
            <dd>{report.hardware.ramGb} GB RAM</dd>
          </div>
          <div>
            <dt><MonitorCog size={16} aria-hidden="true" /> Acceleration</dt>
            <dd>{gpuLabel}<small>{gpuDetail}</small></dd>
          </div>
        </dl>
      </section>

      <section className="health-section" aria-labelledby="health-checks-heading">
        <div className="section-header">
          <div>
            <h2 id="health-checks-heading">Readiness <span>{report.checks.length}</span></h2>
            <p>Dependencies and the configured language-model backend.</p>
          </div>
        </div>
        <div className="health-checks">
          {report.checks.map((check) => (
            <HealthCheckRow check={check} key={check.id} onOpenModels={onOpenModels} />
          ))}
        </div>
      </section>

      <section className="health-recommendations" aria-labelledby="health-recommendations-heading">
        <div>
          <p className="eyebrow">Recommended defaults</p>
          <h2 id="health-recommendations-heading">A practical starting point for this hardware</h2>
        </div>
        <div className="health-models">
          <div>
            <span>Transcription</span>
            <strong>{report.hardware.recommendedAsrModel}</strong>
          </div>
          <div>
            <span>Notes model</span>
            <strong>{report.hardware.recommendedLlmModel}</strong>
          </div>
          <Sparkles size={22} aria-hidden="true" />
        </div>
      </section>
    </div>
  );
}

function HealthCheckRow({
  check,
  onOpenModels,
}: {
  check: HealthCheck;
  onOpenModels: () => void;
}) {
  const stateLabel = check.state === "ok" ? "Ready" : check.state === "warn" ? "Optional" : "Needs attention";
  const canOpenModels = check.remedy === "open-models";

  return (
    <article className={`health-check health-check--${check.state}`}>
      <span className="health-status" aria-label={stateLabel}>
        <StatusIcon state={check.state} />
      </span>
      <div className="health-check__body">
        <h3>{check.label}</h3>
        <p>{check.detail}</p>
      </div>
      {canOpenModels ? (
        <button className="button button--quiet health-check__remedy" type="button" onClick={onOpenModels}>
          Models
        </button>
      ) : (
        <span className="health-check__state">{stateLabel}</span>
      )}
    </article>
  );
}

function HealthLoading() {
  return (
    <div className="health-page health-page--loading" aria-live="polite">
      <header className="page-header">
        <div>
          <p className="eyebrow">Inspecting this machine</p>
          <h1>Health</h1>
        </div>
        <LoaderCircle className="is-spinning" size={22} aria-label="Running health checks" />
      </header>
      <div className="health-skeleton health-skeleton--summary" />
      <div className="health-skeleton-list">
        <div className="health-skeleton" />
        <div className="health-skeleton" />
        <div className="health-skeleton" />
      </div>
    </div>
  );
}

function StatusIcon({ state }: { state: HealthCheckState }) {
  if (state === "ok") {
    return <CircleCheck size={20} aria-hidden="true" />;
  }
  if (state === "warn") {
    return <CircleDashed size={20} aria-hidden="true" />;
  }
  return <CircleAlert size={20} aria-hidden="true" />;
}

function summarize(checks: HealthCheck[]) {
  const failures = checks.filter((check) => check.state === "fail").length;
  const warnings = checks.filter((check) => check.state === "warn").length;

  if (failures > 0) {
    return {
      state: "fail" as const,
      label: `${failures} ${pluralize(failures, "item")} ${failures === 1 ? "needs" : "need"} attention`,
    };
  }
  if (warnings > 0) {
    return {
      state: "warn" as const,
      label: `${warnings} optional ${pluralize(warnings, "item")} unavailable`,
    };
  }
  return { state: "ok" as const, label: "All configured checks are ready" };
}

function pluralize(count: number, singular: string) {
  return count === 1 ? singular : `${singular}s`;
}
