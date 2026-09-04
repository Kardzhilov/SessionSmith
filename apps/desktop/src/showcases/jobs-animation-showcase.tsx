import { StrictMode, useState } from "react";
import ReactDOM from "react-dom/client";
import { Pause, Play, RotateCcw } from "lucide-react";
import type { DesktopJob } from "../api/types";
import { JobsPage } from "../features/jobs/Jobs";
import "../styles/tokens.css";
import "../styles/app.css";
import "./jobs-animation-showcase.css";

function sampleJobs(): DesktopJob[] {
  const now = Math.floor(Date.now() / 1_000);
  return [
    {
      id: 101,
      kind: "transcribe",
      title: "Transcription: incoming audio waves and ear",
      state: "running",
      startedAt: now - 93,
      finishedAt: null,
      summary: null,
      activeChildren: 2,
      phase: "Decoding speech from session audio",
      progress: { label: "Audio decoded", position: 38, total: 100, rate: 0.8 },
      logTail: ["Loaded speech model", "Reading audio frames", "Decoding segment 38"],
      canCancel: true,
    },
    {
      id: 102,
      kind: "notes",
      title: "Document generation: writing hand and paper",
      state: "running",
      startedAt: now - 47,
      finishedAt: null,
      summary: null,
      activeChildren: 6,
      phase: "Writing summaries, recaps, notes, and story",
      progress: { label: "Generating documents", position: 2, total: 0, rate: null },
      logTail: ["Outline ready", "Generating six document types", "Writing campaign summary"],
      canCancel: true,
    },
    {
      id: 103,
      kind: "doctor",
      title: "General operation: orbital activity signal",
      state: "running",
      startedAt: now - 12,
      finishedAt: null,
      summary: null,
      activeChildren: 1,
      phase: "Checking local dependencies",
      progress: { label: "Checks completed", position: 3, total: 6, rate: 0.4 },
      logTail: ["FFmpeg available", "Audio backend available", "Checking model runtime"],
      canCancel: true,
    },
  ];
}

function JobsAnimationShowcase() {
  const [paused, setPaused] = useState(false);
  const [revision, setRevision] = useState(0);

  return (
    <main className="animation-showcase">
      <header className="animation-showcase__toolbar">
        <div>
          <strong>Job animation review</strong>
          <span>Production components with representative live data</span>
        </div>
        <div className="animation-showcase__controls">
          <button className="button button--quiet" type="button" onClick={() => setPaused((value) => !value)}>
            {paused ? <Play size={15} aria-hidden="true" /> : <Pause size={15} aria-hidden="true" />}
            {paused ? "Resume all" : "Pause all"}
          </button>
          <button className="button button--quiet" type="button" onClick={() => { setRevision((value) => value + 1); setPaused(false); }}>
            <RotateCcw size={15} aria-hidden="true" />Replay all
          </button>
        </div>
      </header>
      <div key={revision} className={paused ? "animation-showcase__stage animation-showcase__stage--paused" : "animation-showcase__stage"}>
        <JobsPage
          jobs={sampleJobs()}
          error={null}
          onCancel={() => undefined}
          onClearHistory={() => Promise.resolve()}
          onRefresh={() => undefined}
        />
      </div>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <JobsAnimationShowcase />
  </StrictMode>,
);
