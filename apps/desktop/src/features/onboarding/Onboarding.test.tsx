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

describe("Onboarding campaign creation", () => {
  it("creates a campaign with multiple players entered during setup", async () => {
    vi.spyOn(desktop, "campaignCreateOptions").mockResolvedValue({ presets: campaignSettings.presets });
    vi.spyOn(desktop, "modelsInventory").mockResolvedValue(modelInventory);
    const createCampaign = vi.spyOn(desktop, "campaignCreate").mockResolvedValue({ campaignId: "new-table", name: "New Table" });

    render(<OnboardingScreen
      state={onboardingState}
      health={healthReport}
      healthLoading={false}
      healthError={null}
      initialCampaignId={null}
      initialStep="campaign"
      onRefreshHealth={vi.fn()}
      onRunChecks={vi.fn()}
      onHandoff={vi.fn()}
      onCampaignCreated={vi.fn().mockResolvedValue(undefined)}
      onComplete={vi.fn()}
    />);

    await userEvent.type(screen.getByRole("textbox", { name: "Campaign name" }), "New Table");
    await userEvent.type(screen.getByRole("textbox", { name: /^Player$/ }), "Emilie");
    await userEvent.type(screen.getByRole("textbox", { name: /^Character$/ }), "Fatethrial");
    await userEvent.click(screen.getByRole("button", { name: "Add player" }));
    const playerInputs = screen.getAllByRole("textbox", { name: /^Player$/ });
    const characterInputs = screen.getAllByRole("textbox", { name: /^Character$/ });
    await userEvent.type(playerInputs[1], "Ravn");
    await userEvent.type(characterInputs[1], "Jan Simen");
    await userEvent.click(screen.getByRole("button", { name: "Create campaign" }));

    expect(createCampaign).toHaveBeenCalledWith(expect.objectContaining({
      players: [
        expect.objectContaining({ player: "Emilie", character: "Fatethrial" }),
        expect.objectContaining({ player: "Ravn", character: "Jan Simen" }),
      ],
    }));
  });
});