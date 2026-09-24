import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";

const audioDevices = vi.hoisted(() => vi.fn());
const audioSettings = vi.hoisted(() => vi.fn());
const audioTestStart = vi.hoisted(() => vi.fn());
const audioTestStop = vi.hoisted(() => vi.fn());
const onAudio = vi.hoisted(() => vi.fn());
const logout = vi.hoisted(() => vi.fn());
const roomAvatar = vi.hoisted(() => vi.fn());
// The screen drawn with no channel selected asks which rooms were opened last.
const recentRooms = vi.hoisted(() => vi.fn());

// The main pane draws a room's messages, which is a subscription and three
// commands. Mocked rather than left to fail: an unmocked `invoke` rejects into
// nothing anybody awaits, which surfaces as an unhandled rejection attributed
// to whichever test happened to be running.
const onTimeline = vi.hoisted(() => vi.fn());
const onTyping = vi.hoisted(() => vi.fn());
const onDropped = vi.hoisted(() => vi.fn());
const timelineTyping = vi.hoisted(() => vi.fn());
const onThread = vi.hoisted(() => vi.fn());
const timelineOpen = vi.hoisted(() => vi.fn());
const timelineClose = vi.hoisted(() => vi.fn());
const timelineEarlier = vi.hoisted(() => vi.fn());
// A window around an older message, and the way back out of one. Both are
// needed to tell a room that is following the present from one that is not.
const timelineGoTo = vi.hoisted(() => vi.fn());
const timelinePresent = vi.hoisted(() => vi.fn());
const timelineSend = vi.hoisted(() => vi.fn());
const memberNames = vi.hoisted(() => vi.fn());
const roomAt = vi.hoisted(() => vi.fn());
// The right of the window holds one panel at a time, so asking for a room's
// details shuts whatever thread was there.
const threadOpen = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  audioDevices,
  audioSettings,
  audioTestStart,
  audioTestStop,
  onAudio,
  logout,
  onTimeline,
  onTyping,
  onDropped,
  timelineTyping,
  onThread,
  timelineOpen,
  timelineClose,
  timelineEarlier,
  timelineGoTo,
  timelinePresent,
  timelineSend,
  memberNames,
  roomAt,
  roomAvatar,
  recentRooms,
  threadOpen,
}));

import { AppShell } from "./AppShell";
import { goBack, goForward, pressBack } from "../test/traversal";
import { resetAvatarCache } from "../lib/avatars";
import { HEARING } from "../lib/api";
import type {
  AudioDeviceReport,
  AudioSettings,
  Call,
  CallRefused,
  Channel,
  Message,
  Profile,
  Rooms,
  SelfAudio,
  Thread,
  Timeline,
} from "../lib/api";

const profile: Profile = {
  user_id: "@ada:example.org",
  device_id: "ABCDEFGH",
  homeserver: "https://example.org",
  display_name: "Ada",
  avatar_url: null,
};

