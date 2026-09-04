import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { desktop } from "../../api/desktop";
import { campaignSettings, healthReport, modelInventory, onboardingState } from "../../test/fixtures";
import { OnboardingScreen } from "./Onboarding";

describe("Onboarding health refresh", () => {
  it("keeps a visible completion message after a fast refresh", async () => {
    vi.spyOn(desktop, "campaignCreateOptions").mockResolvedValue({ presets: campaignSettings.presets });
    vi.spyOn(desktop, "modelsInventory").mockResolvedValue(modelInventory);
    let finishRefresh: (() => void) | undefined;
    const onRefreshHealth = vi.fn(() => new Promise<void>((resolve) => {
      finishRefresh = resolve;
    }));
    render(<OnboardingScreen
      state={onboardingState}
      health={healthReport}
      healthLoading={false}
      healthError={null}
      initialCampaignId={null}
      onRefreshHealth={onRefreshHealth}
      onRunChecks={vi.fn()}
      onHandoff={vi.fn()}
      onCampaignCreated={vi.fn()}
      onComplete={vi.fn()}
    />);

    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(screen.getByRole("button", { name: "Refreshing" })).toBeDisabled();
    await act(async () => finishRefresh?.());

    expect(await screen.findByRole("status")).toHaveTextContent("Health status refreshed just now");
  });
});