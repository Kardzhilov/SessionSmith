import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { axe } from "jest-axe";
import { describe, expect, it, vi } from "vitest";
import { desktop } from "../../api/desktop";
import { SpeakerReviewDialog } from "../dialogs/SpeakerReviewDialog";
import { HealthPage } from "../health/Health";
import { NotificationViewport } from "../notifications/Notifications";
import { OnboardingScreen } from "../onboarding/Onboarding";
import { CampaignSettingsPage } from "../settings/CampaignSettings";
import {
  campaignSettings,
  campaignSummary,
  healthReport,
  modelInventory,
  onboardingState,
  speakerReview,
} from "../../test/fixtures";

describe("high-risk surface accessibility", () => {
  it("covers Health remedies", async () => {
    const onRemedy = vi.fn();
    const { container } = render(<HealthPage report={healthReport} loading={false} error={null} onRefresh={vi.fn()} onRunChecks={vi.fn()} onRemedy={onRemedy} checking={false} />);

    await userEvent.click(screen.getByRole("button", { name: "Setup guide" }));
    expect(onRemedy).toHaveBeenCalledWith("install-ffmpeg");
    expect(await axe(container)).toHaveNoViolations();
  });

  it("covers onboarding and its initial focus", async () => {
    vi.spyOn(desktop, "campaignCreateOptions").mockResolvedValue({ presets: campaignSettings.presets });
    vi.spyOn(desktop, "modelsInventory").mockResolvedValue(modelInventory);
    const { container } = render(<OnboardingScreen state={onboardingState} health={healthReport} healthLoading={false} healthError={null} initialCampaignId={null} onRefreshHealth={vi.fn()} onRunChecks={vi.fn()} onHandoff={vi.fn()} onCampaignCreated={vi.fn()} onComplete={vi.fn()} />);

    await waitFor(() => expect(screen.getByRole("heading", { name: "Set up SessionSmith" })).toHaveFocus());
    expect(await axe(container)).toHaveNoViolations();
  });

  it("covers Campaign Settings rename and identity edit controls", async () => {
    vi.spyOn(desktop, "campaignSettings").mockResolvedValue(campaignSettings);
    const { container } = render(<CampaignSettingsPage campaign={campaignSummary} onCampaignRenamed={vi.fn()} />);
    await screen.findByRole("heading", { name: "Thursday Game" });

    await userEvent.click(screen.getByRole("button", { name: "Rename" }));
    expect(screen.getByRole("textbox", { name: "New campaign name" })).toHaveFocus();
    expect(screen.getByRole("textbox", { name: "Type the new name to confirm" })).toBeEnabled();
    expect(await axe(container)).toHaveNoViolations();

    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await userEvent.click(screen.getAllByRole("button", { name: "Edit" })[0]);
    expect(screen.getByRole("textbox", { name: "Game master" })).toBeEnabled();
    expect(await axe(container)).toHaveNoViolations();
  });

  it("covers Speaker Review labels and actions", async () => {
    vi.spyOn(desktop, "speakerReview").mockResolvedValue(speakerReview);
    const onClose = vi.fn();
    const { container } = render(<SpeakerReviewDialog open campaignId="thursday-game" stem="2026-08-27" submitting={false} error={null} onPreviewSample={vi.fn().mockResolvedValue(undefined)} onClose={onClose} onSubmit={vi.fn()} onReset={vi.fn()} />);
    const dialog = await screen.findByRole("dialog", { name: "Review speakers" });

    expect(screen.getByRole("heading", { name: "Review speakers" })).toHaveFocus();
    const assignedName = within(dialog).getByRole("combobox", { name: "Assigned name" });
    expect(assignedName).toBeEnabled();
    expect(within(assignedName).getByRole("option", { name: "Avery" })).toBeInTheDocument();
    expect(within(dialog).getByRole("checkbox", { name: "Save as campaign default" })).toHaveAttribute("type", "checkbox");
    expect(within(dialog).getByRole("button", { name: "Play SPEAKER_00 sample from 0:12" })).toBeEnabled();
    expect(await axe(container)).toHaveNoViolations();
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("covers assertive and polite Notifications regions", async () => {
    const { container } = render(<NotificationViewport notifications={[
      { id: 1, key: "failed", tone: "error", title: "Export failed", message: "The destination is unavailable." },
      { id: 2, key: "saved", tone: "success", title: "Campaign saved" },
    ]} onDismiss={vi.fn()} />);

    expect(container.querySelector('[aria-live="assertive"]')).toHaveTextContent("Export failed");
    expect(container.querySelector('[aria-live="polite"]')).toHaveTextContent("Campaign saved");
    expect(await axe(container)).toHaveNoViolations();
  });
});