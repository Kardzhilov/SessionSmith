import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { axe } from "jest-axe";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { desktop } from "../../api/desktop";
import { modelInventory } from "../../test/fixtures";
import { ModelInventoryPage } from "./Models";

function ModelsHarness() {
  const [visible, setVisible] = useState(false);
  return <>
    <button type="button" onClick={() => setVisible(true)}>Show models</button>
    {visible && <ModelInventoryPage refreshKey={0} modelRunning={false} jobError={null} onActionStarting={vi.fn()} onJobStarted={vi.fn()} />}
  </>;
}

describe("Model catalog accessibility", () => {
  it("passes axe, traps focus, closes with Escape, and restores focus", async () => {
    vi.spyOn(desktop, "modelsInventory").mockResolvedValue(modelInventory);
    const user = userEvent.setup();
    const { container } = render(<ModelsHarness />);
    await user.click(screen.getByRole("button", { name: "Show models" }));
    const browse = await screen.findByRole("button", { name: "Browse catalog (2)" });
    await user.click(browse);

    const dialog = screen.getByRole("dialog", { name: "Transcription catalog" });
    const search = within(dialog).getByRole("searchbox", { name: "Search models" });
    await waitFor(() => expect(search).toHaveFocus());
    expect(await axe(container)).toHaveNoViolations();

    const focusable = Array.from(dialog.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]'));
    const last = focusable[focusable.length - 1];
    last.focus();
    await user.tab();
    expect(within(dialog).getByRole("button", { name: "Close catalog" })).toHaveFocus();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Transcription catalog" })).not.toBeInTheDocument();
    expect(browse).toHaveFocus();
  });
});