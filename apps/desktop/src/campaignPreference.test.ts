import { beforeEach, describe, expect, it } from "vitest";
import type { CampaignSummary } from "./api/types";
import {
  loadLastActiveCampaignId,
  selectAvailableCampaign,
  storeLastActiveCampaignId,
} from "./campaignPreference";

function campaign(id: string, loadError: string | null = null): CampaignSummary {
  return {
    id,
    name: id,
    gm: "",
    setting: "",
    presetId: "generic",
    backendKind: "local",
    sessionCount: 0,
    hasCampaignLog: false,
    loadError,
  };
}

describe("active campaign preference", () => {
  beforeEach(() => window.localStorage.clear());

  it("restores the last active campaign when it is still available", () => {
    storeLastActiveCampaignId("last-active");

    const selected = selectAvailableCampaign(
      [campaign("first"), campaign("last-active")],
      loadLastActiveCampaignId(),
    );

    expect(selected?.id).toBe("last-active");
  });

  it("falls back to the first loadable campaign when the preference is stale", () => {
    storeLastActiveCampaignId("deleted");

    expect(selectAvailableCampaign(
      [campaign("broken", "Could not load"), campaign("available")],
      loadLastActiveCampaignId(),
    )?.id).toBe("available");
  });

  it("clears the preference when no campaign is active", () => {
    storeLastActiveCampaignId("campaign");
    storeLastActiveCampaignId(null);

    expect(loadLastActiveCampaignId()).toBeNull();
  });
});