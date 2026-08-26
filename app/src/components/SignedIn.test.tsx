import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const logout = vi.hoisted(() => vi.fn());
const tokenStorage = vi.hoisted(() => vi.fn());
const onConnection = vi.hoisted(() => vi.fn());
const onVerification = vi.hoisted(() => vi.fn());
const onVerificationFlow = vi.hoisted(() => vi.fn());
const onKeyBackup = vi.hoisted(() => vi.fn());
const onRooms = vi.hoisted(() => vi.fn());
const roomAvatar = vi.hoisted(() => vi.fn());
const resendState = vi.hoisted(() => vi.fn());
const verificationVerifyThisSession = vi.hoisted(() => vi.fn());
const verificationOtherSessionsExist = vi.hoisted(() => vi.fn());
const verificationRecoveryExists = vi.hoisted(() => vi.fn());
const verificationRecover = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  logout,
  tokenStorage,
  onConnection,
  onVerification,
  onVerificationFlow,
  onKeyBackup,
  onRooms,
  roomAvatar,
  resendState,
  verificationVerifyThisSession,
  verificationOtherSessionsExist,
  verificationRecoveryExists,
  verificationRecover,
}));

import { SignedIn } from "./SignedIn";
import type {
  Channel,
  Connection,
  KeyBackup,
  Profile,
  Rooms,
  Space,
  TokenStorage,
  Verification,
  VerificationFlow,
} from "../lib/api";
import { resetRoomAvatarCache } from "./RoomAvatar";

/** The handler the component registered, once it has registered one. */
function connectionHandler(): (state: Connection) => void {
  const call = onConnection.mock.calls.at(-1) as
    | [(state: Connection) => void]
    | undefined;
  if (!call) throw new Error("the component never subscribed to connection");
  return call[0];
}

