import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { NotificationViewport } from "./Notifications";

describe("NotificationViewport", () => {
  it("renders and invokes an export folder action", async () => {
    const openFolder = vi.fn();

    render(
      <NotificationViewport
        notifications={[{
          id: 1,
          key: "export:1:succeeded",
          tone: "success",
          title: "Export completed",
          action: { label: "Open folder", onClick: openFolder },
        }]}
        onDismiss={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Open folder" }));

    expect(openFolder).toHaveBeenCalledOnce();
  });
});