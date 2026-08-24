import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const logout = vi.hoisted(() => vi.fn());
const tokenStorage = vi.hoisted(() => vi.fn());
const onConnection = vi.hoisted(() => vi.fn());
const onVerification = vi.hoisted(() => vi.fn());
const onVerificationFlow = vi.hoisted(() => vi.fn());
const resendState = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  logout,
  tokenStorage,
  onConnection,
  onVerification,
  onVerificationFlow,
  resendState,
}));

import { SignedIn } from "./SignedIn";
import type {
  Connection,
  Profile,
  TokenStorage,
  Verification,
  VerificationFlow,
} from "../lib/api";

/** The handler the component registered, once it has registered one. */
function connectionHandler(): (state: Connection) => void {
  const call = onConnection.mock.calls.at(-1) as
    | [(state: Connection) => void]
    | undefined;
  if (!call) throw new Error("the component never subscribed to connection");
  return call[0];
}

/** The same, for the verification channel. */
function verificationHandler(): (state: Verification) => void {
  const call = onVerification.mock.calls.at(-1) as
    | [(state: Verification) => void]
    | undefined;
  if (!call) throw new Error("the component never subscribed to verification");
  return call[0];
}

/** The same, for the flow channel. */
function flowHandler(): (flow: VerificationFlow) => void {
  const call = onVerificationFlow.mock.calls.at(-1) as
    | [(flow: VerificationFlow) => void]
    | undefined;
  if (!call) throw new Error("the component never subscribed to flows");
  return call[0];
}

/** A request waiting for an answer, on the flow it names. */
function aRequest(flowId: string): VerificationFlow {
  return {
    flowId,
    otherUserId: "@bob:example.org",
    isSelfVerification: true,
    state: { kind: "requested" },
  };
}

/**
 * The token-storage notice, if it is showing.
 *
 * Named rather than found by role alone: the verification banner is a status
 * region too, so a bare `queryByRole("status")` would find whichever came
 * first and pass for the wrong reason.
 */
function storageNotice(): HTMLElement | null {
  return screen.queryByRole("status", {
    name: "Where your sign-in is stored",
  });
}

/**
 * Put every mocked API call back to a working default.
 *
 * One helper rather than a copy per `describe`. The copies drifted the moment
 * a fourth channel arrived: the two that had not been updated handed the
 * component `undefined` where a promise belonged, and every test in them
 * failed for a reason that had nothing to do with what they were testing.
 */