/** The same, for the room list. */
function roomsHandler(): (rooms: Rooms) => void {
  const call = onRooms.mock.calls.at(-1) as [(rooms: Rooms) => void] | undefined;
  if (!call) throw new Error("the component never subscribed to rooms");
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

/** The same, for the key backup channel. */
function keyBackupHandler(): (state: KeyBackup) => void {
  const call = onKeyBackup.mock.calls.at(-1) as
    | [(state: KeyBackup) => void]
    | undefined;
  if (!call) throw new Error("the component never subscribed to key backup");
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
    weStarted: false,
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
  onKeyBackup.mockReset().mockResolvedValue(() => {});
  onRooms.mockReset().mockResolvedValue(() => {});
  roomAvatar.mockReset().mockResolvedValue(null);
  resendState.mockReset().mockResolvedValue(undefined);
  verificationVerifyThisSession.mockReset().mockResolvedValue(undefined);
  // The common case: another session is signed in, so the button is offered.
  verificationOtherSessionsExist.mockReset().mockResolvedValue(true);
  // And the other common case: nobody has set a recovery key up, so the emoji
  // route is the only one on offer. Tests about recovery say otherwise.
  verificationRecoveryExists.mockReset().mockResolvedValue(false);
  verificationRecover.mockReset().mockResolvedValue(undefined);
  resetRoomAvatarCache();
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

/**
 * The account strip, which is where the display name lives now.
 *
 * It used to be this screen's `h1`, and several tests below used that heading
 * as their "the signed-in screen has rendered" anchor. A thirty-two pixel
 * strip in a corner is not a page heading, so the anchor is the labelled group
 * instead. `within` matters: the user ID is also printed among the session
 * facts, so a bare text query would find two of it.
 */
function accountPanel(): Promise<HTMLElement> {
  return screen.findByRole("group", { name: "Account" });
}

describe("SignedIn", () => {
  beforeEach(() => {
    resetApiMocks();
  });

  it("shows the display name when the account has one", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    expect(within(await accountPanel()).getByText("Bob")).toBeVisible();
  });

  it("falls back to the user ID when there is no display name", async () => {
    render(
      <SignedIn
        profile={{ ...profile, display_name: null }}
        onSignedOut={vi.fn()}
      />,
    );

    expect(
      within(await accountPanel()).getByText("@bob:example.org"),
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

  it("falls back to a question mark when there is no letter to use", async () => {
    // A homeserver can hand back an empty display name, which is not null and
    // so does not fall through to the user ID. An avatar with nothing in it
    // reads as a rendering fault rather than as a missing name.
    render(
      <SignedIn
        profile={{ ...profile, display_name: "" }}
        onSignedOut={vi.fn()}
      />,
    );

    expect(within(await accountPanel()).getByText("?")).toBeVisible();
  });

  it("says nothing about storage when the keyring was used", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await accountPanel();

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

    expect(await accountPanel()).toBeVisible();
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
      within(await accountPanel()).getByText("Bob"),
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
      within(await accountPanel()).getByText("Bob"),
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
      within(await accountPanel()).getByText("Bob"),
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

describe("SignedIn starting a verification", () => {
  beforeEach(() => {
    resetApiMocks();
  });

  /**
   * Report the session unverified and let the banner settle.
   *
   * An async `act`, because reporting it starts the lookup for other sessions
   * and that resolves a microtask later. A synchronous one returns before then
   * and the state update lands outside it.
   */
  async function unverified() {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onVerification).toHaveBeenCalled());
    await act(async () => {
      verificationHandler()({ state: "unverified" });
    });
  }

  it("offers to verify this session when there is another one to ask", async () => {
    await unverified();

    expect(
      await screen.findByRole("button", { name: /verify this session/i }),
    ).toBeVisible();
  });

  it("asks the account's other sessions when the button is pressed", async () => {
    await unverified();
    const button = await screen.findByRole("button", {
      name: /verify this session/i,
    });

    await userEvent.click(button);

    expect(verificationVerifyThisSession).toHaveBeenCalled();
  });

  it("offers nothing to press while the session is already verified", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onVerification).toHaveBeenCalled());

    act(() => verificationHandler()({ state: "verified" }));

    await screen.findByText("This session is verified.");
    expect(
      screen.queryByRole("button", { name: /verify this session/i }),
    ).not.toBeInTheDocument();
  });

  it("says what to do instead when there is no route at all", async () => {
    // The honest answer, and it takes both halves to be true. One session on
    // an account has nobody to compare emoji with, and an account with no
    // secret storage has no key to type instead. A button that can only time
    // out is worse than a sentence saying so.
    verificationOtherSessionsExist.mockResolvedValue(false);
    verificationRecoveryExists.mockResolvedValue(false);

    await unverified();

    expect(
      await screen.findByText(/no other session is signed in and this account/i),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /verify this session/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/recovery key/i)).not.toBeInTheDocument();
  });

  it("still offers the button when it could not find out", async () => {
    // Fail open. Being wrong the other way strands somebody who does have a
    // phone signed in, and the cost of being wrong this way is one request
    // that nobody answers.
    verificationOtherSessionsExist.mockRejectedValue(
      new Error("the homeserver said no"),
    );

    await unverified();

    expect(
      await screen.findByRole("button", { name: /verify this session/i }),
    ).toBeVisible();
  });

  it("says so when the request cannot be sent", async () => {
    verificationVerifyThisSession.mockRejectedValue({
      message: "This account has no verification keys set up yet.",
      detail: "no cross-signing identity",
    });
    await unverified();

    await userEvent.click(
      await screen.findByRole("button", { name: /verify this session/i }),
    );

    expect(
      await screen.findByText(
        "This account has no verification keys set up yet.",
      ),
    ).toBeVisible();
  });

  it("does not offer to start a second one while a flow is running", async () => {
    await unverified();
    await screen.findByRole("button", { name: /verify this session/i });

    act(() => flowHandler()(aRequest("the-only-flow")));

    expect(
      screen.queryByRole("button", { name: /verify this session/i }),
    ).not.toBeInTheDocument();
  });

  it("offers it again once the flow is over", async () => {
    await unverified();
    act(() => flowHandler()(aRequest("the-only-flow")));

    act(() =>
      flowHandler()({
        ...aRequest("the-only-flow"),
        state: { kind: "cancelled", reason: "timedOut", byUs: false, detail: "" },
      }),
    );

    expect(
      await screen.findByRole("button", { name: /verify this session/i }),
    ).toBeVisible();
  });
});

describe("SignedIn key backup", () => {
  beforeEach(() => {
    resetApiMocks();
  });

  /** The notice, if it is showing. Named, because the screen has three. */
  function backupNotice(): HTMLElement | null {
    return screen.queryByRole("status", {
      name: "Whether your message keys are backed up",
    });
  }

  async function mounted() {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onKeyBackup).toHaveBeenCalled());
  }

  it("says nothing before anything has been reported", async () => {
    // The state every launch starts in. Warning here would put "your messages
    // are not safe" on screen for the moment before anything had looked.
    await mounted();

    expect(backupNotice()).not.toBeInTheDocument();
  });

  it("warns when the account has no backup at all", async () => {
    // The one case nothing else on this screen covers: no backup exists, so
    // every key this device holds dies with it.
    await mounted();

    act(() => keyBackupHandler()({ state: "missing" }));

    expect(backupNotice()).toBeVisible();
    expect(backupNotice()).toHaveTextContent(/no key backup/i);
  });

  it("says nothing when keys are going up", async () => {
    // The expected case. A third box saying everything is fine is a box
    // nobody reads.
    await mounted();

    act(() => keyBackupHandler()({ state: "enabled" }));

    expect(backupNotice()).not.toBeInTheDocument();
  });

  it("says nothing about a backup this session cannot read yet", async () => {
    // Not silence for lack of anything to say. There is a backup and
    // verifying is what opens it, which is exactly what the banner above is
    // already telling them to do.
    await mounted();

    act(() => keyBackupHandler()({ state: "unusable" }));

    expect(backupNotice()).not.toBeInTheDocument();
  });

  it("says nothing while a backup is being set up", async () => {
    await mounted();

    act(() => keyBackupHandler()({ state: "preparing" }));

    expect(backupNotice()).not.toBeInTheDocument();
  });

  it("stops warning once a backup appears", async () => {
    await mounted();
    act(() => keyBackupHandler()({ state: "missing" }));

    act(() => keyBackupHandler()({ state: "enabled" }));

    expect(backupNotice()).not.toBeInTheDocument();
  });
});

