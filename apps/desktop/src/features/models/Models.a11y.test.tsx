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

  it("matches multilingual models to named languages and only lists present states", async () => {
    vi.spyOn(desktop, "modelsInventory").mockResolvedValue({
      ...modelInventory,
      whisper: [
        { ...modelInventory.whisper[0], languages: ["Multilingual"], languageSummary: "99 languages" },
        modelInventory.whisper[1],
      ],
      asr: [{
        ...modelInventory.whisper[0],
        id: "granite",
        label: "Granite Speech",
        engine: "IBM Granite Speech",
        languages: ["English", "French"],
        languageSummary: "English and French",
      }],
    });
    const user = userEvent.setup();
    render(<ModelInventoryPage refreshKey={0} modelRunning={false} jobError={null} onActionStarting={vi.fn()} onJobStarted={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Browse catalog (3)" }));

    const dialog = screen.getByRole("dialog", { name: "Transcription catalog" });
    const stateFilter = within(dialog).getByRole("combobox", { name: "Install state" });
    expect(within(stateFilter).queryByRole("option", { name: "Ready" })).not.toBeInTheDocument();
    await user.selectOptions(within(dialog).getByRole("combobox", { name: "Language" }), "French");

    expect(within(dialog).getByText("2 of 3 models")).toBeInTheDocument();
    expect(within(dialog).getAllByRole("heading", { name: "Small English" }).length).toBeGreaterThan(0);
    expect(within(dialog).getAllByRole("heading", { name: "Granite Speech" }).length).toBeGreaterThan(0);
    expect(within(dialog).queryByRole("heading", { name: "Medium English" })).not.toBeInTheDocument();
  });
});