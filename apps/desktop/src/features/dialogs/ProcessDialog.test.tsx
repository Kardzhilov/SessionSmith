import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StrictMode } from "react";
import { describe, expect, it, vi } from "vitest";
import { desktop } from "../../api/desktop";
import type { InboxAudio } from "../../api/types";
import { modelInventory } from "../../test/fixtures";
import { ProcessDialog } from "./ProcessDialog";

const audio: InboxAudio[] = [
  { name: "first-session.wav", path: "/audio/first-session.wav", sizeBytes: 1_048_576, modifiedAt: null },
  { name: "second-session.wav", path: "/audio/second-session.wav", sizeBytes: 2_097_152, modifiedAt: null },
];

describe("ProcessDialog audio selection", () => {
  it("starts empty and lets select all toggle every audio file", async () => {
    vi.spyOn(desktop, "modelsInventory").mockResolvedValue(modelInventory);
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <StrictMode>
        <ProcessDialog
          open
          audio={audio}
          submitting={false}
          error={null}
          onClose={vi.fn()}
          onSubmit={onSubmit}
        />
      </StrictMode>,
    );

    const selectAll = screen.getByRole("checkbox", { name: "Select all Inbox audio" });
    const first = screen.getByRole("checkbox", { name: /first-session\.wav/ });
    const second = screen.getByRole("checkbox", { name: /second-session\.wav/ });

    expect(selectAll).not.toBeChecked();
    expect(first).not.toBeChecked();
    expect(second).not.toBeChecked();
    expect(screen.getByText("0 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start processing" })).toBeDisabled();

    await user.click(selectAll);
    expect(selectAll).toBeChecked();
    expect(first).toBeChecked();
    expect(second).toBeChecked();
    expect(first).toBeDisabled();
    expect(second).toBeDisabled();
    expect(screen.getByText("2 selected")).toBeInTheDocument();

    await user.click(selectAll);
    expect(selectAll).not.toBeChecked();
    expect(first).not.toBeChecked();
    expect(second).not.toBeChecked();
    expect(first).toBeEnabled();
    expect(second).toBeEnabled();
    expect(screen.getByText("0 selected")).toBeInTheDocument();

    await user.click(first);
    await user.click(screen.getByRole("button", { name: "Start processing" }));
    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      sourcePaths: ["/audio/first-session.wav"],
      allInbox: false,
    }));
  });
});
