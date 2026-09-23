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
  timelineSend,
  memberNames,
  roomAt,
  roomAvatar,
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
} = {}) {
  const { container, rerender } = render(
    <AppShell
      profile={profile}
      rooms={rooms}
      connection={{ state: "live" }}
      call={call}
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
      showRoom={showRoom}
      onSignedOut={onSignedOut}
    />,
  );
  return { container, rerender, onSignedOut, onJoinVoice, onLeaveVoice };
}

describe("AppShell", () => {
  beforeEach(() => {
    resetAvatarCache();
    audioDevices.mockReset().mockResolvedValue(report);
    audioSettings.mockReset().mockResolvedValue(settings);
    audioTestStart.mockReset().mockResolvedValue(undefined);
    audioTestStop.mockReset().mockResolvedValue(undefined);
    onAudio.mockReset().mockResolvedValue(() => {});
    logout.mockReset().mockResolvedValue(undefined);
    roomAvatar.mockReset().mockResolvedValue(null);
    onTimeline.mockReset().mockResolvedValue(() => {});
    onTyping.mockReset().mockResolvedValue(() => {});
    onDropped.mockReset().mockResolvedValue(() => {});
    timelineTyping.mockReset().mockResolvedValue(undefined);
    onThread.mockReset().mockResolvedValue(() => {});
    timelineOpen.mockReset().mockResolvedValue(undefined);
    timelineClose.mockReset().mockResolvedValue(undefined);
    timelineEarlier.mockReset().mockResolvedValue(undefined);
    timelineSend.mockReset().mockResolvedValue(undefined);
    memberNames.mockReset().mockResolvedValue({});
    roomAt.mockReset().mockResolvedValue("!tech:example.org");
    threadOpen.mockReset().mockResolvedValue(undefined);
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
        "Nothing here yet",
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
});
