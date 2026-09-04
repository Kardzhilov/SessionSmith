import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { desktop } from "../../api/desktop";
import type { AudioPlayerSnapshot } from "../../api/types";
import { GlobalAudioProvider, useGlobalAudio } from "./GlobalAudioPlayer";

vi.mock("../../api/generated/bindings", () => ({
  events: {
    audioTransition: {
      listen: vi.fn().mockResolvedValue(vi.fn()),
    },
  },
}));

vi.mock("../settings/AppSettingsContext", () => ({
  useAppSettings: () => ({
    settings: {
      appearance: "system",
      audioDir: "audio",
      campaignsDir: "campaigns",
      dateFormat: "medium",
      outputDir: "output",
      playerVolume: 50,
      theme: "forest",
    },
    save: vi.fn().mockResolvedValue(undefined),
  }),
}));

const unloaded: AudioPlayerSnapshot = {
  status: "unloaded",
  label: null,
  sourceId: null,
  revision: 0,
  positionMs: 0,
  durationMs: null,
  volume: 50,
  error: null,
};

const loaded: AudioPlayerSnapshot = {
  status: "stopped",
  label: "combined.wav",
  sourceId: 7,
  revision: 1,
  positionMs: 0,
  durationMs: 90_000,
  volume: 50,
  error: null,
};

function SessionPlaybackButton() {
  const { playSession } = useGlobalAudio();
  return <button type="button" onClick={() => void playSession("campaign", "combined")}>Play session</button>;
}

describe("GlobalAudioProvider", () => {
  it("keeps playback available after the initiating screen unmounts", async () => {
    vi.spyOn(desktop, "audioState").mockResolvedValue(unloaded);
    vi.spyOn(desktop, "audioLoad").mockResolvedValue(loaded);
    vi.spyOn(desktop, "audioPlay").mockResolvedValue({ ...loaded, status: "playing", revision: 2 });
    const pause = vi.spyOn(desktop, "audioPause").mockResolvedValue({
      ...loaded,
      status: "paused",
      revision: 3,
    });
    const stop = vi.spyOn(desktop, "audioStop").mockResolvedValue(loaded);

    const { rerender } = render(
      <GlobalAudioProvider>
        <SessionPlaybackButton />
      </GlobalAudioProvider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Play session" }));
    expect(await screen.findByRole("button", { name: "Pause audio" })).toBeEnabled();

    rerender(<GlobalAudioProvider><p>Another screen</p></GlobalAudioProvider>);

    expect(screen.getByText("combined.wav")).toBeInTheDocument();
    expect(stop).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Pause audio" }));
    expect(pause).toHaveBeenCalledWith(7);
  });
});