describe("SignedIn verifying with a recovery key", () => {
  beforeEach(() => {
    resetApiMocks();
    verificationRecoveryExists.mockResolvedValue(true);
  });

  /** Report the session unverified and let both lookups settle. */
  async function unverified() {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onVerification).toHaveBeenCalled());
    await act(async () => {
      verificationHandler()({ state: "unverified" });
    });
  }

  /** The key box, once the banner has decided to offer one. */
  function keyBox(): HTMLElement {
    return screen.getByLabelText(/recovery key/i);
  }

  it("offers the box when the account has a key, even with nobody to ask", async () => {
    // The whole point of the phase. Before it, this was the dead end.
    verificationOtherSessionsExist.mockResolvedValue(false);

    await unverified();

    expect(await screen.findByLabelText("Recovery key")).toBeVisible();
    expect(
      screen.queryByText(/no other session is signed in and this account/i),
    ).not.toBeInTheDocument();
  });

  it("offers both routes when both are open", async () => {
    await unverified();

    expect(
      await screen.findByRole("button", { name: /verify this session/i }),
    ).toBeVisible();
    expect(screen.getByLabelText(/or use your recovery key/i)).toBeVisible();
  });

  it("offers no box when the account has no recovery set up", async () => {
    // An input for a key that was never created sends somebody hunting
    // through a password manager for something that does not exist.
    verificationRecoveryExists.mockResolvedValue(false);

    await unverified();

    await screen.findByRole("button", { name: /verify this session/i });
    expect(screen.queryByLabelText(/recovery key/i)).not.toBeInTheDocument();
  });

  it("offers the box when it could not find out", async () => {
    // Fail open, same as counting the other sessions. Being wrong this way
    // costs one attempt and a clear answer; being wrong the other way leaves
    // a lone session with no route at all.
    verificationRecoveryExists.mockRejectedValue(
      new Error("the homeserver said no"),
    );

    await unverified();

    expect(await screen.findByLabelText(/recovery key/i)).toBeVisible();
  });

  it("will not submit an empty box", async () => {
    await unverified();
    await screen.findByLabelText(/recovery key/i);

    expect(screen.getByRole("button", { name: "Verify" })).toBeDisabled();
  });

  it("sends the key when the form is submitted", async () => {
    await unverified();
    await screen.findByLabelText(/recovery key/i);

    await userEvent.type(keyBox(), "EsTj 3yST y93F SLpB");
    await userEvent.click(screen.getByRole("button", { name: "Verify" }));

    expect(verificationRecover).toHaveBeenCalledWith("EsTj 3yST y93F SLpB");
  });

  it("clears the key once it has been used", async () => {
    // It is a secret, and a verified session has no further use for the thing
    // that verified it. Leaving it in the box leaves it on screen.
    await unverified();
    await screen.findByLabelText(/recovery key/i);

    await userEvent.type(keyBox(), "EsTj 3yST y93F SLpB");
    await userEvent.click(screen.getByRole("button", { name: "Verify" }));

    await waitFor(() => expect(keyBox()).toHaveValue(""));
  });

  it("says which of the four things went wrong", async () => {
    // The reason the Rust side distinguishes them at all. "That did not work"
    // is a bad answer to the likeliest mistake in the milestone.
    verificationRecover.mockRejectedValue({
      message: "That is not this account's recovery key.",
      detail: "that recovery key does not open this account's secret storage",
    });
    await unverified();
    await screen.findByLabelText(/recovery key/i);

    await userEvent.type(keyBox(), "a well formed wrong key");
    await userEvent.click(screen.getByRole("button", { name: "Verify" }));

    expect(
      await screen.findByText("That is not this account's recovery key."),
    ).toBeVisible();
  });

  it("keeps a rejected key in the box to be corrected", async () => {
    // Clearing it on failure means retyping 48 characters to fix one of them.
    verificationRecover.mockRejectedValue({
      message: "That is not this account's recovery key.",
      detail: "wrong key",
    });
    await unverified();
    await screen.findByLabelText(/recovery key/i);

    await userEvent.type(keyBox(), "nearly right");
    await userEvent.click(screen.getByRole("button", { name: "Verify" }));

    await screen.findByText("That is not this account's recovery key.");
    expect(keyBox()).toHaveValue("nearly right");
  });

  it("never logs the key itself", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    verificationRecover.mockRejectedValue({
      message: "That is not this account's recovery key.",
      detail: "wrong key",
    });
    await unverified();
    await screen.findByLabelText(/recovery key/i);

    await userEvent.type(keyBox(), "EsTj3ySTy93FSLpB");
    await userEvent.click(screen.getByRole("button", { name: "Verify" }));
    await screen.findByText("That is not this account's recovery key.");

    for (const call of logged.mock.calls) {
      expect(JSON.stringify(call)).not.toContain("EsTj3ySTy93FSLpB");
    }
    logged.mockRestore();
  });

  it("disables the box while the key is being checked", async () => {
    let finish = () => {};
    verificationRecover.mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    await unverified();
    await screen.findByLabelText(/recovery key/i);

    await userEvent.type(keyBox(), "EsTj 3yST y93F SLpB");
    await userEvent.click(screen.getByRole("button", { name: "Verify" }));

    expect(keyBox()).toBeDisabled();
    expect(screen.getByRole("button", { name: /checking/i })).toBeDisabled();

    await act(async () => {
      finish();
    });
  });

  it("stops offering anything once the session is verified", async () => {
    // The form reports nothing upwards on success. What removes it is the
    // verification watcher noticing the session changed, which is the same
    // event the emoji route ends on.
    await unverified();
    await screen.findByLabelText(/recovery key/i);

    act(() => verificationHandler()({ state: "verified" }));

    await screen.findByText("This session is verified.");
    expect(screen.queryByLabelText(/recovery key/i)).not.toBeInTheDocument();
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
      within(await accountPanel()).getByText("Bob"),
    ).toBeVisible();
    await waitFor(() => expect(logged).toHaveBeenCalled());
  });
});

