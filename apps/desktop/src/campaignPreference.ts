import type { CampaignSummary } from "./api/types";

const activeCampaignKey = "sessionsmith:active-campaign";

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem" | "removeItem">;

export function loadLastActiveCampaignId(storage: ReadableStorage = window.localStorage) {
  try {
    const campaignId = storage.getItem(activeCampaignKey)?.trim();
    return campaignId || null;
  } catch {
    return null;
  }
}

export function storeLastActiveCampaignId(
  campaignId: string | null,
  storage: WritableStorage = window.localStorage,
) {
  try {
    if (campaignId) {
      storage.setItem(activeCampaignKey, campaignId);
    } else {
      storage.removeItem(activeCampaignKey);
    }
  } catch {}
}

export function selectAvailableCampaign(
  campaigns: CampaignSummary[],
  preferredCampaignId: string | null,
) {
  return campaigns.find(
    (campaign) => campaign.id === preferredCampaignId && !campaign.loadError,
  ) ?? campaigns.find((campaign) => !campaign.loadError);
}