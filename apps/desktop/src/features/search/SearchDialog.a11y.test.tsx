import { render } from "@testing-library/react";
import { axe } from "jest-axe";
import { describe, expect, it, vi } from "vitest";
import { SearchDialog } from "./SearchDialog";

describe("SearchDialog accessibility", () => {
  it("has no automated accessibility violations in its empty state", async () => {
    const { container } = render(
      <SearchDialog
        open
        campaignId="campaign-1"
        campaignName="Thursday Game"
        canReindex
        reindexing={false}
        onClose={vi.fn()}
        onReindex={vi.fn()}
        onOpenResult={vi.fn()}
        query=""
        onQueryChange={vi.fn()}
        allCampaigns={false}
        onAllCampaignsChange={vi.fn()}
        sourceKinds={[]}
        onSourceKindsChange={vi.fn()}
        sourceOptions={[{ id: "transcript", label: "Transcripts" }]}
      />,
    );

    expect(await axe(container)).toHaveNoViolations();
  });
});