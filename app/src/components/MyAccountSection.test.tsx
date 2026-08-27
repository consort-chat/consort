import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const logout = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  logout,
}));

import { MyAccountSection } from "./MyAccountSection";
import type { Profile } from "../lib/api";

const profile: Profile = {
  user_id: "@ada:example.org",
  device_id: "ABCDEFGH",
  homeserver: "https://example.org",
  display_name: "Ada",
  avatar_url: null,
};

describe("MyAccountSection", () => {
  beforeEach(() => {
    logout.mockReset().mockResolvedValue(undefined);
  });

  it("shows the three identifiers worth being able to copy", () => {
    // The device id is not decoration. It is what you match against a
    // homeserver's device list when working out why a session is unverified.
    render(<MyAccountSection profile={profile} onSignedOut={vi.fn()} />);

    expect(screen.getByText("@ada:example.org")).toBeVisible();
    expect(screen.getByText("ABCDEFGH")).toBeVisible();
    expect(screen.getByText("https://example.org")).toBeVisible();
  });

  it("signs out when asked", async () => {
    const onSignedOut = vi.fn();
    render(<MyAccountSection profile={profile} onSignedOut={onSignedOut} />);

    await userEvent.click(screen.getByRole("button", { name: /log out/i }));

    await waitFor(() => expect(logout).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onSignedOut).toHaveBeenCalledTimes(1));
  });

  it("cannot be asked to sign out twice", async () => {
    // The button is the only irreversible control in the application, and a
    // slow server is exactly when somebody presses it again.
    let finish = () => {};
    logout.mockImplementation(
      () => new Promise<void>((resolve) => (finish = resolve)),
    );
    render(<MyAccountSection profile={profile} onSignedOut={vi.fn()} />);
    const button = screen.getByRole("button", { name: /log out/i });

    await userEvent.click(button);

    expect(button).toBeDisabled();
    finish();
  });

  it("logs the developer-facing detail of a failed sign-out", async () => {
    // The person sees nothing, because they are already on their way to the
    // login screen. Whoever is debugging it needs the sentence the server
    // actually sent.
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    logout.mockRejectedValue({ message: "Could not reach it.", detail: "timeout" });
    render(<MyAccountSection profile={profile} onSignedOut={vi.fn()} />);

    await userEvent.click(screen.getByRole("button", { name: /log out/i }));

    await waitFor(() =>
      expect(consoleError).toHaveBeenCalledWith(
        "logout reported an error",
        "timeout",
      ),
    );
  });

  it("still signs out locally when the server refuses", async () => {
    // `logout` clears the local session either way, so there is no state
    // where staying on this screen is the right answer.
    logout.mockRejectedValue({ message: "no", detail: "no" });
    const onSignedOut = vi.fn();
    render(<MyAccountSection profile={profile} onSignedOut={onSignedOut} />);

    await userEvent.click(screen.getByRole("button", { name: /log out/i }));

    await waitFor(() => expect(onSignedOut).toHaveBeenCalledTimes(1));
  });
});
