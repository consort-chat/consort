import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const listen = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import {
  asCommandError,
  login,
  logout,
  onConnection,
  onVerification,
  onKeyBackup,
  onVerificationFlow,
  resendState,
  sessionStatus,
  tokenStorage,
  type Connection,
  type KeyBackup,
  type Profile,
  type Verification,
  type VerificationFlow,
  verificationAccept,
  verificationCancel,
  verificationConfirm,
  verificationMismatch,
  verificationOtherSessionsExist,
  verificationRecover,
  verificationRecoveryExists,
  verificationStartSas,
  verificationVerifyThisSession,
} from "./api";

const flow: VerificationFlow = {
  flowId: "the-only-flow",
  otherUserId: "@bob:example.org",
  isSelfVerification: true,
  weStarted: false,
  state: { kind: "requested" },
};

const profile: Profile = {
  user_id: "@bob:example.org",
  device_id: "HZTIUXZKUU",
  homeserver: "https://example.org/",
  display_name: "Bob",
  avatar_url: null,
};

describe("command wrappers", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("calls session_status with no arguments", async () => {
    invoke.mockResolvedValue({ status: "signedOut" });

    await expect(sessionStatus()).resolves.toEqual({ status: "signedOut" });
    expect(invoke).toHaveBeenCalledWith("session_status");
  });

  it("passes the login fields under the names the Rust command expects", async () => {
    // The names are the contract. A rename on either side has to break here
    // rather than at runtime with an unhelpful deserialisation error.
    invoke.mockResolvedValue(profile);

    await login("example.org", "bob", "hunter2");

    expect(invoke).toHaveBeenCalledWith("login", {
      server: "example.org",
      username: "bob",
      password: "hunter2",
    });
  });

  it("returns the profile the login command produced", async () => {
    invoke.mockResolvedValue(profile);
    await expect(login("example.org", "bob", "hunter2")).resolves.toEqual(profile);
  });

  it("propagates a rejected login rather than swallowing it", async () => {
    invoke.mockRejectedValue({ message: "Incorrect username or password.", detail: "M_FORBIDDEN" });

    await expect(login("example.org", "bob", "wrong")).rejects.toMatchObject({
      message: "Incorrect username or password.",
    });
  });

  it("calls logout with no arguments", async () => {
    invoke.mockResolvedValue(undefined);

    await logout();

    expect(invoke).toHaveBeenCalledWith("logout");
  });

  it("calls token_storage and returns the shape the UI renders", async () => {
    invoke.mockResolvedValue({
      kind: "keyring",
      description: "Your sign-in is stored in your system keyring.",
      isPreferred: true,
    });

    await expect(tokenStorage()).resolves.toMatchObject({
      kind: "keyring",
      isPreferred: true,
    });
    expect(invoke).toHaveBeenCalledWith("token_storage");
  });
});

describe("asCommandError", () => {
  it("passes a real CommandError straight through", () => {
    const error = { message: "Could not reach that homeserver.", detail: "dns failure" };
    expect(asCommandError(error)).toBe(error);
  });

  it("wraps a thrown Error, keeping its text as the detail", () => {
    const result = asCommandError(new Error("boom"));

    expect(result.message).toBe("Something went wrong.");
    expect(result.detail).toBe("boom");
  });

  it("wraps a thrown string", () => {
    expect(asCommandError("just a string")).toEqual({
      message: "Something went wrong.",
      detail: "just a string",
    });
  });

  it("wraps null without dereferencing it", () => {
    // `typeof null === "object"`, so the null check is load-bearing and this
    // is the test that fails if someone removes it.
    expect(asCommandError(null)).toEqual({
      message: "Something went wrong.",
      detail: "null",
    });
  });

  it("wraps undefined", () => {
    expect(asCommandError(undefined).detail).toBe("undefined");
  });

  it("rejects an object whose message is not a string", () => {
    const result = asCommandError({ message: 42 });

    expect(result.message).toBe("Something went wrong.");
  });

  it("rejects an object with no message at all", () => {
    expect(asCommandError({ detail: "only a detail" }).message).toBe(
      "Something went wrong.",
    );
  });

  it("never returns a message that is empty", () => {
    for (const input of [null, undefined, 0, "", [], {}, new Error("")]) {
      expect(asCommandError(input).message.length).toBeGreaterThan(0);
    }
  });
});

