import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const audioDevices = vi.hoisted(() => vi.fn());
const audioSettings = vi.hoisted(() => vi.fn());
const audioTestStart = vi.hoisted(() => vi.fn());
const audioTestStop = vi.hoisted(() => vi.fn());
const onAudio = vi.hoisted(() => vi.fn());
const logout = vi.hoisted(() => vi.fn());
const roomAvatar = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  audioDevices,
  audioSettings,
  audioTestStart,
  audioTestStop,
  onAudio,
  logout,
  roomAvatar,
}));

import { AppShell } from "./AppShell";
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
  return { id, name, kind: "voice", avatar: null, joined: true, participants: [] };
}

function textChannel(id: string, name: string): Channel {
  return { id, name, kind: "text", avatar: null, joined: true, participants: [] };
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
}: {
  rooms?: Rooms;
  call?: Call;
  selfAudio?: SelfAudio;
  onSignedOut?: ReturnType<typeof vi.fn>;
  onJoinVoice?: ReturnType<typeof vi.fn>;
  onLeaveVoice?: ReturnType<typeof vi.fn>;
  onSetMuted?: ReturnType<typeof vi.fn>;
  onSetDeafened?: ReturnType<typeof vi.fn>;
  onSetAway?: ReturnType<typeof vi.fn>;
  callRefused?: CallRefused | null;
  onDismissRefusal?: ReturnType<typeof vi.fn>;
} = {}) {
  const { container } = render(
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
      onSignedOut={onSignedOut}
    />,
  );
  return { container, onSignedOut, onJoinVoice, onLeaveVoice };
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
  });

  it("opens settings from the gear on the user strip", async () => {
    shell();

    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    expect(await screen.findByRole("dialog")).toBeVisible();
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
});
