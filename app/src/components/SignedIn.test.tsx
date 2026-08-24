import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const logout = vi.hoisted(() => vi.fn());
const tokenStorage = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  logout,
  tokenStorage,
}));

import { SignedIn } from "./SignedIn";
import type { Profile, TokenStorage } from "../lib/api";

const profile: Profile = {
  user_id: "@bob:example.org",
  device_id: "HZTIUXZKUU",
  homeserver: "https://example.org/",
  display_name: "Bob",
  avatar_url: null,
};

const keyring: TokenStorage = {
  kind: "keyring",
  description: "Your sign-in is stored in your system keyring.",
  isPreferred: true,
};

const fileFallback: TokenStorage = {
  kind: "file",
  description:
    "No system keyring was available, so your sign-in is stored in a file that only your user account can read.",
  isPreferred: false,
};

describe("SignedIn", () => {
  beforeEach(() => {
    logout.mockReset().mockResolvedValue(undefined);
    tokenStorage.mockReset().mockResolvedValue(keyring);
  });

  it("shows the display name when the account has one", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(
      await screen.findByRole("heading", { level: 1, name: "Bob" }),
    ).toBeVisible();
  });

  it("falls back to the user ID when there is no display name", async () => {
    render(
      <SignedIn
        profile={{ ...profile, display_name: null }}
        onSignedOut={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("heading", { level: 1, name: "@bob:example.org" }),
    ).toBeVisible();
  });

  it("prints the device ID, which is what you need for verification", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(await screen.findByText("HZTIUXZKUU")).toBeVisible();
  });

  it("prints the user ID and homeserver", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(await screen.findByText("@bob:example.org")).toBeVisible();
    expect(screen.getByText("https://example.org/")).toBeVisible();
  });

  it("builds the avatar initial from the name, without the sigil", async () => {
    render(
      <SignedIn
        profile={{ ...profile, display_name: null }}
        onSignedOut={vi.fn()}
      />,
    );

    // "@bob:example.org" should give "B", not "@".
    expect(await screen.findByText("B")).toBeVisible();
  });

  it("says nothing about storage when the keyring was used", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await screen.findByRole("heading", { level: 1 });

    await waitFor(() => expect(tokenStorage).toHaveBeenCalled());
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("tells the user when the token had to go in a file instead", async () => {
    tokenStorage.mockResolvedValue(fileFallback);
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    const notice = await screen.findByRole("status");

    expect(notice).toHaveTextContent(/only your user account can read/i);
  });

  it("stays usable when the storage lookup fails", async () => {
    tokenStorage.mockRejectedValue({ message: "nope", detail: "nope" });
    vi.spyOn(console, "error").mockImplementation(() => {});

    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(await screen.findByRole("heading", { level: 1 })).toBeVisible();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("signs out when the button is clicked", async () => {
    const user = userEvent.setup();
    const onSignedOut = vi.fn();
    render(<SignedIn profile={profile} onSignedOut={onSignedOut} />);

    await user.click(await screen.findByRole("button", { name: /sign(ing)? out/i }));

    await waitFor(() => expect(logout).toHaveBeenCalledOnce());
    expect(onSignedOut).toHaveBeenCalledOnce();
  });

  it("leaves the screen even when the server-side logout fails", async () => {
    // `logout` clears the local session regardless, so staying here would
    // show a signed-in screen for a session that no longer exists.
    const user = userEvent.setup();
    const onSignedOut = vi.fn();
    logout.mockRejectedValue({ message: "Could not reach it.", detail: "timeout" });
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(<SignedIn profile={profile} onSignedOut={onSignedOut} />);

    await user.click(await screen.findByRole("button", { name: /sign(ing)? out/i }));

    await waitFor(() => expect(onSignedOut).toHaveBeenCalledOnce());
  });

  it("disables the button while signing out", async () => {
    const user = userEvent.setup();
    let release: () => void = () => {};
    logout.mockReturnValue(new Promise<void>((resolve) => (release = resolve)));
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: /sign(ing)? out/i }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: /sign(ing)? out/i })).toBeDisabled(),
    );
    release();
  });

  it("logs the developer-facing detail of a failed sign-out", async () => {
    const user = userEvent.setup();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    logout.mockRejectedValue({ message: "Could not reach it.", detail: "timeout" });
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: /sign(ing)? out/i }));

    await waitFor(() =>
      expect(consoleError).toHaveBeenCalledWith(
        "logout reported an error",
        "timeout",
      ),
    );
  });

  it("does not set state after unmounting", async () => {
    // A storage lookup that resolves after the component is gone would warn
    // in React and, worse, hide a real leak behind the noise.
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    let release: (value: TokenStorage) => void = () => {};
    tokenStorage.mockReturnValue(new Promise((resolve) => (release = resolve)));

    const { unmount } = render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    unmount();
    release(fileFallback);
    await Promise.resolve();

    expect(consoleError).not.toHaveBeenCalled();
  });
});
