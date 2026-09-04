import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { AppUpdateControl } from "./AppUpdateControl";

vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));

describe("AppUpdateControl", () => {
  beforeEach(() => {
    vi.mocked(check).mockReset();
    vi.mocked(relaunch).mockReset();
  });

  it("reports when the installed version is current", async () => {
    vi.mocked(check).mockResolvedValue(null);

    render(<AppUpdateControl currentVersion="1.1.0" />);

    expect(await screen.findByText("Version 1.1.0 is up to date.")).toBeInTheDocument();
  });

  it("downloads, installs, and relaunches for an available update", async () => {
    const downloadAndInstall = vi.fn(async (onEvent: (event: { event: "Finished" }) => void) => {
      onEvent({ event: "Finished" });
    });
    const close = vi.fn().mockResolvedValue(undefined);
    vi.mocked(check).mockResolvedValue({ version: "1.2.0", downloadAndInstall, close } as never);

    render(<AppUpdateControl currentVersion="1.1.0" />);
    await userEvent.click(await screen.findByRole("button", { name: "Download and install" }));

    await waitFor(() => expect(downloadAndInstall).toHaveBeenCalledOnce());
    await waitFor(() => expect(relaunch).toHaveBeenCalledOnce());
  });
});