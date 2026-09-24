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
const onThread = vi.hoisted(() => vi.fn(() => Promise.resolve(() => {})));
const resendState = vi.hoisted(() => vi.fn(() => Promise.resolve()));
const quit = vi.hoisted(() => vi.fn(() => Promise.resolve()));
const appearanceSettings = vi.hoisted(() => vi.fn());
const setAppearanceSettings = vi.hoisted(() => vi.fn());
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
  onThread,
  resendState,
  quit,
  appearanceSettings,
  setAppearanceSettings,
}));

import { App } from "./App";
import { onZoomed } from "./lib/scale";
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
    quit.mockClear();
    appearanceSettings
      .mockReset()
      .mockResolvedValue({ applicationScale: 1, textScale: 1 });
    setAppearanceSettings.mockReset().mockResolvedValue(undefined);
    document.documentElement.style.removeProperty("font-size");
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

    await user.click(screen.getByRole("button", { name: /user settings/i }));
    await user.click(screen.getByRole("button", { name: /log out/i }));

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

  describe("Ctrl+Q", () => {
    it("closes Consort", async () => {
      sessionStatus.mockResolvedValue({ status: "signedOut" });
      render(<App />);
      await screen.findByRole("heading", { name: /sign in/i });

      await userEvent.keyboard("{Control>}q{/Control}");

      expect(quit).toHaveBeenCalledTimes(1);
    });

    it("closes it from the splash, before anything is known", async () => {
      // A session check waiting on a homeserver that is not answering is
      // exactly when somebody wants out, and it is before the shell exists.
      sessionStatus.mockReturnValue(new Promise(() => {}));
      render(<App />);

      await userEvent.keyboard("{Control>}q{/Control}");

      expect(quit).toHaveBeenCalledTimes(1);
    });

    it("closes it from inside a text box as well", async () => {
      // Every other application on this desktop quits on Ctrl+Q wherever the
      // caret is, and a quit key that depended on focus would do nothing in
      // the one place somebody spends their time.
      sessionStatus.mockResolvedValue({ status: "signedOut" });
      render(<App />);

      await userEvent.click(await screen.findByLabelText(/username/i));
      await userEvent.keyboard("{Control>}q{/Control}");

      expect(quit).toHaveBeenCalledTimes(1);
    });

    it("leaves a bare q to whatever is being typed into", async () => {
      sessionStatus.mockResolvedValue({ status: "signedOut" });
      render(<App />);
      await screen.findByRole("heading", { name: /sign in/i });

      await userEvent.keyboard("q");

      expect(quit).not.toHaveBeenCalled();
    });

    it("stops listening once the app is gone", async () => {
      sessionStatus.mockResolvedValue({ status: "signedOut" });
      const { unmount } = render(<App />);
      await screen.findByRole("heading", { name: /sign in/i });

      unmount();
      await userEvent.keyboard("{Control>}q{/Control}");

      expect(quit).not.toHaveBeenCalled();
    });
  });

  describe("Ctrl and plus or minus", () => {
    /** One press of Ctrl and the equals key, ready to dispatch. */
    function zoomIn(): KeyboardEvent {
      return new KeyboardEvent("keydown", {
        key: "=",
        ctrlKey: true,
        cancelable: true,
        bubbles: true,
      });
    }

    /** Signed out and settled, which is the cheapest screen to test on. */
    async function opened() {
      sessionStatus.mockResolvedValue({ status: "signedOut" });
      const rendered = render(<App />);
      await screen.findByRole("heading", { name: /sign in/i });
      await waitFor(() => expect(appearanceSettings).toHaveBeenCalled());
      return rendered;
    }

    it("draws the page at the text size that was saved", async () => {
      // The other half of the size, the webview zoom, is already in place by
      // now: Rust applies it during setup, before this page has painted. This
      // is the half only the page can do.
      appearanceSettings.mockResolvedValue({
        applicationScale: 1,
        textScale: 1.25,
      });

      await opened();

      await waitFor(() =>
        expect(document.documentElement.style.fontSize).toBe("125%"),
      );
    });

    it("zooms in a step", async () => {
      await opened();

      await userEvent.keyboard("{Control>}={/Control}");

      await waitFor(() =>
        expect(setAppearanceSettings).toHaveBeenCalledWith({
          applicationScale: 1.1,
          textScale: 1,
        }),
      );
    });

    it("zooms out a step", async () => {
      await opened();

      await userEvent.keyboard("{Control>}-{/Control}");

      await waitFor(() =>
        expect(setAppearanceSettings).toHaveBeenCalledWith({
          applicationScale: 0.9,
          textScale: 1,
        }),
      );
    });

    it("goes back to the size it started at on Ctrl and zero", async () => {
      appearanceSettings.mockResolvedValue({
        applicationScale: 1.8,
        textScale: 1.2,
      });
      await opened();

      await userEvent.keyboard("{Control>}0{/Control}");

      await waitFor(() =>
        expect(setAppearanceSettings).toHaveBeenCalledWith({
          applicationScale: 1,
          // Left alone deliberately. These are two knobs, and a reset that
          // quietly moved the other one would be them becoming one.
          textScale: 1.2,
        }),
      );
    });

    it("keeps the text size somebody chose while zooming", async () => {
      // The read is fresh on every press rather than remembered, so a text
      // size changed in Settings since launch is not written back over.
      appearanceSettings.mockResolvedValue({
        applicationScale: 1,
        textScale: 1.3,
      });
      await opened();

      await userEvent.keyboard("{Control>}={/Control}");

      await waitFor(() =>
        expect(setAppearanceSettings).toHaveBeenCalledWith({
          applicationScale: 1.1,
          textScale: 1.3,
        }),
      );
    });

    it("writes nothing when it is already as large as it goes", async () => {
      appearanceSettings.mockResolvedValue({
        applicationScale: 2,
        textScale: 1,
      });
      await opened();

      await userEvent.keyboard("{Control>}={/Control}");

      await waitFor(() => expect(appearanceSettings).toHaveBeenCalledTimes(2));
      expect(setAppearanceSettings).not.toHaveBeenCalled();
    });

    it("works from inside a text box, as it does in every browser", async () => {
      await opened();

      await userEvent.click(screen.getByLabelText(/username/i));
      await userEvent.keyboard("{Control>}={/Control}");

      await waitFor(() => expect(setAppearanceSettings).toHaveBeenCalled());
    });

    it("works from the splash, before anything is known", async () => {
      // The same reason Ctrl+Q is bound here. Somebody who cannot read the
      // splash cannot read the settings screen that would fix it either.
      sessionStatus.mockReturnValue(new Promise(() => {}));
      render(<App />);
      await waitFor(() => expect(appearanceSettings).toHaveBeenCalled());

      await userEvent.keyboard("{Control>}={/Control}");

      await waitFor(() => expect(setAppearanceSettings).toHaveBeenCalled());
    });

    it("takes both presses when the key is held, rather than losing one", async () => {
      // Held down, these keys repeat faster than a round trip, so the second
      // press arrives while the first is still being written. Two presses that
      // both read the size before either has written it would both step from
      // the same place, and one of them simply would not have happened.
      //
      // Dispatched directly and without awaiting in between, because
      // `userEvent.keyboard` waits for the page to settle between keys and so
      // cannot produce the overlap this is about.
      //
      // Reads are held until they are answered by hand below, which is what
      // makes the difference visible: taken in turn, the second press has not
      // asked for the size yet, and asking later is how it sees 1.1. Answered
      // together, both would see 1 and both would write 1.1.
      const writes: number[] = [];
      const asked: Array<() => void> = [];
      appearanceSettings.mockImplementation(
        () =>
          new Promise((resolve) => {
            asked.push(() =>
              resolve({ applicationScale: writes.at(-1) ?? 1, textScale: 1 }),
            );
          }),
      );
      setAppearanceSettings.mockImplementation(
        (appearance: { applicationScale: number }) => {
          writes.push(appearance.applicationScale);
          return Promise.resolve();
        },
      );
      await opened();

      window.dispatchEvent(zoomIn());
      window.dispatchEvent(zoomIn());

      await waitFor(() => {
        for (const answer of asked.splice(0)) answer();
        expect(writes).toEqual([1.1, 1.2]);
      });
    });

    it("says so when the size could not be written", async () => {
      const consoleError = vi
        .spyOn(console, "error")
        .mockImplementation(() => {});
      setAppearanceSettings.mockRejectedValue({
        message: "Something went wrong.",
        detail: "the settings file is read-only",
      });
      await opened();

      await userEvent.keyboard("{Control>}={/Control}");

      await waitFor(() =>
        expect(consoleError).toHaveBeenCalledWith(
          "appearance_settings failed",
          expect.objectContaining({ detail: "the settings file is read-only" }),
        ),
      );
    });

    it("keeps answering presses after one of them failed", async () => {
      // The catch is inside each press rather than around the chain of them.
      // Around it, one failed write would swallow every press afterwards and
      // the keys would silently stop working for the rest of the session.
      vi.spyOn(console, "error").mockImplementation(() => {});
      setAppearanceSettings.mockRejectedValueOnce({
        message: "Something went wrong.",
        detail: "once",
      });
      await opened();

      await userEvent.keyboard("{Control>}={/Control}");
      await waitFor(() => expect(setAppearanceSettings).toHaveBeenCalledTimes(1));

      await userEvent.keyboard("{Control>}={/Control}");

      await waitFor(() =>
        expect(setAppearanceSettings).toHaveBeenCalledTimes(2),
      );
    });

    it("keeps the keystroke to itself, so WebKitGTK does not act on it too", async () => {
      // The webview has its own opinion about these combinations, and two
      // zooms per press would put the window somewhere neither meant.
      await opened();
      const press = zoomIn();

      window.dispatchEvent(press);

      expect(press.defaultPrevented).toBe(true);
    });

    it("leaves a bare minus to whatever is being typed into", async () => {
      await opened();

      await userEvent.keyboard("-");

      expect(setAppearanceSettings).not.toHaveBeenCalled();
    });

    it("stops listening once the app is gone", async () => {
      const { unmount } = await opened();

      unmount();
      await userEvent.keyboard("{Control>}={/Control}");

      expect(setAppearanceSettings).not.toHaveBeenCalled();
    });

    it("says so on the page's own channel, so an open slider can catch up", async () => {
      const heard = vi.fn();
      const stop = onZoomed(heard);
      await opened();

      await userEvent.keyboard("{Control>}={/Control}");

      await waitFor(() => expect(heard).toHaveBeenCalled());
      stop();
    });

    it("leaves the size alone when reading it fails", async () => {
      // Nothing to put in front of somebody: the window is still the size it
      // was, and it is still changeable from Settings.
      const consoleError = vi
        .spyOn(console, "error")
        .mockImplementation(() => {});
      appearanceSettings.mockRejectedValue({
        message: "Something went wrong.",
        detail: "ipc died",
      });
      sessionStatus.mockResolvedValue({ status: "signedOut" });
      render(<App />);
      await screen.findByRole("heading", { name: /sign in/i });

      await waitFor(() =>
        expect(consoleError).toHaveBeenCalledWith(
          "appearance_settings failed",
          expect.objectContaining({ detail: "ipc died" }),
        ),
      );
      expect(setAppearanceSettings).not.toHaveBeenCalled();
    });
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