const report: AudioDeviceReport = {
  input: {
    devices: [{ name: "Built-in Microphone", isDefault: true }],
    selected: "Built-in Microphone",
    missing: null,
  },
  output: {
    devices: [{ name: "Built-in Speakers", isDefault: true }],
    selected: "Built-in Speakers",
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

const EMPTY_HOME: Rooms = {
  spaces: [{ id: "home", name: "Home", avatar: null, channels: [] }],
};

function voice(id: string, name: string): Channel {
  return { id, name, kind: "voice", avatar: null, joined: true,
    participants: [],
    unread: 0,
    mentions: 0,
  };
}

function textChannel(id: string, name: string): Channel {
  return { id, name, kind: "text", avatar: null, joined: true,
    participants: [],
    unread: 0,
    mentions: 0,
  };
}

function shell({
  rooms = EMPTY_HOME,
  call = { state: "disconnected" } as Call,
  selfAudio = HEARING,
  onSignedOut = vi.fn(),
  onJoinVoice = vi.fn(),
  onLeaveVoice = vi.fn(),
  onSetMuted = vi.fn(),
  onSetDeafened = vi.fn(),
  onSetAway = vi.fn(),
  callRefused = null,
  onDismissRefusal = vi.fn(),
  showRoom = null,
  onRoomShown = vi.fn(),
}: {
  rooms?: Rooms;
  call?: Call;
  selfAudio?: SelfAudio;
  onSignedOut?: Mock<() => void>;
  onJoinVoice?: Mock<(roomId: string) => void>;
  onLeaveVoice?: Mock<() => void>;
  onSetMuted?: Mock<(muted: boolean) => void>;
  onSetDeafened?: Mock<(deafened: boolean) => void>;
  onSetAway?: Mock<(away: boolean) => void>;
  callRefused?: CallRefused | null;
  onDismissRefusal?: Mock<() => void>;
  showRoom?: { roomId: string } | null;
  onRoomShown?: Mock<() => void>;
} = {}) {
  const draw = (
    nextRooms: Rooms,
    nextShowRoom: { roomId: string } | null,
    nextCall: Call,
  ) => (
    <AppShell
      profile={profile}
      rooms={nextRooms}
      connection={{ state: "live" }}
      call={nextCall}
      selfAudio={selfAudio}
      verification={{ state: "verified" }}
      keyBackup={{ state: "enabled" }}
      storage={null}
      flows={[]}
      canStartVerification
      onDismissFlow={vi.fn()}
      onJoinVoice={onJoinVoice}
      onLeaveVoice={onLeaveVoice}
      onSetMuted={onSetMuted}
      onSetDeafened={onSetDeafened}
      onSetAway={onSetAway}
      callRefused={callRefused}
      onDismissRefusal={onDismissRefusal}
      showRoom={nextShowRoom}
      onRoomShown={onRoomShown}
      onSignedOut={onSignedOut}
    />
  );
  const { container, rerender } = render(draw(rooms, showRoom, call));
  /*
    Hand the same shell a new tree rather than rendering a second one, which
    would be a second shell. A room list arriving again is what every sync
    that touches anything does, and the shell holds state the call moves
    through, so some of what it does is only visible across a change rather
    than in one render.
  */
  const again = (next: {
    rooms?: Rooms;
    showRoom?: { roomId: string } | null;
    call?: Call;
  }) =>
    rerender(
      draw(next.rooms ?? rooms, next.showRoom ?? showRoom, next.call ?? call),
    );
  return {
    container,
    rerender,
    again,
    onRoomShown,
    onSignedOut,
    onJoinVoice,
    onLeaveVoice,
  };
}

describe("AppShell", () => {
  beforeEach(() => {
    resetAvatarCache();
    // jsdom has none, and a link followed to a message lands by calling it.
    Element.prototype.scrollIntoView = vi.fn();
    audioDevices.mockReset().mockResolvedValue(report);
    audioSettings.mockReset().mockResolvedValue(settings);
    audioTestStart.mockReset().mockResolvedValue(undefined);
    audioTestStop.mockReset().mockResolvedValue(undefined);
    onAudio.mockReset().mockResolvedValue(() => {});
    logout.mockReset().mockResolvedValue(undefined);
    roomAvatar.mockReset().mockResolvedValue(null);
    recentRooms.mockReset().mockResolvedValue([]);
    onTimeline.mockReset().mockResolvedValue(() => {});
    onTyping.mockReset().mockResolvedValue(() => {});
    onDropped.mockReset().mockResolvedValue(() => {});
    timelineTyping.mockReset().mockResolvedValue(undefined);
    onThread.mockReset().mockResolvedValue(() => {});
    timelineOpen.mockReset().mockResolvedValue(undefined);
    timelineClose.mockReset().mockResolvedValue(undefined);
    timelineEarlier.mockReset().mockResolvedValue(undefined);
    timelineGoTo.mockReset().mockResolvedValue(undefined);
    timelinePresent.mockReset().mockResolvedValue(undefined);
    timelineSend.mockReset().mockResolvedValue(undefined);
    memberNames.mockReset().mockResolvedValue({});
    roomAt.mockReset().mockResolvedValue("!tech:example.org");
    threadOpen.mockReset().mockResolvedValue(undefined);
  });

  it("says so once the room a notification asked for is shown", async () => {
    /*
      The whole of #103. `showRoom` is an ask, and an ask that is never taken
      back is read again by an effect that re-runs on every room list, which
      drags the selection back to that room on every sync. Only the shell knows
      whether it landed, so saying so is the shell's job, and spending it is
      the caller's.
    */
    const rooms: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [textChannel("!tech:example.org", "tech")],
        },
      ],
    };

    const { onRoomShown } = shell({
      rooms,
      showRoom: { roomId: "!tech:example.org" },
    });

    await screen.findByRole("heading", { level: 1, name: "#tech" });
    expect(onRoomShown).toHaveBeenCalledTimes(1);
  });

  it("says nothing while the rail still does not have the room", async () => {
    // The retry above is the reason the effect watches the room list at all,
    // and a retry that reported success would spend an ask it never took.
    const empty: Rooms = {
      spaces: [{ id: "home", name: "Home", avatar: null, channels: [] }],
    };
    const arrived: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [textChannel("!tech:example.org", "tech")],
        },
      ],
    };
    const asked = { roomId: "!tech:example.org" };

    const { again, onRoomShown } = shell({ rooms: empty, showRoom: asked });
    expect(onRoomShown).not.toHaveBeenCalled();

    again({ rooms: arrived });

    await screen.findByRole("heading", { level: 1, name: "#tech" });
    expect(onRoomShown).toHaveBeenCalledTimes(1);
  });

  it("shows the room a notification was clicked to get to", async () => {
    const rooms: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [
            textChannel("!general:example.org", "general"),
            textChannel("!tech:example.org", "tech"),
          ],
        },
      ],
    };

    shell({ rooms, showRoom: { roomId: "!tech:example.org" } });

    expect(
      await screen.findByRole("heading", { name: "#tech" }),
    ).toBeInTheDocument();
  });

  it("waits for the rail to know about a room it was sent to", async () => {
    // A notification about a room joined a moment ago can be clicked before
    // the room list has caught up, and a press that did nothing would be a
    // notification that lied about where it went.
    const empty: Rooms = {
      spaces: [{ id: "home", name: "Home", avatar: null, channels: [] }],
    };
    const arrived: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [textChannel("!tech:example.org", "tech")],
        },
      ],
    };
    const asked = { roomId: "!tech:example.org" };

    shell({ rooms: empty, showRoom: asked });
    expect(screen.queryByRole("heading", { name: "#tech" })).toBeNull();

    shell({ rooms: arrived, showRoom: asked });

    expect(
      await screen.findByRole("heading", { name: "#tech" }),
    ).toBeInTheDocument();
  });

  it("shows nothing in particular when no notification has been clicked", async () => {
    const rooms: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [textChannel("!general:example.org", "general")],
        },
      ],
    };

    shell({ rooms });

    expect(screen.queryByRole("heading", { name: "#general" })).toBeNull();
  });

  it("folds the channel list away, and back", async () => {
    const rooms: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [textChannel("!general:example.org", "general")],
        },
      ],
    };
    const { container } = shell({ rooms });
    await userEvent.click(await screen.findByRole("button", { name: /general/i }));

    await userEvent.click(
      screen.getByRole("button", { name: /hide the channel list/i }),
    );

    expect(container.querySelector(".shell")).toHaveAttribute(
      "data-sidebar",
      "folded",
    );

    await userEvent.click(
      screen.getByRole("button", { name: /show the channel list/i }),
    );

    expect(container.querySelector(".shell")).not.toHaveAttribute(
      "data-sidebar",
    );
  });

  it("can be unfolded before any channel has been picked", async () => {
    // The empty pane has no room header to hang the control on, so it carries
    // its own. Without it, folding the list before choosing a channel is a
    // one-way door.
    const { container } = shell();

    await userEvent.click(
      screen.getByRole("button", { name: /hide the channel list/i }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: /show the channel list/i }),
    );

    expect(container.querySelector(".shell")).not.toHaveAttribute(
      "data-sidebar",
    );
  });

  it("keeps the session's identifiers out of the room, and in settings", async () => {
    // They were printed under the message pane while there was nothing else
    // to put there. A room is not a debug panel, and every one of them is two
    // clicks away under My Account, which is where somebody looking for their
    // device ID goes.
    shell();

    expect(screen.queryByText("ABCDEFGH")).not.toBeInTheDocument();
    expect(screen.queryByText("https://example.org")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    expect(await screen.findByText("ABCDEFGH")).toBeVisible();
  });

  it("opens settings from the gear on the user strip", async () => {
    shell();

    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    expect(await screen.findByRole("dialog")).toBeVisible();
  });

  it("leaves out the banner strip when there is nothing to announce", () => {
    // It used to be drawn empty, and an empty grid row still takes the gap
    // either side of it. That gap was a band of dead space above every room
    // name for a session with nothing wrong with it.
    const { container } = shell();

    expect(container.querySelector(".shell__alerts")).toBeNull();
  });

  it("draws the banner strip once something has to be said", () => {
    const { container } = shell({
      callRefused: {
        roomId: "!lounge:example.org",
        readiness: { state: "noIdentity" },
      },
    });

    expect(container.querySelector(".shell__alerts")).not.toBeNull();
  });

  it("shows no dialog until it is asked for", () => {
    shell();

    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("takes the rest of the application out of reach while settings are open", async () => {
    // The focus trap keeps Tab inside the dialog. This is the other half:
    // nothing behind it should be clickable, focusable, or read out.
    const { container } = shell();
    const layout = container.querySelector(".shell");

    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    expect(layout).toHaveAttribute("inert");
  });

  it("gives the application back when settings close", async () => {
    const { container } = shell();
    const layout = container.querySelector(".shell");
    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    await userEvent.click(screen.getByRole("button", { name: /close settings/i }));

    expect(layout).not.toHaveAttribute("inert");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("signs out from inside settings", async () => {
    const { onSignedOut } = shell();
    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    await userEvent.click(screen.getByRole("button", { name: /log out/i }));

    await waitFor(() => expect(logout).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onSignedOut).toHaveBeenCalledTimes(1));
  });

  it("puts focus back on the gear when settings close", async () => {
    shell();
    const gear = screen.getByRole("button", { name: /user settings/i });
    await userEvent.click(gear);

    await userEvent.click(screen.getByRole("button", { name: /close settings/i }));

    await waitFor(() => expect(document.activeElement).toBe(gear));
  });

  describe("a room's details", () => {
    const GENERAL = "!general:example.org";

    const withRoom: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [textChannel(GENERAL, "general")],
        },
      ],
    };

    /** Select the room, and hand back its heading, which opens the details. */
    async function heading() {
      shell({ rooms: withRoom });
      await userEvent.click(
        within(screen.getByRole("region", { name: "Text" })).getByRole(
          "button",
          { name: /general/ },
        ),
      );
      // Scoped to the pane, because the row in the list beside it is called
      // the same thing.
      return within(screen.getByRole("main")).getByRole("button", {
        name: "#general",
      });
    }

    /** Hand a thread to everybody watching that channel, as Rust would. */
    function arriveThread() {
      const thread: Thread = {
        roomId: GENERAL,
        rootId: "$root",
        messages: [],
        moreBefore: false,
      };
      act(() => {
        for (const [handler] of onThread.mock.calls) {
          (handler as (value: Thread | null) => void)(thread);
        }
      });
    }

    it("opens them from the room's own heading", async () => {
      // Issue #85. The heading drew the name and the topic and did nothing
      // when it was pressed.
      await userEvent.click(await heading());

      expect(screen.getByRole("heading", { name: "Room info" })).toBeVisible();
    });

    it("puts them away when the heading is pressed again", async () => {
      const name = await heading();
      await userEvent.click(name);

      await userEvent.click(name);

      expect(screen.queryByRole("heading", { name: "Room info" })).toBeNull();
    });

    it("puts them away from their own close control", async () => {
      await userEvent.click(await heading());

      await userEvent.click(
        screen.getByRole("button", { name: "Close room info" }),
      );

      expect(screen.queryByRole("heading", { name: "Room info" })).toBeNull();
    });

    it("shuts any open thread on the way up", async () => {
      // One column, one panel. Both at once leaves the conversation a strip.
      await userEvent.click(await heading());

      expect(threadOpen).toHaveBeenCalledWith(null);
    });

    it("gives the column back when a thread arrives", async () => {
      // The other direction, and the shell cannot ask for it: which thread is
      // open is Rust's answer, so the panel is what reports one.
      await userEvent.click(await heading());

      arriveThread();

      expect(screen.queryByRole("heading", { name: "Room info" })).toBeNull();
    });

    it("puts them away on Escape", async () => {
      await userEvent.click(await heading());

      await userEvent.keyboard("{Escape}");

      expect(screen.queryByRole("heading", { name: "Room info" })).toBeNull();
    });

    it("leaves them open when a dialog over them takes the Escape", async () => {
      // One press, one thing. The dialog is on top, so it is what closes, and
      // details that went with it would be a second thing nobody asked for.
      await userEvent.click(await heading());
      await userEvent.click(
        screen.getByRole("button", { name: /user settings/i }),
      );
      expect(await screen.findByRole("dialog")).toBeVisible();

      await userEvent.keyboard("{Escape}");

      expect(screen.queryByRole("dialog")).toBeNull();
      expect(screen.getByRole("heading", { name: "Room info" })).toBeVisible();
    });

    it("draws nothing before a room is picked", async () => {
      // There is no heading to press in the empty pane, and nothing for a
      // panel about a room to describe.
      shell({ rooms: withRoom });

      expect(screen.queryByRole("heading", { name: "Room info" })).toBeNull();
    });
  });

  /*
    What happens to the selection when a room stops being one of this
    account's, which until #93 could only happen from another session.

    Nothing in the shell does any of this on purpose. Both selections are
    derived from the room list every render, so a room that has gone simply
    stops being selected, and these say that out loud because the alternative
    reading is written down in `openRoom` and is not quite right: what is left
    where it is is the stored channel ID, and what somebody sees is the empty
    pane.
  */
  describe("a room that has been left", () => {
    const GENERAL = "!general:example.org";
    const LOUNGE = "!lounge:example.org";

    function withRooms(channels: Channel[]): Rooms {
      return {
        spaces: [
          {
            id: "home",
            name: "Home",
            avatar: null,
            channels,
          },
        ],
      };
    }

    const both = withRooms([
      textChannel(GENERAL, "general"),
      textChannel(LOUNGE, "lounge"),
    ]);

    /** Select `name` in the channel list, as a click would. */
    async function select(name: RegExp) {
      await userEvent.click(
        within(screen.getByRole("region", { name: "Text" })).getByRole(
          "button",
          { name },
        ),
      );
    }

    it("falls back to the empty pane when the room being read goes", async () => {
      const { again } = shell({ rooms: both });
      await select(/general/);
      expect(
        within(screen.getByRole("main")).getByRole("heading", {
          name: "#general",
        }),
      ).toBeVisible();

      again({ rooms: withRooms([textChannel(LOUNGE, "lounge")]) });

      expect(
        screen.getByRole("heading", { name: "Nothing here yet" }),
      ).toBeVisible();
    });

    it("stays in the space the room was in rather than jumping", async () => {
      // The rail entry outlives the room. Somebody who left one channel of a
      // space is still in the space, and a selection that fell back to the
      // first entry would move them somewhere they did not ask to be.
      const { again } = shell({ rooms: both });
      await select(/general/);

      again({ rooms: withRooms([textChannel(LOUNGE, "lounge")]) });

      expect(
        within(screen.getByRole("region", { name: "Text" })).getByRole(
          "button",
          { name: /lounge/ },
        ),
      ).toBeVisible();
    });

    it("leaves the selection alone when it is some other room that goes", async () => {
      // The half that makes the other half worth having. Leaving a room is
      // not a reason to stop reading the one on screen.
      const { again } = shell({ rooms: both });
      await select(/general/);

      again({ rooms: withRooms([textChannel(GENERAL, "general")]) });

      expect(
        within(screen.getByRole("main")).getByRole("heading", {
          name: "#general",
        }),
      ).toBeVisible();
    });

    it("takes the room's details away with the room", async () => {
      // The panel is where the leave control is, so the panel is what would
      // otherwise be left describing a room this account is not in, offering
      // to leave it again.
      const { again } = shell({ rooms: both });
      await select(/general/);
      await userEvent.click(
        within(screen.getByRole("main")).getByRole("button", {
          name: "#general",
        }),
      );
      expect(screen.getByRole("heading", { name: "Room info" })).toBeVisible();

      again({ rooms: withRooms([textChannel(LOUNGE, "lounge")]) });

      expect(screen.queryByRole("heading", { name: "Room info" })).toBeNull();
    });
  });

  describe("voice channels", () => {
    const LOUNGE = "!lounge:example.org";

    const withVoice: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [textChannel("!g:example.org", "general"), voice(LOUNGE, "Lounge")],
        },
      ],
    };

    it("joins a voice channel when it is clicked", async () => {
      const { onJoinVoice } = shell({ rooms: withVoice });

      await userEvent.click(screen.getByRole("button", { name: "Lounge" }));

      expect(onJoinVoice).toHaveBeenCalledWith(LOUNGE);
    });

    it("does not try to join a text channel", async () => {
      // The one thing a click on the wrong row must not do. Joining a call in
      // a text room is legal MatrixRTC and is nothing anybody asked for.
      const { onJoinVoice } = shell({ rooms: withVoice });

      await userEvent.click(screen.getByRole("button", { name: /general/ }));

      expect(onJoinVoice).not.toHaveBeenCalled();
    });

    it("still selects the voice channel it joined", async () => {
      // The main pane names what was clicked, so joining without selecting
      // would leave the heading pointing at whatever was open before.
      shell({ rooms: withVoice });

      await userEvent.click(screen.getByRole("button", { name: "Lounge" }));

      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "Lounge",
      );
    });

    it("names the channel it is connected to in the panel", () => {
      shell({
        rooms: withVoice,
        call: {
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    },
      });

      const panel = screen.getByRole("group", { name: /voice connection/i });
      expect(panel).toHaveTextContent(/voice connected/i);
      expect(panel).toHaveTextContent("Lounge");
    });

    it("says it is still working while a join is in flight", () => {
      // A join waits on a homeserver, an authorisation service and an SFU in
      // turn. A panel that looked connected during it would be claiming
      // something that is not true yet.
      shell({
        rooms: withVoice,
        call: { state: "connecting", roomId: LOUNGE },
      });

      expect(
        screen.getByRole("group", { name: /voice connection/i }),
      ).toHaveTextContent(/connecting/i);
    });

    it("shows no connection panel when there is no call", () => {
      // A permanent row saying "not in a voice channel" is a row that is
      // wrong-looking most of the time and teaches people to stop reading it.
      shell({ rooms: withVoice });

      expect(
        screen.queryByRole("group", { name: /voice connection/i }),
      ).toBeNull();
    });

    it("leaves the call from the panel", async () => {
      const { onLeaveVoice } = shell({
        rooms: withVoice,
        call: {
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    },
      });

      await userEvent.click(
        screen.getByRole("button", { name: /disconnect from voice/i }),
      );

      expect(onLeaveVoice).toHaveBeenCalledTimes(1);
    });

    it("names a channel it is connected to in another space", () => {
      // The reason the lookup walks every space. A voice channel stays joined
      // while somebody browses elsewhere, which is the point of a panel that
      // is always on screen.
      shell({
        rooms: {
          spaces: [
            { id: "home", name: "Home", avatar: null, channels: [] },
            {
              id: "!hq:example.org",
              name: "Kahu HQ",
              avatar: null,
              channels: [voice(LOUNGE, "Lounge")],
            },
          ],
        },
        call: {
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    },
      });

      expect(
        screen.getByRole("group", { name: /voice connection/i }),
      ).toHaveTextContent("Lounge");
    });

    it("draws a placeholder rather than a room id for a channel it cannot name", () => {
      shell({
        rooms: EMPTY_HOME,
        call: {
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    },
      });

      const panel = screen.getByRole("group", { name: /voice connection/i });
      expect(panel).toHaveTextContent(/voice channel/i);
      expect(panel).not.toHaveTextContent(LOUNGE);
    });

    it("floats a card for the call it is in", () => {
      // Wiring rather than behaviour: everything the card draws is already in
      // the shell, and a card that has to subscribe to anything is a card in
      // the wrong place.
      shell({
        rooms: withVoice,
        call: {
          state: "connected",
          roomId: LOUNGE,
          participants: [],
          trouble: null,
        },
      });

      expect(screen.getByRole("region", { name: "Call in Lounge" })).toBeVisible();
    });

    it("floats no card when there is no call", () => {
      shell({ rooms: withVoice });

      expect(screen.queryByRole("region", { name: /^Call in/ })).toBeNull();
    });

    describe("putting the card away and getting it back", () => {
      const IN_LOUNGE: Call = {
        state: "connected",
        roomId: LOUNGE,
        participants: [],
        trouble: null,
      };
      const card = () => screen.queryByRole("region", { name: /^Call in/ });
      /*
        Inside the voice strip, and named exactly. The channel list has a
        "Read Lounge without connecting" on it, which a looser match takes as
        well and which is not this.
      */
      const line = () =>
        within(screen.getByRole("group", { name: /voice connection/i })).getByRole(
          "button",
          { name: /^(voice connected|connecting)$/i },
        );

      it("brings the card back from the panel after it has been hidden", async () => {
        // The whole point. Hiding used to be a door that only shut: the card
        // drew nothing, its own controls went with it, and there was nothing
        // anywhere else that asked for it back short of rejoining the call.
        shell({ rooms: withVoice, call: IN_LOUNGE });

        await userEvent.click(
          screen.getByRole("button", { name: "Hide the call card" }),
        );
        expect(card()).toBeNull();

        await userEvent.click(line());

        expect(card()).toBeVisible();
      });

      it("puts the card away from the same control", async () => {
        // One control, both directions. Pressing it while the card is up has
        // to do something, or it is a button that is dead most of the time.
        shell({ rooms: withVoice, call: IN_LOUNGE });
        expect(card()).toBeVisible();

        await userEvent.click(line());

        expect(card()).toBeNull();
      });

      it("says which way it will go, all the way through", async () => {
        shell({ rooms: withVoice, call: IN_LOUNGE });
        expect(line()).toHaveAttribute("aria-expanded", "true");

        await userEvent.click(line());
        expect(line()).toHaveAttribute("aria-expanded", "false");

        await userEvent.click(line());
        expect(line()).toHaveAttribute("aria-expanded", "true");
      });

      it("brings the card back for the next call on its own", async () => {
        // Putting it away is about this call. A card that stayed shut until
        // somebody found the control again would be a setting nobody chose.
        const { again } = shell({ rooms: withVoice, call: IN_LOUNGE });

        await userEvent.click(
          screen.getByRole("button", { name: "Hide the call card" }),
        );
        expect(card()).toBeNull();

        again({ call: { state: "disconnected" } });
        again({ call: { state: "connecting", roomId: LOUNGE } });

        expect(card()).toBeVisible();
      });

      it("offers nothing to press when there is no call", () => {
        // The card only exists inside a call, so neither does the way back.
        shell({ rooms: withVoice });

        expect(
          screen.queryByRole("group", { name: /voice connection/i }),
        ).toBeNull();
        expect(
          screen.queryByRole("button", {
            name: /^(voice connected|connecting)$/i,
          }),
        ).toBeNull();
      });
    });

    it("shows no connection panel for a join that failed", () => {
      // There is no connection to put in it. What is worth saying is which
      // channel would not take the call, and that belongs beside the channel.
      shell({
        rooms: withVoice,
        call: { state: "failed", roomId: LOUNGE, error: "no voice server" },
      });

      expect(
        screen.queryByRole("group", { name: /voice connection/i }),
      ).toBeNull();
      expect(screen.getByRole("alert")).toHaveTextContent("no voice server");
    });
  });

  describe("a link to a room inside a message", () => {
    const TECH = "!tech:example.org";
    const GENERAL = "!general:example.org";
    const twoRooms: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [
            textChannel("!general:example.org", "general"),
            textChannel(TECH, "tech"),
          ],
        },
      ],
    };

    /** Open general and say one thing in it, with a link in what was said. */
    async function linked(body: string) {
      let publish: (timeline: Timeline) => void = () => {};
      onTimeline.mockImplementation((handler: (t: Timeline) => void) => {
        publish = handler;
        return Promise.resolve(() => {});
      });
      shell({ rooms: twoRooms });

      await userEvent.click(
        await screen.findByRole("button", { name: /general/i }),
      );
      await waitFor(() => expect(timelineOpen).toHaveBeenCalled());
      await act(async () => {
        publish({
          roomId: "!general:example.org",
          messages: [
            {
              id: "$1",
              sender: "@ada:example.org",
              at: Date.UTC(2026, 0, 1),
              body,
              kind: "text",
            },
          ],
          moreBefore: false,
          moreAfter: false,
          loading: false,
          loadingAfter: false,
        });
      });

      // Scoped to the conversation. The channel list draws a `#tech` of its
      // own, and the badge in the message is the one under test.
      return within(screen.getByRole("log"));
    }

    /*
      The room's end of the IPC, with the one rule that makes #105 cost
      something rather than merely look wrong.

      `consort_matrix::timeline` drops a sync that arrives while a window
      somebody jumped into is being drawn, because what was just said does not
      belong under a message from last March (`timeline/mod.rs`, and
      `what_is_said_while_reading_older_messages_does_not_land_under_them`
      covers it). So a room left in a window is a room that has silently
      stopped receiving, and a stale ask that puts every re-entry back into one
      is a person who is not being told they are missing messages.

      Watching a room reads it afresh, `Loaded::new` starting with no focus, so
      opening is what comes back out of a window.
    */
    function theRoom(roomId: string) {
      let publish: (timeline: Timeline) => void = () => {};
      let focused: string | null = null;
      let live: Message[] = [];
      let drawn: Message[] = [];

      const report = () => {
        publish({
          roomId,
          messages: [...drawn],
          moreBefore: false,
          moreAfter: false,
          loading: false,
          loadingAfter: false,
          ...(focused === null ? {} : { focus: focused }),
        });
      };

      onTimeline.mockImplementation((handler: (t: Timeline) => void) => {
        publish = handler;
        return Promise.resolve(() => {});
      });
      timelineOpen.mockImplementation((asked: string) => {
        if (asked === roomId) {
          focused = null;
          drawn = [...live];
          report();
        }
        return Promise.resolve(undefined);
      });
      timelineGoTo.mockImplementation((eventId: string) => {
        focused = eventId;
        drawn = [said(eventId, "last March")];
        report();
        return Promise.resolve(undefined);
      });

      return {
        /** Whatever was in the room before anybody opened it. */
        already(messages: Message[]) {
          live = [...messages];
        },
        /** A message arriving on a sync, dropped while a window is drawn. */
        arrives(message: Message) {
          live = [...live, message];
          if (focused !== null) return;
          drawn = [...live];
          report();
        },
      };
    }

    function said(id: string, body: string): Message {
      return {
        id,
        sender: "@ada:example.org",
        at: Date.UTC(2026, 0, 1),
        body,
        kind: "text",
      };
    }

    it("names the room the way the rest of the shell names it", async () => {
      // The hash included. A badge saying `tech` beside a heading saying
      // `#tech` reads as two different rooms.
      const said = await linked(`look at https://matrix.to/#/${TECH}`);

      expect(said.getByRole("button", { name: "#tech" })).toBeVisible();
    });

    it("shows the room when the badge is pressed", async () => {
      const said = await linked(`look at https://matrix.to/#/${TECH}`);

      await userEvent.click(said.getByRole("button", { name: "#tech" }));

      expect(roomAt).toHaveBeenCalledWith(TECH);
      expect(
        await screen.findByRole("heading", { level: 1, name: "#tech" }),
      ).toBeVisible();
    });

    /**
     * Follow a link to `$old`, from a message in general, and land in a window.
     *
     * Returns the room, so the test can go on saying things in it.
     */
    async function followedIntoAWindow() {
      const room = theRoom(GENERAL);
      room.already([said("$1", `look at https://matrix.to/#/${GENERAL}/$old`)]);
      shell({ rooms: twoRooms });

      await userEvent.click(
        await screen.findByRole("button", { name: /general/i }),
      );
      await screen.findByRole("heading", { level: 1, name: "#general" });
      roomAt.mockResolvedValue(GENERAL);

      const said1 = within(screen.getByRole("log"));
      await userEvent.click(said1.getByRole("button", { name: /message in/i }));
      await waitFor(() => expect(timelineGoTo).toHaveBeenCalledWith("$old"));
      await screen.findByText(/showing older messages/i);
      timelineGoTo.mockClear();
      return room;
    }

    /** Away to the other room and back, which unmounts and remounts the pane. */
    async function awayAndBack() {
      await userEvent.click(screen.getByRole("button", { name: /tech/i }));
      await screen.findByRole("heading", { level: 1, name: "#tech" });
      await userEvent.click(screen.getByRole("button", { name: /general/i }));
      await screen.findByRole("heading", { level: 1, name: "#general" });
    }

    it("does not go back into the window when the room is reopened", async () => {
      // #105. The pane is keyed on the room, so coming back mounts a fresh one
      // that reads the same ask as though it were a new press.
      await followedIntoAWindow();

      await awayAndBack();

      expect(timelineGoTo).not.toHaveBeenCalled();
    });

    it("goes on delivering messages after a link has been followed", async () => {
      /*
        What #105 actually costs. A room held in a window drops every sync, so
        somebody who followed one link is silently not receiving anything in
        that room for the rest of the session, and the only sign is a banner
        they have already read once and dismissed as expected.
      */
      const room = await followedIntoAWindow();

      await awayAndBack();
      await act(async () => {
        room.arrives(said("$2", "the thing they needed to hear"));
      });

      expect(
        await screen.findByText("the thing they needed to hear"),
      ).toBeVisible();
      expect(screen.queryByText(/showing older messages/i)).toBeNull();
    });

    it("says why a link went nowhere, rather than doing nothing", async () => {
      // The address was written by whoever sent the message, so an alias no
      // directory knows and a room this account is not in are both ordinary.
      roomAt.mockRejectedValue({
        message: "Nothing here could find a room at that address.",
        detail: "no",
      });
      const said = await linked("look at https://matrix.to/#/#gone:example.org");

      await userEvent.click(
        said.getByRole("button", { name: "#gone:example.org" }),
      );

      const complaint = await screen.findByRole("alert");
      expect(complaint).toHaveTextContent(/could find a room at that address/i);
      await userEvent.click(
        within(complaint).getByRole("button", { name: "Dismiss" }),
      );
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });

  describe("back and forward", () => {
    const GENERAL = "!general:example.org";
    const TECH = "!tech:example.org";

    const twoRooms: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [textChannel(GENERAL, "general"), textChannel(TECH, "tech")],
        },
      ],
    };

    it("comes back to the room you were reading", async () => {
      shell({ rooms: twoRooms });
      await userEvent.click(screen.getByRole("button", { name: /general/ }));
      await userEvent.click(screen.getByRole("button", { name: /tech/ }));

      await goBack();

      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "#general",
      );
    });

    it("goes forward again to the room you came back from", async () => {
      shell({ rooms: twoRooms });
      await userEvent.click(screen.getByRole("button", { name: /general/ }));
      await userEvent.click(screen.getByRole("button", { name: /tech/ }));
      await goBack();

      await goForward();

      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "#tech",
      );
    });

    it("comes back past the first room to the empty pane", async () => {
      // Where the shell opens is somewhere it was, so the room picked first
      // has something behind it rather than being the end of the line.
      shell({ rooms: twoRooms });
      await userEvent.click(screen.getByRole("button", { name: /general/ }));

      await goBack();

      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "Consort",
      );
    });

    it("comes back to the space you were in", async () => {
      const twoSpaces: Rooms = {
        spaces: [
          {
            id: "home",
            name: "Home",
            avatar: null,
            channels: [textChannel(GENERAL, "general")],
          },
          {
            id: "!hq:example.org",
            name: "Kahu HQ",
            avatar: null,
            channels: [textChannel(TECH, "tech")],
          },
        ],
      };
      shell({ rooms: twoSpaces });
      await userEvent.click(screen.getByRole("button", { name: /general/ }));
      await userEvent.click(screen.getByRole("button", { name: "Kahu HQ" }));

      await goBack();

      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "#general",
      );
    });

    it("comes back one room when the mouse's back button is pressed", async () => {
      // The whole of issue 60, end to end: the button on the side of a mouse
      // and the room it lands on. A press that moves twice lands on the empty
      // pane instead, which is what this looked like when it was wrong.
      shell({ rooms: twoRooms });
      await userEvent.click(screen.getByRole("button", { name: /general/ }));
      await userEvent.click(screen.getByRole("button", { name: /tech/ }));

      await pressBack();

      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "#general",
      );
    });

    it("does not join a voice channel on the way back to it", async () => {
      // Going back is looking at where you were, not clicking it again. A
      // traversal that reconnected would drag somebody into a call they left.
      const withVoice: Rooms = {
        spaces: [
          {
            id: "home",
            name: "Home",
            avatar: null,
            channels: [
              textChannel(GENERAL, "general"),
              voice("!lounge:example.org", "Lounge"),
            ],
          },
        ],
      };
      const { onJoinVoice } = shell({ rooms: withVoice });
      await userEvent.click(screen.getByRole("button", { name: "Lounge" }));
      await userEvent.click(screen.getByRole("button", { name: /general/ }));
      onJoinVoice.mockClear();

      await goBack();

      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "Lounge",
      );
      expect(onJoinVoice).not.toHaveBeenCalled();
    });
  });

  describe("the screen with no channel selected", () => {
    const LOUNGE = "!lounge:example.org";
    const TECH = "!tech:example.org";

    /** Home is selected; the other rail entry holds the rooms below. */
    const elsewhere: Rooms = {
      spaces: [
        {
          id: "home",
          name: "Home",
          avatar: null,
          channels: [textChannel("!general:example.org", "general")],
        },
        {
          id: "!kahu:example.org",
          name: "Kahu HQ",
          avatar: null,
          channels: [textChannel(TECH, "tech"), voice(LOUNGE, "Lounge")],
        },
      ],
    };

    it("opens a recent room that is under another rail entry", async () => {
      // The reason the pane hands back the entry as well as the channel.
      // Selecting a channel from the list beside it can assume the space it
      // was picked in; this cannot, because the pane lists every one of them.
      recentRooms.mockResolvedValue([TECH]);
      shell({ rooms: elsewhere });

      await userEvent.click(
        await screen.findByRole("button", { name: /#tech/ }),
      );

      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
        "#tech",
      );
    });

    it("joins a recent voice channel, the way the list beside it does", async () => {
      recentRooms.mockResolvedValue([LOUNGE]);
      const { onJoinVoice } = shell({ rooms: elsewhere });

      await userEvent.click(
        await screen.findByRole("button", { name: /Lounge/ }),
      );

      expect(onJoinVoice).toHaveBeenCalledWith(LOUNGE);
    });

    it("does not join a recent text room", async () => {
      recentRooms.mockResolvedValue([TECH]);
      const { onJoinVoice } = shell({ rooms: elsewhere });

      await userEvent.click(
        await screen.findByRole("button", { name: /#tech/ }),
      );

      expect(onJoinVoice).not.toHaveBeenCalled();
    });
  });
});