function resetApiMocks() {
  logout.mockReset().mockResolvedValue(undefined);
  tokenStorage.mockReset().mockResolvedValue(keyring);
  onConnection.mockReset().mockResolvedValue(() => {});
  onVerification.mockReset().mockResolvedValue(() => {});
  onVerificationFlow.mockReset().mockResolvedValue(() => {});
  resendState.mockReset().mockResolvedValue(undefined);
}

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
    resetApiMocks();
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
    expect(storageNotice()).not.toBeInTheDocument();
  });

  it("tells the user when the token had to go in a file instead", async () => {
    tokenStorage.mockResolvedValue(fileFallback);
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    const notice = await screen.findByRole("status", {
      name: "Where your sign-in is stored",
    });

    expect(notice).toHaveTextContent(/only your user account can read/i);
  });

  it("stays usable when the storage lookup fails", async () => {
    tokenStorage.mockRejectedValue({ message: "nope", detail: "nope" });
    vi.spyOn(console, "error").mockImplementation(() => {});

    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(await screen.findByRole("heading", { level: 1 })).toBeVisible();
    expect(storageNotice()).not.toBeInTheDocument();
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

describe("SignedIn connection state", () => {
  beforeEach(() => {
    resetApiMocks();
  });

  it("says it is connecting before the sync loop has reported anything", async () => {
    // The header used to be the string "Connected", written into the markup.
    // It said so while signed out of a homeserver that had been down for an
    // hour, which is the whole reason this state exists.
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(await screen.findByText("Connecting")).toBeVisible();
  });

  it("says it is connected once the sync loop is live", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onConnection).toHaveBeenCalled());

    act(() => connectionHandler()({ state: "live" }));

    expect(await screen.findByText("Connected")).toBeVisible();
  });

  it("says it is reconnecting while the loop is retrying", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onConnection).toHaveBeenCalled());

    act(() =>
      connectionHandler()({ state: "offline", attempt: 2, retryInSeconds: 4 }),
    );

    expect(await screen.findByText("Reconnecting")).toBeVisible();
    expect(screen.queryByText("Connected")).not.toBeInTheDocument();
  });

  it("says the session ended when the homeserver rejected the token", async () => {
    // Distinct from any other stop, because it is the one the user has to do
    // something about.
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onConnection).toHaveBeenCalled());

    act(() =>
      connectionHandler()({ state: "stopped", reason: "sessionEnded" }),
    );

    expect(await screen.findByText("Session ended")).toBeVisible();
  });

  it("says it is disconnected when the loop stopped for any other reason", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onConnection).toHaveBeenCalled());

    act(() => connectionHandler()({ state: "stopped", reason: "failed" }));

    expect(await screen.findByText("Disconnected")).toBeVisible();
  });

  it("stays usable and logs when the subscription itself fails", async () => {
    // The rejection has to be handled where the subscription is made, not
    // only in the cleanup. Handling it only on unmount leaves an unhandled
    // rejection for the whole time the screen is open, which in a Tauri
    // webview is a console full of noise and in a test run is a failure with
    // nobody's name on it.
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    onConnection.mockRejectedValue(new Error("no IPC here"));

    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(
      await screen.findByRole("heading", { level: 1, name: "Bob" }),
    ).toBeVisible();
    await waitFor(() => expect(logged).toHaveBeenCalled());
    expect(screen.getByText("Connecting")).toBeVisible();
  });

  it("stops listening when it unmounts", async () => {
    const unlisten = vi.fn();
    onConnection.mockResolvedValue(unlisten);
    const { unmount } = render(
      <SignedIn profile={profile} onSignedOut={vi.fn()} />,
    );
    await waitFor(() => expect(onConnection).toHaveBeenCalled());

    unmount();

    await waitFor(() => expect(unlisten).toHaveBeenCalled());
  });

  it("stops listening even when it unmounts before the subscription resolves", async () => {
    // Signing out immediately after signing in. The promise resolves into a
    // component that is already gone, and without handling it the listener
    // stays registered for the life of the process.
    const unlisten = vi.fn();
    let resolve: (value: () => void) => void = () => {};
    onConnection.mockReturnValue(
      new Promise<() => void>((r) => {
        resolve = r;
      }),
    );
    const { unmount } = render(
      <SignedIn profile={profile} onSignedOut={vi.fn()} />,
    );

    unmount();
    resolve(unlisten);

    await waitFor(() => expect(unlisten).toHaveBeenCalled());
  });
});

