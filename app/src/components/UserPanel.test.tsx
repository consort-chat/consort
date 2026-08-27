import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { UserPanel } from "./UserPanel";
import type { Connection, Profile } from "../lib/api";

const profile: Profile = {
  user_id: "@ada:example.org",
  device_id: "ABCDEFGH",
  homeserver: "https://example.org",
  display_name: "Ada",
  avatar_url: null,
};

function panel(connection: Connection = { state: "live" }, onOpenSettings = vi.fn()) {
  render(
    <UserPanel
      profile={profile}
      connection={connection}
      onOpenSettings={onOpenSettings}
    />,
  );
  return onOpenSettings;
}

describe("UserPanel", () => {
  it("names the account and says what the connection is doing", () => {
    panel({ state: "offline", attempt: 2, retryInSeconds: 4 });

    expect(screen.getByText("Ada")).toBeVisible();
    expect(screen.getByText(/reconnecting/i)).toBeVisible();
  });

  it("offers a way into settings", () => {
    panel();

    // Found by its accessible name rather than its glyph. The control is an
    // icon, so the name is the only thing a screen reader or a test has.
    expect(screen.getByRole("button", { name: /settings/i })).toBeVisible();
  });

  it("opens settings when the icon is pressed", async () => {
    const onOpenSettings = panel();

    await userEvent.click(screen.getByRole("button", { name: /settings/i }));

    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it("no longer signs anybody out from the strip itself", () => {
    // Sign out moved into the settings modal, under My Account, which is
    // where every client with this strip puts it. Leaving a second one here
    // would put an irreversible action one stray click from a device picker.
    panel();

    expect(screen.queryByRole("button", { name: /sign out/i })).toBeNull();
  });

  it("carries the user id for the case where the name is truncated", () => {
    panel();

    expect(screen.getByText("Ada")).toHaveAttribute("title", "@ada:example.org");
  });
});
