import { render, screen, within } from "@testing-library/react";
import { axe } from "jest-axe";
import { describe, expect, it, vi } from "vitest";
import { desktop } from "../../api/desktop";
import { transcriptPage } from "../../test/fixtures";
import { TranscriptPanel } from "./Workspace";

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 52,
    getVirtualItems: () => Array.from({ length: count }, (_, index) => ({ index, key: index, start: index * 52 })),
    measureElement: vi.fn(),
    scrollToIndex: vi.fn(),
  }),
}));

describe("TranscriptPanel accessibility", () => {
  it("exposes representative virtual rows with coherent list positions", async () => {
    vi.spyOn(desktop, "transcriptRead").mockResolvedValue(transcriptPage);
    const { container } = render(<TranscriptPanel campaignId="thursday-game" stem="2026-08-27" totalLines={2} initialTranscriptLine={null} refreshKey={0} onSeekTo={vi.fn()} playbackPositionMs={0} playing={false} />);
    const list = await screen.findByRole("list", { name: "Transcript lines, 2 total" });
    const rows = within(list).getAllByRole("listitem");

    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveAttribute("aria-posinset", "1");
    expect(rows[0]).toHaveAttribute("aria-setsize", "2");
    expect(within(rows[0]).getByRole("button", { name: "00:12" })).toHaveAccessibleName("00:12");
    expect(await axe(container)).toHaveNoViolations();
  });
});