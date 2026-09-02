import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const listen = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import {
  asCommandError,
  audioDevices,
  mediaUrl,
  callConnect,
  callDisconnect,
  callRoomId,
  callSetDeafened,
  callSetMuted,
  onCall,
  onSelfAudio,
  HEARING,
  audioSettings,
  audioTestStart,
  audioTestStop,
  audioTonePlay,
  audioToneStop,
  login,
  logout,
  onConnection,
  onVerification,
  onKeyBackup,
  onRooms,
  onVerificationFlow,
  onAudio,
  onThread,
  threadOpen,
  resendState,
  roomAvatar,
  setAudioSettings,
  sessionStatus,
  tokenStorage,
  type AudioActivity,
  type AudioDeviceReport,
  type Call,
  type SelfAudio,
  type AudioSettings,
  type Connection,
  type KeyBackup,
  type Profile,
  type Rooms,
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

const rooms: Rooms = {
  spaces: [
    { id: "home", name: "Home", avatar: null, channels: [] },
    {
      id: "!space:example.org",
      name: "Kahu HQ",
      avatar: "mxc://example.org/abc",
      channels: [
        {
          id: "!lounge:example.org",
          name: "Lounge",
          kind: "voice",
          avatar: null,
          joined: true,
          participants: [],
        },
        {
          id: "!unknown:example.org",
          name: null,
          kind: "text",
          avatar: null,
          joined: false,
          participants: [],
        },
      ],
    },
  ],
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

  it("passes the room id under the name the Rust command expects", async () => {
    // camelCase here, snake_case there. Tauri converts, and getting it wrong
    // is a deserialisation error at runtime rather than a compile failure.
    invoke.mockResolvedValue(null);

    await roomAvatar("!general:example.org");

    expect(invoke).toHaveBeenCalledWith("room_avatar", {
      roomId: "!general:example.org",
    });
  });

  it("returns null for a room with no avatar rather than throwing", async () => {
    invoke.mockResolvedValue(null);

    await expect(roomAvatar("home")).resolves.toBeNull();
  });

  it("returns the data url for a room that has one", async () => {
    invoke.mockResolvedValue("data:image/png;base64,AAAA");

    await expect(roomAvatar("!general:example.org")).resolves.toBe(
      "data:image/png;base64,AAAA",
    );
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

describe("threads", () => {
  beforeEach(() => {
    invoke.mockReset().mockResolvedValue(undefined);
  });

  it("names the root it wants opened", async () => {
    await threadOpen("$root:example.org");

    expect(invoke).toHaveBeenCalledWith("thread_open", {
      rootId: "$root:example.org",
    });
  });

  it("shuts the panel by asking for no root at all", async () => {
    // Rather than a second command. There is one thing open at a time and
    // this says which, so `null` is the honest way to say none.
    await threadOpen(null);

    expect(invoke).toHaveBeenCalledWith("thread_open", { rootId: null });
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

  it("subscribes to the thread channel the Rust side emits on", async () => {
    await onThread(vi.fn());

    expect(listen).toHaveBeenCalledWith("thread", expect.any(Function));
  });

  it("hands a shut panel through as null rather than dropping it", async () => {
    // The panel is drawn from this, so a shut one has to arrive. Swallowing
    // the null would leave the last thread on screen after its room closed.
    const handler = vi.fn();
    await onThread(handler);
    const [, forward] = listen.mock.calls[0] as [
      string,
      (event: { payload: unknown }) => void,
    ];

    forward({ payload: null });

    expect(handler).toHaveBeenCalledWith(null);
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

  it("subscribes to the rooms channel by the name Rust emits on", async () => {
    await onRooms(vi.fn());

    expect(listen).toHaveBeenCalledWith("rooms", expect.any(Function));
  });

  it("hands the rooms handler the whole tree rather than the envelope", async () => {
    const handler = vi.fn();
    await onRooms(handler);
    const [, forward] = listen.mock.calls[0] as [
      string,
      (event: { payload: Rooms }) => void,
    ];

    forward({ payload: rooms });

    expect(handler).toHaveBeenCalledWith(rooms);
  });

  it("returns the rooms unlisten function too", async () => {
    const unlisten = vi.fn();
    listen.mockResolvedValue(unlisten);

    const returned = await onRooms(vi.fn());
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

describe("the audio commands", () => {
  const report: AudioDeviceReport = {
    input: {
      devices: [{ name: "Yeti", isDefault: true }],
      selected: "Yeti",
      missing: null,
    },
    output: {
      devices: [{ name: "Headphones", isDefault: true }],
      selected: "Headphones",
      missing: null,
    },
  };

  const settings: AudioSettings = {
    input: null,
    output: null,
    gate: {
      openAt: 0.6,
      closeAt: 0.3,
      attackFrames: 2,
      holdMs: 300,
      denoise: true,
      voiceActivity: true,
    },
  };

  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
  });

  it("asks for the device list with no arguments", async () => {
    invoke.mockResolvedValue(report);

    await expect(audioDevices()).resolves.toEqual(report);
    expect(invoke).toHaveBeenCalledWith("audio_devices");
  });

  it("asks for the saved settings with no arguments", async () => {
    invoke.mockResolvedValue(settings);

    await expect(audioSettings()).resolves.toEqual(settings);
    expect(invoke).toHaveBeenCalledWith("audio_settings");
  });

  it("passes settings under the name the Rust command expects", async () => {
    invoke.mockResolvedValue(undefined);

    await setAudioSettings(settings);

    expect(invoke).toHaveBeenCalledWith("set_audio_settings", { audio: settings });
  });

  it("starts and stops the microphone test with no arguments", async () => {
    invoke.mockResolvedValue(undefined);

    await audioTestStart();
    await audioTestStop();

    expect(invoke).toHaveBeenNthCalledWith(1, "audio_test_start");
    expect(invoke).toHaveBeenNthCalledWith(2, "audio_test_stop");
  });

  it("plays and stops the test tone with no arguments", async () => {
    // No arguments for the same reason as the microphone test: the Rust side
    // reads the saved output and resolves it against what is plugged in, so a
    // device that has gone falls back rather than refusing.
    invoke.mockResolvedValue(undefined);

    await audioTonePlay();
    await audioToneStop();

    expect(invoke).toHaveBeenNthCalledWith(1, "audio_tone_play");
    expect(invoke).toHaveBeenNthCalledWith(2, "audio_tone_stop");
  });

  it("hands the audio payload to the handler unwrapped", async () => {
    // The same contract as every other listener here: what arrives is the
    // event's payload, not the Tauri envelope around it.
    const seen: AudioActivity[] = [];
    listen.mockImplementation(
      (_name: string, handler: (event: { payload: AudioActivity }) => void) => {
        handler({ payload: { state: "level", level: 0.5, probability: 0.9, open: true } });
        return Promise.resolve(() => {});
      },
    );

    await onAudio((activity) => seen.push(activity));

    expect(listen).toHaveBeenCalledWith("audio", expect.any(Function));
    expect(seen).toEqual([
      { state: "level", level: 0.5, probability: 0.9, open: true },
    ]);
  });
});

describe("the call commands", () => {
  const LOUNGE = "!lounge:example.org";

  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
  });

  it("passes the room under the name the Rust command expects", async () => {
    // The Rust parameter is `room_id`, which Tauri matches against camel case.
    // A mismatch here is a command that always fails at the boundary.
    invoke.mockResolvedValue(undefined);

    await callConnect(LOUNGE);

    expect(invoke).toHaveBeenCalledWith("call_connect", { roomId: LOUNGE });
  });

  it("leaves with no arguments", async () => {
    invoke.mockResolvedValue(undefined);

    await callDisconnect();

    expect(invoke).toHaveBeenCalledWith("call_disconnect");
  });

  it("hands the call payload to the handler unwrapped", async () => {
    // Including the roster, which rides on the state rather than a channel of
    // its own so that the two cannot arrive out of step.
    const connected: Call = {
      state: "connected",
      roomId: LOUNGE,
      participants: [{ id: "@ada:example.org", name: "Ada" }],
      trouble: null,
    };
    const seen: Call[] = [];
    listen.mockImplementation(
      (_name: string, handler: (event: { payload: Call }) => void) => {
        handler({ payload: connected });
        return Promise.resolve(() => {});
      },
    );

    await onCall((call) => seen.push(call));

    expect(listen).toHaveBeenCalledWith("call", expect.any(Function));
    expect(seen).toEqual([connected]);
  });

  it("does not share a channel with the sync loop", async () => {
    // Both have a state that means "connected" and they answer entirely
    // different questions. One channel for the two would make either
    // unreadable.
    listen.mockResolvedValue(() => {});

    await onCall(() => {});
    await onConnection(() => {});

    expect(listen).toHaveBeenNthCalledWith(1, "call", expect.any(Function));
    expect(listen).toHaveBeenNthCalledWith(2, "connection", expect.any(Function));
  });

  it("asks to mute and to unmute by the same command", async () => {
    invoke.mockResolvedValue(undefined);

    await callSetMuted(true);
    await callSetMuted(false);

    expect(invoke).toHaveBeenNthCalledWith(1, "call_set_muted", { muted: true });
    expect(invoke).toHaveBeenNthCalledWith(2, "call_set_muted", { muted: false });
  });

  it("asks to deafen and to undeafen by the same command", async () => {
    invoke.mockResolvedValue(undefined);

    await callSetDeafened(true);

    expect(invoke).toHaveBeenCalledWith("call_set_deafened", { deafened: true });
  });

  it("does not put mute on the channel the call is on", async () => {
    // Only the last event per channel is replayed to a webview that reloaded.
    // Sharing would mean a mute evicting the call it was pressed during, and a
    // client coming back believing it is in no channel while it is publishing
    // one.
    listen.mockResolvedValue(() => {});

    await onCall(() => {});
    await onSelfAudio(() => {});

    expect(listen).toHaveBeenNthCalledWith(1, "call", expect.any(Function));
    expect(listen).toHaveBeenNthCalledWith(
      2,
      "self-audio",
      expect.any(Function),
    );
  });

  it("hands the mute state to the handler unwrapped", async () => {
    const seen: SelfAudio[] = [];
    listen.mockImplementation(
      (_channel: string, handler: (event: { payload: SelfAudio }) => void) => {
        handler({ payload: { muted: true, deafened: true } });
        return Promise.resolve(() => {});
      },
    );

    await onSelfAudio((audio) => seen.push(audio));

    expect(seen).toEqual([{ muted: true, deafened: true }]);
  });

  it("starts where Rust starts", () => {
    // The two never exchange an opening value: nothing is emitted until
    // something changes. Agreeing on the default is what makes silence mean
    // "neither" rather than "not known yet".
    expect(HEARING).toEqual({ muted: false, deafened: false, away: false });
  });

  it("reads the room out of every state that has one", () => {
    expect(callRoomId({ state: "connecting", roomId: LOUNGE })).toBe(LOUNGE);
    expect(
      callRoomId({
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    }),
    ).toBe(LOUNGE);
    expect(callRoomId({ state: "failed", roomId: LOUNGE, error: "no" })).toBe(
      LOUNGE,
    );
    expect(callRoomId({ state: "disconnected" })).toBeNull();
  });

  it("addresses an attachment at the path Rust decodes", () => {
    // The literal is the contract, and `media.rs` has the same one in a test
    // reading it back. Changing either encoding has to fail on both sides
    // rather than becoming a 400 for every attachment in every room.
    expect(
      mediaUrl('{"url":"mxc://example.org/abc","key":{"k":"a+b/c"}}'),
    ).toBe(
      "consortmedia://localhost/eyJ1cmwiOiJteGM6Ly9leGFtcGxlLm9yZy9hYmMiLCJrZXkiOnsiayI6ImErYi9jIn19",
    );
  });

  it("uses no character a path would have to escape", () => {
    const url = mediaUrl('{"url":"mxc://example.org/a?b&c"}');

    expect(url.slice("consortmedia://localhost/".length)).toMatch(
      /^[A-Za-z0-9_-]+$/,
    );
  });
});
