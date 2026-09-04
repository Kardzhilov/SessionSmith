import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { desktop } from "../../api/desktop";
import type { AppSettings } from "../../api/types";
import { AppSettingsProvider, useAppSettings } from "./AppSettingsContext";

const settings: AppSettings = {
  revision: "initial-revision",
  dateFormat: "dmy",
  appearance: "system",
  theme: "default",
  playerVolume: 50,
  audioDir: "audio",
  campaignsDir: "campaigns",
  outputDir: "output",
  themes: [],
};

function SaveHarness() {
  const { loading, save } = useAppSettings();
  if (loading) return <span>Loading</span>;
  return <button type="button" onClick={() => void save({ dateFormat: "dmy", appearance: "light", theme: "default", playerVolume: 50, audioDir: "audio", campaignsDir: "campaigns", outputDir: "output" })}>Save settings</button>;
}

describe("AppSettingsProvider", () => {
  it("uses the latest revision when another settings surface has written the config", async () => {
    vi.spyOn(desktop, "appSettings")
      .mockResolvedValueOnce(settings)
      .mockResolvedValueOnce({ ...settings, revision: "latest-revision" });
    const write = vi.spyOn(desktop, "appSettingsWrite").mockResolvedValue({
      ...settings,
      revision: "saved-revision",
      appearance: "light",
    });
    render(<AppSettingsProvider><SaveHarness /></AppSettingsProvider>);

    await userEvent.click(await screen.findByRole("button", { name: "Save settings" }));

    expect(write).toHaveBeenCalledWith(expect.objectContaining({
      appearance: "light",
      expectedRevision: "latest-revision",
    }));
  });
});