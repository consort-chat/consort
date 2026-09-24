import { beforeEach, describe, expect, it, vi } from "vitest";
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";

import tauriConfig from "../../src-tauri/tauri.conf.json";

const invoke = vi.hoisted(() => vi.fn());

// Only `invoke` is faked. `convertFileSrc` is kept because it is not IPC: it
// builds a string, and which string it builds is a platform difference this
// file has tests for. It reads the stand-in runtime `test/setup.ts` installs.
vi.mock("@tauri-apps/api/core", async (actual) => ({
  ...(await actual<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

const listen = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/event", () => ({ listen }));

import {
  asCommandError,
  attachPasted,
  attachFile,
  audioDevices,
  pasteAttachment,
  onDropped,
  pickAttachment,
  mediaUrl,
  mxcUrl,
  callConnect,
  callDisconnect,
  callRoomId,
  callSetAway,
  callSetDeafened,
  callSetMuted,
  onCallReadiness,
  onCall,
  onSelfAudio,
  HEARING,
  audioSettings,
  audioTestStart,
  audioTestStop,
  audioTonePlay,
  audioToneStop,
  audioMonitorStart,
  audioMonitorStop,
  setPersonVolume,
  login,
  logout,
  onConnection,
  onVerification,
  onKeyBackup,
  onRooms,
  onShowRoom,
  onVerificationFlow,
  onAudio,
  onThread,
  onTyping,
  threadOpen,
  threadSend,
  openLink,
  roomAt,
  directRoom,
  memberNames,
  roomMembers,
  onTimeline,
  timelineOpen,
  timelineClose,
  timelineEarlier,
  timelineLater,
  timelinePresent,
  timelineSend,
  saveAttachment,
  NO_TIMELINE,
  timelineCopyLink,
  timelineGoTo,
  timelineEdit,
  timelineReact,
  timelineReply,
  timelineMarkRead,
  timelineTyping,
  timelineUnreact,
  privacySettings,
  setPrivacySettings,
  notificationSettings,
  setNotificationSettings,
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
  type CallReadiness,
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
          unread: 0,
          mentions: 0,
        },
        {
          id: "!unknown:example.org",
          name: null,
          kind: "text",
          avatar: null,
          joined: false,
          participants: [],
          unread: 0,
          mentions: 0,
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

  it("names the thread a reply belongs in and what it is answering", async () => {
    await threadSend(
      "!general:example.org",
      "$root:example.org",
      "$last:example.org",
      null,
      "Consort",
    );

    expect(invoke).toHaveBeenCalledWith("thread_send", {
      roomId: "!general:example.org",
      rootId: "$root:example.org",
      inReplyTo: "$last:example.org",
      answering: null,
      body: "Consort",
    });
  });

  it("names the author when a reply is answering one message rather than the last", async () => {
    // Who wrote it is what turns the fallback into a real answer in Rust, so
    // a reply that carried the address without the name would be drawn as one
    // more line at the bottom of the thread.
    await threadSend(
      "!general:example.org",
      "$root:example.org",
      "$said:example.org",
      "@ada:example.org",
      "quite",
    );

    expect(invoke).toHaveBeenCalledWith("thread_send", {
      roomId: "!general:example.org",
      rootId: "$root:example.org",
      inReplyTo: "$said:example.org",
      answering: "@ada:example.org",
      body: "quite",
    });
  });

  it("shuts the panel by asking for no root at all", async () => {
    // Rather than a second command. There is one thing open at a time and
    // this says which, so `null` is the honest way to say none.
    await threadOpen(null);

    expect(invoke).toHaveBeenCalledWith("thread_open", { rootId: null });
  });
});

describe("replies and links", () => {
  beforeEach(() => {
    invoke.mockReset().mockResolvedValue(undefined);
  });

  it("names what a reply answers and who wrote it", async () => {
    // The sender is not decoration. It is what the reply names in
    // `m.mentions`, which is what makes an answer arrive as something.
    await timelineReply(
      "!general:example.org",
      "$said:example.org",
      "@ada:example.org",
      "quite",
    );

    expect(invoke).toHaveBeenCalledWith("timeline_reply", {
      roomId: "!general:example.org",
      replyTo: "$said:example.org",
      sender: "@ada:example.org",
      body: "quite",
    });
  });

  it("names the message being corrected and nothing about who sent it", async () => {
    // No sender rides along, unlike a reply. Who wrote it is read off the
    // target event in Rust, and taking the webview's word for it would be
    // taking the webview's word for who may rewrite whom.
    await timelineEdit(
      "!general:example.org",
      "$said:example.org",
      "corrected",
    );

    expect(invoke).toHaveBeenCalledWith("timeline_edit", {
      roomId: "!general:example.org",
      eventId: "$said:example.org",
      body: "corrected",
    });
  });

  it("asks Rust to put a message address on the clipboard", async () => {
    await timelineCopyLink("!general:example.org", "$said:example.org");

    expect(invoke).toHaveBeenCalledWith("timeline_copy_link", {
      roomId: "!general:example.org",
      eventId: "$said:example.org",
    });
  });

  it("names the message a jump is to", async () => {
    // The one argument, under the name the Rust command expects. Getting it
    // wrong is a deserialisation error at runtime rather than a build failure,
    // and the symptom is a reply row that does nothing.
    await timelineGoTo("$said:example.org");

    expect(invoke).toHaveBeenCalledWith("timeline_go_to", {
      eventId: "$said:example.org",
    });
  });

  it("asks Rust which room an address points at", async () => {
    invoke.mockResolvedValue("!general:example.org");

    await expect(roomAt("#general:example.org")).resolves.toBe(
      "!general:example.org",
    );
    expect(invoke).toHaveBeenCalledWith("room_at", {
      address: "#general:example.org",
    });
  });
});

describe("attachments", () => {
  beforeEach(() => {
    invoke.mockReset().mockResolvedValue(undefined);
    listen.mockReset().mockResolvedValue(() => {});
  });

  it("asks Rust to open the picker, because this page cannot", async () => {
    invoke.mockResolvedValue({
      path: "/home/ada/cat.png",
      name: "cat.png",
      size: 10,
    });

    await expect(pickAttachment()).resolves.toEqual({
      path: "/home/ada/cat.png",
      name: "cat.png",
      size: 10,
    });
    expect(invoke).toHaveBeenCalledWith("attachment_pick");
  });

  it("subscribes to the channel a drop arrives on", async () => {
    // The name is a contract with `AppEvent::DROPPED`, and Tauri does not
    // complain about a listener for a channel nothing sends on.
    await onDropped(vi.fn());

    expect(listen).toHaveBeenCalledWith("dropped", expect.any(Function));
  });

  it("sends a picked file by its path, with the caption beside it", async () => {
    await attachFile(
      "!general:example.org",
      "/home/ada/cat.png",
      "look",
      "$said:example.org",
    );

    expect(invoke).toHaveBeenCalledWith("timeline_attach_file", {
      roomId: "!general:example.org",
      path: "/home/ada/cat.png",
      caption: "look",
      replyTo: "$said:example.org",
    });
  });

  it("asks Rust what is on the clipboard rather than reading it here", async () => {
    invoke.mockResolvedValue({ name: "pasted-image.png", size: 4096 });

    const shot = await pasteAttachment();

    expect(invoke).toHaveBeenCalledWith("attachment_paste");
    expect(shot).toEqual({ name: "pasted-image.png", size: 4096 });
  });

  it("says there was nothing to stage when the clipboard held words", async () => {
    // Which is how the keystroke goes on to put them in the box: Rust answers
    // with nothing rather than staging a rendering of the text.
    invoke.mockResolvedValue(null);

    expect(await pasteAttachment()).toBeNull();
  });

  it("sends a pasted screenshot by asking for the one Rust is holding", async () => {
    // No bytes and no name. The picture never crossed to this side, so there
    // is nothing here to send but the instruction to send it.
    await attachPasted("!general:example.org", "look", "$said:example.org");

    expect(invoke).toHaveBeenCalledWith("timeline_attach_pasted", {
      roomId: "!general:example.org",
      caption: "look",
      replyTo: "$said:example.org",
    });
  });
});

describe("the timeline commands", () => {
  const GENERAL = "!general:example.org";

  beforeEach(() => {
    invoke.mockReset().mockResolvedValue(undefined);
    listen.mockReset().mockResolvedValue(() => {});
  });

  it("subscribes to the channel a room's messages arrive on", async () => {
    // The name is a contract with `AppEvent::TIMELINE`. A typo here is a pane
    // that stays empty rather than anything that fails.
    await onTimeline(vi.fn());

    expect(listen).toHaveBeenCalledWith("timeline", expect.any(Function));
  });

  it("hands the timeline handler the payload rather than the envelope", async () => {
    const handler = vi.fn();
    await onTimeline(handler);
    const [, forward] = listen.mock.calls[0] as [
      string,
      (event: { payload: unknown }) => void,
    ];
    const timeline = { ...NO_TIMELINE, roomId: GENERAL };

    forward({ payload: timeline });

    expect(handler).toHaveBeenCalledWith(timeline);
  });

  it("opens a room under the name the Rust command expects", async () => {
    await timelineOpen(GENERAL);

    expect(invoke).toHaveBeenCalledWith("timeline_open", { roomId: GENERAL });
  });

  it("closes whatever room was open with no arguments", async () => {
    // Nothing is named because nothing has to be: exactly one room is open.
    await timelineClose();

    expect(invoke).toHaveBeenCalledWith("timeline_close");
  });

  it("asks for a page of older messages with no arguments", async () => {
    await timelineEarlier();

    expect(invoke).toHaveBeenCalledWith("timeline_earlier");
  });

  it("asks for a page of newer messages with no arguments", async () => {
    await timelineLater();

    expect(invoke).toHaveBeenCalledWith("timeline_later");
  });

  it("goes back to the live end with no arguments", async () => {
    await timelinePresent();

    expect(invoke).toHaveBeenCalledWith("timeline_present");
  });

  it("sends what was typed under the names the Rust command expects", async () => {
    await timelineSend(GENERAL, "morning");

    expect(invoke).toHaveBeenCalledWith("timeline_send", {
      roomId: GENERAL,
      body: "morning",
    });
  });

  it("saves an attachment by its handle and answers where it landed", async () => {
    invoke.mockResolvedValue("/home/ada/Downloads/cat.png");

    await expect(saveAttachment("{\"url\":\"mxc://example.org/abc\"}", "cat.png"))
      .resolves.toBe("/home/ada/Downloads/cat.png");
    expect(invoke).toHaveBeenCalledWith("timeline_media_save", {
      source: "{\"url\":\"mxc://example.org/abc\"}",
      name: "cat.png",
    });
  });

  it("answers null when the save was cancelled rather than throwing", async () => {
    // Dismissing the file dialog is not a failure, and treating it as one
    // would put an error in front of somebody who chose not to save.
    invoke.mockResolvedValue(null);

    await expect(saveAttachment("{}", "cat.png")).resolves.toBeNull();
  });

  it("asks for member names under the names the Rust command expects", async () => {
    invoke.mockResolvedValue({ "@bob:example.org": "Bob" });

    await expect(memberNames(GENERAL, ["@bob:example.org"])).resolves.toEqual({
      "@bob:example.org": "Bob",
    });
    expect(invoke).toHaveBeenCalledWith("member_names", {
      roomId: GENERAL,
      userIds: ["@bob:example.org"],
    });
  });

  it("asks for the direct room with a person and answers which one it is", async () => {
    invoke.mockResolvedValue("!dm:example.org");

    await expect(directRoom("@bob:example.org")).resolves.toBe("!dm:example.org");
    expect(invoke).toHaveBeenCalledWith("direct_room", {
      userId: "@bob:example.org",
    });
  });

  it("asks who is in a room under the name the Rust command expects", async () => {
    // A room at a time, never in the room list, because the list is re-sent
    // in full whenever anything in it changes.
    const who = {
      joined: { count: 1, shown: [{ person: { id: "@bob:example.org", name: "Bob" } }] },
      invited: { count: 0, shown: [] },
    };
    invoke.mockResolvedValue(who);

    await expect(roomMembers(GENERAL)).resolves.toEqual(who);
    expect(invoke).toHaveBeenCalledWith("room_members", { roomId: GENERAL });
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

  it("names the message a reaction is on and what it is", async () => {
    await timelineReact("!general:example.org", "$said:example.org", "🎉");

    expect(invoke).toHaveBeenCalledWith("timeline_react", {
      roomId: "!general:example.org",
      eventId: "$said:example.org",
      key: "🎉",
    });
  });

  it("takes a reaction back by its own event, not the message it is on", async () => {
    // Passing the message would redact the message.
    await timelineUnreact("!general:example.org", "$mine:example.org");

    expect(invoke).toHaveBeenCalledWith("timeline_unreact", {
      roomId: "!general:example.org",
      reactionId: "$mine:example.org",
    });
  });

  it("says whether this session is typing, and where", async () => {
    await timelineTyping("!general:example.org", true);

    expect(invoke).toHaveBeenCalledWith("timeline_typing", {
      roomId: "!general:example.org",
      typing: true,
    });
  });

  it("marks a message read without naming the room", async () => {
    // The room is whichever one is open, which Rust already knows. Passing it
    // from here would let a receipt for the room just left arrive after the
    // room change and be answered by the new room's watcher.
    await timelineMarkRead("$said:example.org");

    expect(invoke).toHaveBeenCalledWith("timeline_mark_read", {
      eventId: "$said:example.org",
    });
  });

  it("reads what this account tells other people about itself", async () => {
    await privacySettings();

    expect(invoke).toHaveBeenCalledWith("privacy_settings");
  });

  it("saves the whole privacy section rather than one field", async () => {
    await setPrivacySettings({ publicReadReceipts: false });

    expect(invoke).toHaveBeenCalledWith("set_privacy_settings", {
      privacy: { publicReadReceipts: false },
    });
  });

  it("reads when to interrupt somebody", async () => {
    await notificationSettings();

    expect(invoke).toHaveBeenCalledWith("notification_settings");
  });

  it("saves the whole notification section rather than one field", async () => {
    await setNotificationSettings({
      enabled: true,
      mentionsOnly: true,
      sound: false,
    });

    expect(invoke).toHaveBeenCalledWith("set_notification_settings", {
      notifications: { enabled: true, mentionsOnly: true, sound: false },
    });
  });

  it("asks Rust to open a link rather than following it", async () => {
    // Following it in the webview would replace Consort with the website.
    await openLink("https://example.org");

    expect(invoke).toHaveBeenCalledWith("open_link", {
      address: "https://example.org",
    });
  });

  it("subscribes to the typing channel the Rust side emits on", async () => {
    await onTyping(vi.fn());

    expect(listen).toHaveBeenCalledWith("typing", expect.any(Function));
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

  it("subscribes to the show-room channel by the name Rust emits on", async () => {
    await onShowRoom(vi.fn());

    expect(listen).toHaveBeenCalledWith("show-room", expect.any(Function));
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

  it("starts playing the microphone back with no arguments", async () => {
    invoke.mockResolvedValue(undefined);

    await audioMonitorStart();

    expect(invoke).toHaveBeenCalledWith("audio_monitor_start");
  });

  it("stops playing the microphone back with no arguments", async () => {
    invoke.mockResolvedValue(undefined);

    await audioMonitorStop();

    expect(invoke).toHaveBeenCalledWith("audio_monitor_stop");
  });

  it("passes a person's volume under the names the Rust command expects", async () => {
    invoke.mockResolvedValue(undefined);

    await setPersonVolume("@bob:example.org", 140);

    expect(invoke).toHaveBeenCalledWith("set_person_volume", {
      userId: "@bob:example.org",
      percent: 140,
    });
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

  it("asks to go away and to come back by the same command", async () => {
    invoke.mockResolvedValue(undefined);

    await callSetAway(true);

    expect(invoke).toHaveBeenCalledWith("call_set_away", { away: true });
  });

  it("subscribes to the channel saying whether a call can be joined", async () => {
    // Watched rather than asked once, so verifying mid-session is noticed.
    listen.mockResolvedValue(() => {});

    await onCallReadiness(vi.fn());

    expect(listen).toHaveBeenCalledWith("call-readiness", expect.any(Function));
  });

  it("hands the readiness handler the payload rather than the envelope", async () => {
    listen.mockResolvedValue(() => {});
    const handler = vi.fn();
    await onCallReadiness(handler);
    const [, forward] = listen.mock.calls[0] as [
      string,
      (event: { payload: CallReadiness }) => void,
    ];

    forward({ payload: { state: "sessionUnverified" } });

    expect(handler).toHaveBeenCalledWith({ state: "sessionUnverified" });
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

  it("addresses a plain mxc as the source Rust parses it back into", () => {
    // What a custom emoji needs: it comes from a pack rather than from an
    // event, so there is no attachment handle and this builds the one a plain
    // `MediaSource` serialises to. `media_source_of_a_plain_mxc` in
    // `timeline/media.rs` reads this same literal back.
    expect(mxcUrl("mxc://example.org/abc")).toBe(
      "consortmedia://localhost/eyJ1cmwiOiJteGM6Ly9leGFtcGxlLm9yZy9hYmMifQ",
    );
  });

  it("refuses to address anything that is not an mxc", () => {
    // The only thing stopping a message pointing an img at somebody's web
    // server, which is a read receipt the reader did not agree to.
    expect(mxcUrl("https://tracker.example/pixel.gif")).toBeUndefined();
    expect(mxcUrl("data:image/gif;base64,R0lGOD")).toBeUndefined();
    expect(mxcUrl("")).toBeUndefined();
  });

  it("uses no character a path would have to escape", () => {
    const url = mediaUrl('{"url":"mxc://example.org/a?b&c"}');

    expect(url.slice("consortmedia://localhost/".length)).toMatch(
      /^[A-Za-z0-9_-]+$/,
    );
  });
});

describe("addressing an attachment on Windows", () => {
  // Issue #76. v0.6.0 drew no attachment at all on Windows and every one of
  // them on Arch, because the URL was written out by hand in the one shape
  // Linux and macOS use. WebView2 cannot register a non-standard scheme, so
  // wry serves a custom protocol over an HTTP subdomain there and filters on
  // `http://consortmedia.*` to catch it. A `consortmedia://` request matches
  // no filter, reaches no handler, and fails without reaching a log, which is
  // why nothing in the Rust output named it.

  it("serves media over the HTTP subdomain WebView2 can intercept", () => {
    mockConvertFileSrc("windows");

    expect(
      mediaUrl('{"url":"mxc://example.org/abc","key":{"k":"a+b/c"}}'),
    ).toBe(
      "http://consortmedia.localhost/eyJ1cmwiOiJteGM6Ly9leGFtcGxlLm9yZy9hYmMiLCJrZXkiOnsiayI6ImErYi9jIn19",
    );
  });

  it("addresses a custom emoji the same way", () => {
    mockConvertFileSrc("windows");

    expect(mxcUrl("mxc://example.org/abc")).toBe(
      "http://consortmedia.localhost/eyJ1cmwiOiJteGM6Ly9leGFtcGxlLm9yZy9hYmMifQ",
    );
  });

  it("hands over the same handle either way, so Rust decodes one thing", () => {
    // The path is what `media::handle` base64 decodes, and it must not depend
    // on which platform wrote it. Only the origin in front of it may differ.
    const handle = '{"url":"mxc://example.org/abc"}';

    mockConvertFileSrc("linux");
    const unix = mediaUrl(handle);
    mockConvertFileSrc("windows");
    const windows = mediaUrl(handle);

    expect(new URL(windows).pathname).toBe(new URL(unix).pathname);
  });
});

describe("the content security policy and the media URL agree", () => {
  // The other half of issue #76, and the half that was already right. A media
  // URL is only ever as good as the policy that admits it, and the two live in
  // different files that change for different reasons: the scheme is chosen in
  // `api.ts` and permitted in `tauri.conf.json`. Nothing but this test makes
  // one follow the other, and the failure it guards against is silent on the
  // platform whoever made the change was sitting at.

  /** The CSP the bundle actually ships, by directive, each as a source list. */
  const directives = new Map(
    tauriConfig.app.security.csp
      .split(";")
      .map((directive) => directive.trim().split(/\s+/))
      .filter(([name]) => name !== "")
      .map(([name, ...sources]) => [name, sources] as const),
  );

  /**
   * The CSP source that has to be listed for `url` to load.
   *
   * An origin for the HTTP form Windows and Android use, and a bare scheme for
   * the `consortmedia://localhost` one everywhere else, which is how a policy
   * names a non-standard scheme.
   */
  const permitting = (url: string): string => {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:"
      ? parsed.origin
      : parsed.protocol;
  };

  it.each(["linux", "windows"])("admits a picture drawn on %s", (name) => {
    mockConvertFileSrc(name);

    const source = permitting(mediaUrl('{"url":"mxc://example.org/abc"}'));

    expect(directives.get("img-src")).toContain(source);
  });

  it.each(["linux", "windows"])("admits a clip played on %s", (name) => {
    mockConvertFileSrc(name);

    const source = permitting(mediaUrl('{"url":"mxc://example.org/abc"}'));

    expect(directives.get("media-src")).toContain(source);
  });
});