describe("SignedIn verification state", () => {
  beforeEach(() => {
    resetApiMocks();
  });

  it("says it is still checking before anything has been reported", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(
      await screen.findByText("Checking whether this session is verified."),
    ).toBeVisible();
  });

  it("does not claim the session is verified while the state is unknown", async () => {
    // The reason the state has three values instead of two. Rendering "not
    // known yet" as verified tells somebody their messages are safe before
    // anything has checked.
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onVerification).toHaveBeenCalled());

    act(() => verificationHandler()({ state: "unknown" }));

    expect(
      screen.queryByText("This session is verified."),
    ).not.toBeInTheDocument();
  });

  it("says the session is not verified, and what that costs", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onVerification).toHaveBeenCalled());

    act(() => verificationHandler()({ state: "unverified" }));

    expect(
      await screen.findByText("This session is not verified."),
    ).toBeVisible();
    expect(screen.getByText(/encrypted calls/i)).toBeVisible();
  });

  it("says the session is verified once it is", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onVerification).toHaveBeenCalled());

    act(() => verificationHandler()({ state: "verified" }));

    expect(await screen.findByText("This session is verified.")).toBeVisible();
    expect(
      screen.queryByText("This session is not verified."),
    ).not.toBeInTheDocument();
  });

  it("follows the state back down if the session stops being verified", async () => {
    // Cross-signing can be reset from another client, and the SDK reports it
    // with no user action here. A banner that only ever improves would keep
    // claiming a session is verified after it is not.
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onVerification).toHaveBeenCalled());

    act(() => verificationHandler()({ state: "verified" }));
    act(() => verificationHandler()({ state: "unverified" }));

    expect(
      await screen.findByText("This session is not verified."),
    ).toBeVisible();
  });

  it("asks to be caught up, and not before it is listening", async () => {
    // The race the whole resend exists for. Asking first would be answered
    // into the void, which is exactly the bug.
    let attached = false;
    onVerification.mockImplementation(() => {
      attached = true;
      return Promise.resolve(() => {});
    });
    resendState.mockImplementation(() => {
      expect(attached).toBe(true);
      return Promise.resolve();
    });

    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    await waitFor(() => expect(resendState).toHaveBeenCalledTimes(1));
  });

  it("does not ask to be caught up if it unmounted while subscribing", async () => {
    let resolve: (value: () => void) => void = () => {};
    onVerification.mockReturnValue(
      new Promise<() => void>((r) => {
        resolve = r;
      }),
    );
    const { unmount } = render(
      <SignedIn profile={profile} onSignedOut={vi.fn()} />,
    );

    unmount();
    resolve(() => {});

    await waitFor(() => expect(onVerification).toHaveBeenCalled());
    expect(resendState).not.toHaveBeenCalled();
  });

  it("stays usable and logs when the verification subscription fails", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    onVerification.mockRejectedValue(new Error("no IPC here"));

    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(
      await screen.findByRole("heading", { level: 1, name: "Bob" }),
    ).toBeVisible();
    await waitFor(() => expect(logged).toHaveBeenCalled());
    expect(
      screen.getByText("Checking whether this session is verified."),
    ).toBeVisible();
  });

  it("stays usable and logs when asking to be caught up fails", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    resendState.mockRejectedValue(new Error("no IPC here"));

    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(
      await screen.findByRole("heading", { level: 1, name: "Bob" }),
    ).toBeVisible();
    await waitFor(() => expect(logged).toHaveBeenCalled());
  });

  it("stops listening to every channel when it unmounts", async () => {
    const stopConnection = vi.fn();
    const stopVerification = vi.fn();
    const stopFlows = vi.fn();
    onConnection.mockResolvedValue(stopConnection);
    onVerification.mockResolvedValue(stopVerification);
    onVerificationFlow.mockResolvedValue(stopFlows);
    const { unmount } = render(
      <SignedIn profile={profile} onSignedOut={vi.fn()} />,
    );
    await waitFor(() => expect(onVerificationFlow).toHaveBeenCalled());

    unmount();

    await waitFor(() => {
      expect(stopConnection).toHaveBeenCalled();
      expect(stopVerification).toHaveBeenCalled();
      expect(stopFlows).toHaveBeenCalled();
    });
  });
});

describe("SignedIn verification requests", () => {
  beforeEach(() => {
    resetApiMocks();
  });

  /** Render, wait for the subscription, and hand back the flow handler. */
  async function mounted() {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onVerificationFlow).toHaveBeenCalled());
    return flowHandler();
  }

  function panels(): HTMLElement[] {
    return screen.queryAllByRole("status", {
      name: "Session verification request",
    });
  }

  it("shows nothing until a request arrives", async () => {
    await mounted();

    expect(panels()).toHaveLength(0);
  });

  it("shows a request that arrives", async () => {
    const handle = await mounted();

    act(() => handle(aRequest("the-only-flow")));

    expect(panels()).toHaveLength(1);
    expect(panels()[0]).toHaveTextContent(/wants to verify this one/i);
  });

  it("replaces a flow with its own later state rather than stacking them", async () => {
    const handle = await mounted();
    act(() => handle(aRequest("the-only-flow")));

    act(() =>
      handle({ ...aRequest("the-only-flow"), state: { kind: "confirmed" } }),
    );

    expect(panels()).toHaveLength(1);
    expect(panels()[0]).toHaveTextContent(/waiting for the other session/i);
  });

  it("shows two concurrent requests separately", async () => {
    // A request goes to every device on the account, and there is nothing
    // stopping two arriving. Keeping one slot would silently drop the second,
    // leaving somebody waiting on a device that will never answer.
    const handle = await mounted();

    act(() => handle(aRequest("first")));
    act(() => handle(aRequest("second")));

    expect(panels()).toHaveLength(2);
  });

  it("keeps the other one when a finished flow is dismissed", async () => {
    const handle = await mounted();
    act(() => handle(aRequest("first")));
    act(() => handle({ ...aRequest("second"), state: { kind: "done" } }));

    await userEvent.click(screen.getByRole("button", { name: /dismiss/i }));

    expect(panels()).toHaveLength(1);
    expect(panels()[0]).toHaveTextContent(/wants to verify this one/i);
  });

  it("stays usable and logs when the flow subscription fails", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    onVerificationFlow.mockRejectedValue(new Error("no IPC here"));

    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(
      await screen.findByRole("heading", { level: 1, name: "Bob" }),
    ).toBeVisible();
    await waitFor(() => expect(logged).toHaveBeenCalled());
  });
});