describe("SignedIn the room list", () => {
  beforeEach(() => {
    resetApiMocks();
  });

  const general: Channel = {
    id: "!general:example.org",
    name: "general",
    kind: "text",
    avatar: null,
    joined: true,
  };
  const lounge: Channel = {
    id: "!lounge:example.org",
    name: "Lounge",
    kind: "voice",
    avatar: null,
    joined: true,
  };
  const homeSpace: Space = {
    id: "home",
    name: "Home",
    avatar: null,
    channels: [
      {
        id: "!dm:example.org",
        name: "aayejayy",
        kind: "text",
        avatar: null,
        joined: true,
      },
    ],
  };
  const kahuHq: Space = {
    id: "!s:example.org",
    name: "Kahu HQ",
    avatar: null,
    channels: [general, lounge],
  };
  const rooms: Rooms = { spaces: [homeSpace, kahuHq] };

  async function showing(tree: Rooms = rooms) {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);
    await waitFor(() => expect(onRooms).toHaveBeenCalled());
    act(() => roomsHandler()(tree));
  }

  it("draws an empty shell before the first room list arrives", async () => {
    render(<SignedIn profile={profile} onSignedOut={vi.fn()} />);

    await accountPanel();
    expect(screen.queryByRole("button", { name: "Home" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "Nothing here yet",
    );
  });

  it("opens on Home, with the rooms that belong to no space", async () => {
    await showing();

    expect(
      await screen.findByRole("button", { name: "Home" }),
    ).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("button", { name: "#aayejayy" })).toBeVisible();
  });

  it("shows a space's channels when its rail icon is clicked", async () => {
    await showing();
    await screen.findByRole("button", { name: "Kahu HQ" });

    await userEvent.click(screen.getByRole("button", { name: "Kahu HQ" }));

    expect(screen.getByRole("button", { name: "#general" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Lounge" })).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "#aayejayy" }),
    ).not.toBeInTheDocument();
  });

  it("names the selected text channel in the main pane, with its hash", async () => {
    await showing();
    await userEvent.click(await screen.findByRole("button", { name: "Kahu HQ" }));

    await userEvent.click(screen.getByRole("button", { name: "#general" }));

    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "#general",
    );
  });

  it("says something different about a voice channel", async () => {
    // The one distinction the next milestone depends on being visible.
    await showing();
    await userEvent.click(await screen.findByRole("button", { name: "Kahu HQ" }));

    await userEvent.click(screen.getByRole("button", { name: "Lounge" }));

    const heading = screen.getByRole("heading", { level: 1 });
    expect(heading).toHaveTextContent("Lounge");
    expect(heading).not.toHaveTextContent("#");
    expect(screen.getByText(/joining a voice channel/i)).toBeVisible();
  });

  it("forgets the selected channel when the space changes", async () => {
    // A channel belongs to the space it was picked in. Carrying the selection
    // across would highlight a channel in a list it is not in.
    await showing();
    await userEvent.click(await screen.findByRole("button", { name: "Kahu HQ" }));
    await userEvent.click(screen.getByRole("button", { name: "#general" }));

    await userEvent.click(screen.getByRole("button", { name: "Home" }));

    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "Nothing here yet",
    );
  });

  it("falls back to Home when the selected space is left", async () => {
    // A room list that says the space is gone arrives as a whole new tree, so
    // the selection has to survive being wrong rather than pointing at a room
    // nobody is in any more.
    await showing();
    await userEvent.click(await screen.findByRole("button", { name: "Kahu HQ" }));

    act(() => roomsHandler()({ spaces: [homeSpace] }));

    expect(
      await screen.findByRole("button", { name: "Home" }),
    ).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("button", { name: "#aayejayy" })).toBeVisible();
  });

  it("drops the selection when the selected channel is removed", async () => {
    await showing();
    await userEvent.click(await screen.findByRole("button", { name: "Kahu HQ" }));
    await userEvent.click(screen.getByRole("button", { name: "#general" }));

    act(() =>
      roomsHandler()({
        spaces: [homeSpace, { ...kahuHq, channels: [lounge] }],
      }),
    );

    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "Nothing here yet",
    );
    expect(
      screen.queryByRole("button", { name: "#general" }),
    ).not.toBeInTheDocument();
  });

  it("replaces the tree rather than merging into it", async () => {
    // The Rust side sends the whole thing every time precisely so that this
    // side never has to work out what changed.
    await showing();
    await screen.findByRole("button", { name: "Kahu HQ" });

    act(() => roomsHandler()({ spaces: [homeSpace] }));

    expect(
      screen.queryByRole("button", { name: "Kahu HQ" }),
    ).not.toBeInTheDocument();
  });
});