describe("event subscriptions", () => {
  beforeEach(() => {
    listen.mockReset().mockResolvedValue(() => {});
  });

  it("subscribes to the channel the Rust side emits on", async () => {
    // The name is a contract with `AppEvent::CONNECTION`, and Tauri does not
    // complain about a listener for a channel nothing sends on. Getting it
    // wrong is silence, not an error.
    await onConnection(vi.fn());

    expect(listen).toHaveBeenCalledWith("connection", expect.any(Function));
  });

  it("hands the handler the payload rather than the event envelope", async () => {
    const handler = vi.fn();
    await onConnection(handler);
    const [, forward] = listen.mock.calls[0] as [
      string,
      (event: { payload: Connection }) => void,
    ];

    forward({ payload: { state: "live" } });

    expect(handler).toHaveBeenCalledWith({ state: "live" });
  });

  it("returns the unlisten function so an effect can clean up after itself", async () => {
    // A listener leaked across a sign out and a sign in shows up as every
    // event arriving twice, which is easy to ship and unpleasant to find.
    const unlisten = vi.fn();
    listen.mockResolvedValue(unlisten);

    const returned = await onConnection(vi.fn());
    returned();

    expect(unlisten).toHaveBeenCalled();
  });

  it("subscribes to the verification channel by the name Rust emits on", async () => {
    await onVerification(vi.fn());

    expect(listen).toHaveBeenCalledWith("verification", expect.any(Function));
  });

  it("hands the verification handler the payload rather than the envelope", async () => {
    const handler = vi.fn();
    await onVerification(handler);
    const [, forward] = listen.mock.calls[0] as [
      string,
      (event: { payload: Verification }) => void,
    ];

    forward({ payload: { state: "unverified" } });

    expect(handler).toHaveBeenCalledWith({ state: "unverified" });
  });

  it("returns the verification unlisten function too", async () => {
    const unlisten = vi.fn();
    listen.mockResolvedValue(unlisten);

    const returned = await onVerification(vi.fn());
    returned();

    expect(unlisten).toHaveBeenCalled();
  });

  it("subscribes to the verification-flow channel by the name Rust emits on", async () => {
    await onVerificationFlow(vi.fn());

    expect(listen).toHaveBeenCalledWith("verification-flow", expect.any(Function));
  });

  it("hands the flow handler the payload rather than the envelope", async () => {
    const handler = vi.fn();
    await onVerificationFlow(handler);
    const [, forward] = listen.mock.calls[0] as [
      string,
      (event: { payload: VerificationFlow }) => void,
    ];

    forward({ payload: flow });

    expect(handler).toHaveBeenCalledWith(flow);
  });

  it("returns the flow unlisten function too", async () => {
    const unlisten = vi.fn();
    listen.mockResolvedValue(unlisten);

    const returned = await onVerificationFlow(vi.fn());
    returned();

    expect(unlisten).toHaveBeenCalled();
  });

  it("subscribes to the key-backup channel by the name Rust emits on", async () => {
    await onKeyBackup(vi.fn());

    expect(listen).toHaveBeenCalledWith("key-backup", expect.any(Function));
  });

  it("hands the key backup handler the payload rather than the envelope", async () => {
    const handler = vi.fn();
    await onKeyBackup(handler);
    const [, forward] = listen.mock.calls[0] as [
      string,
      (event: { payload: KeyBackup }) => void,
    ];

    forward({ payload: { state: "missing" } });

    expect(handler).toHaveBeenCalledWith({ state: "missing" });
  });

  it("returns the key backup unlisten function too", async () => {
    const unlisten = vi.fn();
    listen.mockResolvedValue(unlisten);

    const returned = await onKeyBackup(vi.fn());
    returned();

    expect(unlisten).toHaveBeenCalled();
  });

  it("names the flow on every verification action", async () => {
    // Every one of them takes the same pair, because nothing on this side
    // holds a flow: the identifiers from the event are the address.
    const actions = [
      [verificationAccept, "verification_accept"],
      [verificationStartSas, "verification_start_sas"],
      [verificationConfirm, "verification_confirm"],
      [verificationMismatch, "verification_mismatch"],
      [verificationCancel, "verification_cancel"],
    ] as const;

    for (const [call, command] of actions) {
      invoke.mockReset().mockResolvedValue(undefined);

      await call("@bob:example.org", "the-only-flow");

      expect(invoke).toHaveBeenCalledWith(command, {
        userId: "@bob:example.org",
        flowId: "the-only-flow",
      });
    }
  });

  it("asks to verify this session with no arguments", async () => {
    // Nothing for the webview to name: it is always this session asking, and
    // always the account's own identity being asked.
    invoke.mockReset().mockResolvedValue(undefined);

    await verificationVerifyThisSession();

    expect(invoke).toHaveBeenCalledWith("verification_verify_this_session");
  });

  it("returns whether there is another session to verify against", async () => {
    invoke.mockReset().mockResolvedValue(true);

    await expect(verificationOtherSessionsExist()).resolves.toBe(true);
    expect(invoke).toHaveBeenCalledWith("verification_other_sessions_exist");
  });

  it("returns whether the account has a recovery key to ask for", async () => {
    invoke.mockReset().mockResolvedValue(false);

    await expect(verificationRecoveryExists()).resolves.toBe(false);
    expect(invoke).toHaveBeenCalledWith("verification_recovery_exists");
  });

  it("passes the recovery key under the name the Rust command expects", async () => {
    invoke.mockReset().mockResolvedValue(undefined);

    await verificationRecover("EsTj 3yST y93F SLpB");

    expect(invoke).toHaveBeenCalledWith("verification_recover", {
      recoveryKey: "EsTj 3yST y93F SLpB",
    });
  });

  it("asks to be caught up with no arguments", async () => {
    invoke.mockReset().mockResolvedValue(undefined);

    await resendState();

    expect(invoke).toHaveBeenCalledWith("resend_state");
  });
});
