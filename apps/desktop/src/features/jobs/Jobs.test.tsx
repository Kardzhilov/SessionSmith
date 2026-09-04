import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { DesktopJob } from "../../api/types";
import { JobsPage } from "./Jobs";

function job(overrides: Partial<DesktopJob> = {}): DesktopJob {
  return {
    id: 7,
    kind: "transcribe",
    title: "Transcribe Thursday session",
    state: "running",
    startedAt: Math.floor(Date.now() / 1_000) - 30,
    finishedAt: null,
    summary: null,
    activeChildren: 2,
    phase: "Decoding audio",
    progress: {
      label: "Audio decoded",
      position: 50,
      total: 100,
      rate: 5,
    },
    logTail: ["Loaded model", "Processing segment 4"],
    canCancel: true,
    ...overrides,
  };
}

describe("JobsPage", () => {
  it("shows reported determinate progress, timing, output, and cancellation", async () => {
    const onCancel = vi.fn();
    render(<JobsPage jobs={[job()]} error={null} onCancel={onCancel} onClearHistory={vi.fn().mockResolvedValue(undefined)} onRefresh={vi.fn()} />);

    const progress = screen.getByRole("progressbar", { name: "Audio decoded" });
    expect(progress).toHaveAttribute("aria-valuenow", "50");
    expect(screen.getByText("50 of 100 · 50%")).toBeInTheDocument();
    expect(screen.getByText("10s")).toBeInTheDocument();
    expect(screen.getByText("5/sec")).toBeInTheDocument();
    expect(screen.getByText("Processing segment 4")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Cancel job" }));
    expect(onCancel).toHaveBeenCalledWith(7);
  });

  it("uses activity-specific visuals and switches a full run when document generation starts", () => {
    const { rerender } = render(
      <JobsPage
        jobs={[job({ kind: "run", phase: "Transcribe · Thursday session" })]}
        error={null}
        onCancel={vi.fn()}
        onClearHistory={vi.fn().mockResolvedValue(undefined)}
        onRefresh={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Audio waves entering an ear")).toBeInTheDocument();
    expect(screen.queryByLabelText("A hand writing a document")).not.toBeInTheDocument();

    rerender(
      <JobsPage
        jobs={[job({ kind: "run", phase: "Notes" })]}
        error={null}
        onCancel={vi.fn()}
        onClearHistory={vi.fn().mockResolvedValue(undefined)}
        onRefresh={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("A hand writing a document")).toBeInTheDocument();
    expect(screen.queryByLabelText("Audio waves entering an ear")).not.toBeInTheDocument();
  });

  it("keeps indeterminate progress honest when no total or rate is reported", () => {
    render(
      <JobsPage
        jobs={[job({ progress: { label: "Segments processed", position: 7, total: 0, rate: null } })]}
        error={null}
        onCancel={vi.fn()}
        onClearHistory={vi.fn().mockResolvedValue(undefined)}
        onRefresh={vi.fn()}
      />,
    );

    const progress = screen.getByRole("progressbar", { name: "Segments processed" });
    expect(progress).not.toHaveAttribute("aria-valuenow");
    expect(screen.getByText("7 completed")).toBeInTheDocument();
    expect(screen.getByText("Calculating")).toBeInTheDocument();
    expect(screen.getByText("Measuring")).toBeInTheDocument();
    expect(screen.queryByText(/7%/)).not.toBeInTheDocument();
  });

  it("exposes completed job details and full output in expandable history", async () => {
    render(
      <JobsPage
        jobs={[job({
          state: "succeeded",
          finishedAt: Math.floor(Date.now() / 1_000),
          summary: "Transcript written",
          canCancel: false,
        })]}
        error={null}
        onCancel={vi.fn()}
        onClearHistory={vi.fn().mockResolvedValue(undefined)}
        onRefresh={vi.fn()}
      />,
    );

    expect(screen.getByText("Transcription · Transcript written")).toBeInTheDocument();
    await userEvent.click(screen.getByText("Transcribe Thursday session"));
    expect(screen.getByText("Transcript written")).toBeInTheDocument();
    expect(screen.getByLabelText("Job output")).toHaveTextContent("Loaded model");
    expect(screen.getByLabelText("Job output")).toHaveTextContent("Processing segment 4");
    expect(screen.queryByRole("button", { name: "Cancel job" })).not.toBeInTheDocument();
  });

  it("requires confirmation before clearing terminal history", async () => {
    const onClearHistory = vi.fn().mockResolvedValue(undefined);
    render(
      <JobsPage
        jobs={[job({ state: "failed", finishedAt: Math.floor(Date.now() / 1_000), canCancel: false })]}
        error={null}
        onCancel={vi.fn()}
        onClearHistory={onClearHistory}
        onRefresh={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Clear history" }));
    expect(onClearHistory).not.toHaveBeenCalled();
    expect(screen.getByRole("group", { name: "Confirm clear job history" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Clear all" }));
    expect(onClearHistory).toHaveBeenCalledOnce();
  });
});
