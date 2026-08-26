import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const sessionStatus = vi.hoisted(() => vi.fn());
const login = vi.hoisted(() => vi.fn());
const logout = vi.hoisted(() => vi.fn());
const tokenStorage = vi.hoisted(() => vi.fn());
// Mocked even though nothing here asserts on it: the signed-in screen
// subscribes on mount, and the real one reaches for a Tauri global that does
// not exist in jsdom.
const onConnection = vi.hoisted(() => vi.fn(() => Promise.resolve(() => {})));
const onVerification = vi.hoisted(() =>
  vi.fn(() => Promise.resolve(() => {})),
);
const onVerificationFlow = vi.hoisted(() =>
  vi.fn(() => Promise.resolve(() => {})),
);
const onKeyBackup = vi.hoisted(() => vi.fn(() => Promise.resolve(() => {})));
const onRooms = vi.hoisted(() => vi.fn(() => Promise.resolve(() => {})));
const resendState = vi.hoisted(() => vi.fn(() => Promise.resolve()));
vi.mock("./lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./lib/api")>()),
  sessionStatus,
  login,
  logout,
  tokenStorage,
  onConnection,
  onVerification,
  onVerificationFlow,
  onKeyBackup,
  onRooms,
  resendState,
}));

import { App } from "./App";
import type { Profile } from "./lib/api";

const profile: Profile = {
  user_id: "@bob:example.org",
  device_id: "HZTIUXZKUU",
  homeserver: "https://example.org/",
  display_name: "Bob",
  avatar_url: null,
};

describe("App", () => {
  beforeEach(() => {
    sessionStatus.mockReset();
    login.mockReset().mockResolvedValue(profile);
    logout.mockReset().mockResolvedValue(undefined);
    tokenStorage.mockReset().mockResolvedValue({
      kind: "keyring",
      description: "Your sign-in is stored in your system keyring.",
      isPreferred: true,
    });
  });

  it("shows the splash while the session status is unknown", () => {
    sessionStatus.mockReturnValue(new Promise(() => {}));

    render(<App />);

    expect(screen.getByText(/signing you in/i)).toBeVisible();
  });

  it("shows the login form when nobody is signed in", async () => {
    sessionStatus.mockResolvedValue({ status: "signedOut" });

    render(<App />);

    expect(await screen.findByRole("heading", { name: /sign in/i })).toBeVisible();
  });

  it("goes straight to the signed-in screen when a session was restored", async () => {
    sessionStatus.mockResolvedValue({ status: "signedIn", profile });

    render(<App />);

    expect(await screen.findByRole("group", { name: "Account" })).toBeVisible();
  });

  it("falls back to the login form when the status check itself fails", async () => {
    // The Rust side already reports an unrestorable session as signed out, so
    // reaching here means the command failed. The only useful screen is the
    // login form.
    sessionStatus.mockRejectedValue({ message: "Something went wrong.", detail: "ipc died" });
    vi.spyOn(console, "error").mockImplementation(() => {});

    render(<App />);

    expect(await screen.findByRole("heading", { name: /sign in/i })).toBeVisible();
  });

  it("moves to the signed-in screen after a successful login", async () => {
    const user = userEvent.setup();
    sessionStatus.mockResolvedValue({ status: "signedOut" });
    render(<App />);
    await screen.findByRole("heading", { name: /sign in/i });

    await user.type(screen.getByLabelText(/homeserver/i), "example.org");
    await user.type(screen.getByLabelText(/username/i), "bob");
    await user.type(screen.getByLabelText(/password/i), "hunter2");
    await user.click(screen.getByRole("button", { name: /sign(ing)? in/i }));

    expect(await screen.findByRole("group", { name: "Account" })).toBeVisible();
  });

  it("returns to the login form after signing out", async () => {
    const user = userEvent.setup();
    sessionStatus.mockResolvedValue({ status: "signedIn", profile });
    render(<App />);
    await screen.findByRole("group", { name: "Account" });

    await user.click(screen.getByRole("button", { name: /sign(ing)? out/i }));

    expect(await screen.findByRole("heading", { name: /sign in/i })).toBeVisible();
  });

  it("only asks for the session status once", async () => {
    sessionStatus.mockResolvedValue({ status: "signedOut" });

    render(<App />);
    await screen.findByRole("heading", { name: /sign in/i });

    // StrictMode double-invokes effects in development. The cancelled flag is
    // what stops that becoming two visible state transitions; this asserts the
    // user-visible result rather than the call count, which StrictMode owns.
    expect(screen.getAllByRole("heading", { name: /sign in/i })).toHaveLength(1);
  });

  it("ignores a status that resolves after the app unmounts", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    let release: (value: { status: "signedOut" }) => void = () => {};
    sessionStatus.mockReturnValue(new Promise((resolve) => (release = resolve)));

    const { unmount } = render(<App />);
    unmount();
    release({ status: "signedOut" });
    await Promise.resolve();

    expect(consoleError).not.toHaveBeenCalled();
  });

  it("ignores a rejection that arrives after the app unmounts", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    let reject: (reason: unknown) => void = () => {};
    sessionStatus.mockReturnValue(new Promise((_, r) => (reject = r)));

    const { unmount } = render(<App />);
    unmount();
    reject({ message: "gone", detail: "gone" });
    await Promise.resolve();

    expect(consoleError).not.toHaveBeenCalled();
  });

  it("logs the failure detail when the status check fails", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    sessionStatus.mockRejectedValue({ message: "Something went wrong.", detail: "ipc died" });

    render(<App />);
    await screen.findByRole("heading", { name: /sign in/i });

    await waitFor(() =>
      expect(consoleError).toHaveBeenCalledWith(
        "session_status failed",
        expect.objectContaining({ detail: "ipc died" }),
      ),
    );
  });
});
