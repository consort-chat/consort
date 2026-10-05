import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

// The people under a voice channel draw their avatars, which is a command.
// Mocked here rather than left to fail quietly, because an unmocked `invoke`
// throws into the catch that turns a missing picture into an initial, and the
// tests would still pass while exercising the wrong path.
const memberAvatar = vi.hoisted(() => vi.fn());
// The menu a name opens reads the saved levels and writes them back. Mocked
// for the same reason the avatar is: an unmocked `invoke` throws into a catch
// and the test would pass having exercised the failure path.
const audioSettings = vi.hoisted(() => vi.fn());
const setPersonVolume = vi.hoisted(() => vi.fn());
// Which sections are folded is a preference in the settings file, so the list
// reads it on mount and writes a press down. Answered for every test rather
// than only the ones about folding, because every render reaches it.
const sidebarSettings = vi.hoisted(() => vi.fn());
const setSectionFolded = vi.hoisted(() => vi.fn());
const setSectionOrder = vi.hoisted(() => vi.fn());
// #170's four, for the same reason: every render reads the sections back and
// every press on one writes.
const createSection = vi.hoisted(() => vi.fn());
const renameSection = vi.hoisted(() => vi.fn());
const deleteSection = vi.hoisted(() => vi.fn());
const setRoomSection = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberAvatar,
  audioSettings,
  setPersonVolume,
  sidebarSettings,
  setSectionFolded,
  setSectionOrder,
  createSection,
  renameSection,
  deleteSection,
  setRoomSection,
}));

import { ChannelList } from "./ChannelList";
import type { Call, Channel, Participant, Space } from "../lib/api";
import { resetAvatarCache } from "../lib/avatars";

function text(id: string, name: string | null, joined = true): Channel {
  return {
    id,
    name,
    kind: "text",
    avatar: null,
    joined,
    participants: [],
    unread: 0,
    mentions: 0,
  };
}

function voice(
  id: string,
  name: string,
  participants: Participant[] = [],
  joined = true,
): Channel {
  return {
    id,
    name,
    kind: "voice",
    avatar: null,
    joined,
    participants,
    unread: 0,
    mentions: 0,
  };
}

function person(id: string, name: string, muted = false): Participant {
  return { id, name, muted };
}

/** Somebody in the call with their camera on. */
function onCamera(id: string, name: string): Participant {
  return { id, name, muted: false, camera: true };
}

function space(channels: Channel[], name = "Kahu HQ"): Space {
  return { id: "!s:example.org", name, avatar: null, channels };
}

/** The names in one group, in the order they are drawn. */
function namesIn(label: string): string[] {
  return within(screen.getByRole("region", { name: label }))
    .getAllByRole("button")
    // The rows themselves. A voice row also carries the control that reads it
    // without connecting, and the people in it are controls of their own.
    .filter((button) => button.classList.contains("channels__entry"))
    .map((button) => button.textContent ?? "");
}

const PNG = "data:image/png;base64,iVBORw0KGgo=";

/** No call, which is what almost every test here is about. */
const IDLE: Call = { state: "disconnected" };

const LOUNGE = "!lounge:example.org";

/** A sidebar nobody has touched, which is what `sidebarSettings` answers with. */
const STORED = { folded: [], order: [], sections: [] };

/** A section somebody made under the space these tests render. */
function made(key: string, name: string, rooms: string[] = []) {
  return { key, name, space: "!s:example.org", rooms };
}

/** What `audioSettings` answers with, for the menu a name opens. */
const SETTINGS = {
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
  callSounds: false,
  callVoices: true,
  outputVolume: 100,
  notificationVolume: 60,
  personVolumes: {},
};

describe("ChannelList", () => {
  beforeEach(() => {
    resetAvatarCache();
    memberAvatar.mockReset().mockResolvedValue(null);
    // Every name is now a control that opens a menu, and the menu reads the
    // saved levels. Answered for every test rather than only the ones about
    // the menu, because any click on a name reaches it.
    audioSettings.mockReset().mockResolvedValue(SETTINGS);
    setPersonVolume.mockReset().mockResolvedValue(undefined);
    sidebarSettings.mockReset().mockResolvedValue(STORED);
    setSectionFolded.mockReset().mockResolvedValue(undefined);
    setSectionOrder.mockReset().mockResolvedValue(undefined);
    createSection.mockReset().mockResolvedValue("custom-1");
    renameSection.mockReset().mockResolvedValue(undefined);
    deleteSection.mockReset().mockResolvedValue(undefined);
    setRoomSection.mockReset().mockResolvedValue(undefined);
  });

  /** Render one space's channels, with the props every test shares. */
  function list(channels: Channel[]) {
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space(channels)}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );
  }

  it("marks a channel with something waiting in it", () => {
    list([{ ...text("!a:example.org", "general"), unread: 4 }]);

    expect(
      screen.getByRole("button", { name: /general/ }),
    ).toHaveAttribute("data-unread", "true");
  });

  it("leaves a channel that has been read unmarked", () => {
    list([text("!a:example.org", "general")]);

    expect(
      screen.getByRole("button", { name: /general/ }),
    ).toHaveAttribute("data-unread", "false");
  });

  it("counts what is waiting in an unread channel", () => {
    list([{ ...text("!a:example.org", "general"), unread: 12 }]);

    expect(
      screen.getByLabelText("12 unread messages in general"),
    ).toHaveTextContent("12");
  });

  it("says one unread message in the singular", () => {
    list([{ ...text("!a:example.org", "general"), unread: 1 }]);

    expect(
      screen.getByLabelText("1 unread message in general"),
    ).toBeInTheDocument();
  });

  it("draws no unread count on a channel that has been read", () => {
    list([text("!a:example.org", "general")]);

    expect(
      screen.queryByLabelText(/unread message/),
    ).not.toBeInTheDocument();
  });

  it("stops counting unread messages at the same point mentions stop", () => {
    list([{ ...text("!a:example.org", "general"), unread: 300 }]);

    expect(screen.getByText("99+")).toBeInTheDocument();
  });

  it("counts the times somebody said your name", () => {
    list([{ ...text("!a:example.org", "general"), unread: 9, mentions: 3 }]);

    expect(
      screen.getByLabelText("3 mentions in general"),
    ).toHaveTextContent("3");
  });

  it("says one mention in the singular", () => {
    list([{ ...text("!a:example.org", "general"), unread: 1, mentions: 1 }]);

    expect(screen.getByLabelText("1 mention in general")).toBeInTheDocument();
  });

  it("stops counting mentions past the point the number stops helping", () => {
    // Past this the number is width rather than information, and a badge that
    // grew to four digits would push the channel name off the row.
    list([{ ...text("!a:example.org", "general"), unread: 300, mentions: 214 }]);

    expect(screen.getByText("99+")).toBeInTheDocument();
  });

  it("gives the badge to the mention when a channel has both", () => {
    // One slot, and the mention has first claim on it. A mention already
    // implies unread, so a second number beside it counts something the
    // reader has worked out, and two on one row is a list to read rather
    // than scan.
    list([{ ...text("!a:example.org", "general"), unread: 9, mentions: 3 }]);

    expect(screen.getByLabelText("3 mentions in general")).toBeInTheDocument();
    expect(screen.queryByLabelText(/unread message/)).not.toBeInTheDocument();
    expect(screen.queryByText("9")).not.toBeInTheDocument();
  });

  it("still marks a mentioned channel as unread", () => {
    // The name in white is the whole unread signal on a row whose badge went
    // to the mentions, so it had better still be there.
    list([{ ...text("!a:example.org", "general"), unread: 9, mentions: 3 }]);

    expect(screen.getByRole("button", { name: /general/ })).toHaveAttribute(
      "data-unread",
      "true",
    );
  });

  it("names the space at the top", () => {
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([text("!a:example.org", "general")])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(screen.getByText("Kahu HQ")).toBeVisible();
  });

  it("splits text from voice", () => {
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([
          text("!a:example.org", "general"),
          voice("!b:example.org", "Lounge"),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(namesIn("Text")).toEqual(["#general"]);
    expect(namesIn("Voice")).toEqual(["Lounge"]);
  });

  it("keeps the order it was given inside each group", () => {
    // Filtering preserves the order the snapshot decided; sorting each group
    // separately would not, and the order is MSC1772's rather than ours.
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([
          text("!c:example.org", "zulu"),
          voice("!d:example.org", "Zulu Voice"),
          text("!a:example.org", "alpha"),
          voice("!b:example.org", "Alpha Voice"),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(namesIn("Text")).toEqual(["#zulu", "#alpha"]);
    expect(namesIn("Voice")).toEqual(["Zulu Voice", "Alpha Voice"]);
  });

  it("omits a group with nothing in it", () => {
    // A "Voice" header over nothing reads as a list that failed to load.
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([text("!a:example.org", "general")])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole("region", { name: "Voice" }),
    ).not.toBeInTheDocument();
  });

  it("says so when a space has no channels at all", () => {
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(screen.getByText(/nothing in here yet/i)).toBeVisible();
  });

  it("marks the selected channel as the current one", () => {
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([
          text("!a:example.org", "general"),
          text("!b:example.org", "random"),
        ])}
        selectedId="!b:example.org"
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "#random" })).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(
      screen.getByRole("button", { name: "#general" }),
    ).not.toHaveAttribute("aria-current");
  });

  it("reports which channel was clicked", async () => {
    const onSelect = vi.fn();
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([voice("!v:example.org", "Lounge")])}
        selectedId={null}
        call={IDLE}
        onSelect={onSelect}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Lounge" }));

    expect(onSelect).toHaveBeenCalledWith("!v:example.org");
  });

  it("offers a channel this account never joined as one to join", () => {
    // What was here was a row that could not be clicked, which is #128: the
    // space says the channel exists and every other client lets somebody walk
    // into it.
    const onSelect = vi.fn();
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        space={space([text("!never:example.org", null, false)])}
        selectedId={null}
        call={IDLE}
        onSelect={onSelect}
        onJoin={vi.fn()}
      />,
    );

    const entry = screen.getByRole("button", { name: /join unknown channel/i });
    expect(entry).toBeEnabled();
  });

  it("joins a channel this account is not in rather than selecting it", async () => {
    // Two different things from one row. Selecting a room nobody is in would
    // open a timeline of nothing, so the click asks to be let in first.
    const onSelect = vi.fn();
    const onJoin = vi.fn();
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        space={space([text("!never:example.org", "announcements", false)])}
        selectedId={null}
        call={IDLE}
        onSelect={onSelect}
        onJoin={onJoin}
      />,
    );

    await userEvent.click(
      screen.getByRole("button", { name: /join announcements/i }),
    );

    expect(onJoin).toHaveBeenCalledWith("!never:example.org");
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("says a join is in flight, and will not send a second one", async () => {
    const onJoin = vi.fn();
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        space={space([text("!never:example.org", "announcements", false)])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
        onJoin={onJoin}
        joining={{ roomId: "!never:example.org", problem: null }}
      />,
    );

    const entry = screen.getByRole("button", { name: /joining announcements/i });
    expect(entry).toBeDisabled();

    await userEvent.click(entry);
    expect(onJoin).not.toHaveBeenCalled();
  });

  it("leaves the other channels clickable while one is joining", async () => {
    // One join in flight is not the list going away. Disabling everything
    // would make a slow homeserver look like a frozen application.
    const onJoin = vi.fn();
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        space={space([
          text("!never:example.org", "announcements", false),
          text("!other:example.org", "notices", false),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
        onJoin={onJoin}
        joining={{ roomId: "!never:example.org", problem: null }}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: /join notices/i }));

    expect(onJoin).toHaveBeenCalledWith("!other:example.org");
  });

  it("says why a join did not work, beside the channel it was for", () => {
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        space={space([
          text("!never:example.org", "announcements", false),
          text("!other:example.org", "notices", false),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
        onJoin={vi.fn()}
        joining={{
          roomId: "!never:example.org",
          problem: "That channel is invite only.",
        }}
      />,
    );

    // `getByRole` rather than `getAllByRole`, because it throws on a second
    // one: the refusal belongs to the row it was about, and a sentence under
    // every unjoined channel would be the list saying it four times.
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("That channel is invite only.");
    const row = alert.closest("li");
    expect(row).toHaveTextContent("announcements");
    expect(row).not.toHaveTextContent("notices");
  });

  it("offers a refused channel again rather than leaving it dead", async () => {
    // A room that was invite only this morning can be one somebody has since
    // been asked into, and a control that gave up after one refusal would
    // need the application restarting to try again.
    const onJoin = vi.fn();
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        space={space([text("!never:example.org", "announcements", false)])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
        onJoin={onJoin}
        joining={{
          roomId: "!never:example.org",
          problem: "That channel is invite only.",
        }}
      />,
    );

    await userEvent.click(
      screen.getByRole("button", { name: /join announcements/i }),
    );

    expect(onJoin).toHaveBeenCalledWith("!never:example.org");
  });

  it("reads a voice channel without connecting to it", async () => {
    // The messages in a voice room are a room like any other, and connecting
    // to the call should not be the price of reading them.
    const onSelect = vi.fn();
    const onOpenRoom = vi.fn();
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={onOpenRoom}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([voice(LOUNGE, "Lounge")])}
        selectedId={null}
        call={IDLE}
        onSelect={onSelect}
      />,
    );

    await userEvent.click(
      screen.getByRole("button", { name: /read lounge without connecting/i }),
    );

    expect(onOpenRoom).toHaveBeenCalledWith(LOUNGE);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("offers nothing of the kind on a text channel", () => {
    // Clicking one already does exactly this, and a second control that
    // repeated it would be a control with nothing to say.
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([text("!a:example.org", "general")])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole("button", { name: /without connecting/i }),
    ).toBeNull();
  });

  it("offers nothing of the kind on a channel this account is not in", () => {
    // There is nothing to read. The row itself offers the way in.
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([voice(LOUNGE, "Lounge", [], false)])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole("button", { name: /without connecting/i }),
    ).toBeNull();
  });

  it("shows who is in a voice channel without anybody opening it", () => {
    // The whole point of this half of the feature: presence is a read of room
    // state, so it is on screen before anything is clicked or connected to.
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([
          voice("!v:example.org", "Lounge", [
            person("@ada:example.org", "Ada"),
            person("@ben:example.org", "Ben"),
          ]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    const people = within(screen.getByRole("list", { name: "In Lounge" }));
    expect(
      people.getAllByRole("listitem").map((item) => item.textContent),
    ).toEqual(["AAda", "BBen"]);
  });

  it("draws people in the order it was given", () => {
    // Oldest membership first, decided in Rust. Re-sorting here would make the
    // list move under the pointer every time somebody joined.
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([
          voice("!v:example.org", "Lounge", [
            person("@zoe:example.org", "Zoe"),
            person("@ada:example.org", "Ada"),
          ]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    const people = within(screen.getByRole("list", { name: "In Lounge" }));
    expect(
      people.getAllByRole("listitem").map((item) => item.textContent),
    ).toEqual(["ZZoe", "AAda"]);
  });

  it("draws nothing under a voice channel nobody is in", () => {
    // An empty voice channel has to keep exactly the shape it had before
    // presence existed, or every quiet channel gains a gap under it.
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([voice("!v:example.org", "Lounge")])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole("list", { name: "In Lounge" }),
    ).not.toBeInTheDocument();
  });

  it("does not turn a person into a way to open the channel", async () => {
    // Nesting the list inside the button would make every name a target that
    // opens the room, which is not what clicking somebody should ever mean.
    const onSelect = vi.fn();
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([
          voice("!v:example.org", "Lounge", [person("@ada:example.org", "Ada")]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={onSelect}
      />,
    );

    await userEvent.click(screen.getByText("Ada"));

    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Lounge" })).toBeVisible();
  });

  it("opens the person menu clear of the column it was opened from", async () => {
    // Not under the pointer, which is where it used to open. WebKitGTK paints a
    // scroll container's scrollbar on top of the page rather than inside it, so
    // a card overlapping this column came out with a grey bar down the middle
    // of it and no `z-index` could lift it clear. Staying off the column is the
    // only fix that holds, and it is also where every other client puts it.
    const { container } = render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([
          voice("!v:example.org", "Lounge", [person("@ada:example.org", "Ada")]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );
    const column = container.querySelector(".channels");
    if (column === null) throw new Error("no channel list to measure");
    column.getBoundingClientRect = () => new DOMRect(0, 0, 240, 600);

    await userEvent.click(screen.getByText("Ada"));

    const card = screen.getByRole("dialog", { name: "Ada" });
    expect(card).toHaveStyle({ left: "240px" });
  });

  it("asks for a person's picture in the room they are in", async () => {
    // A Matrix profile is per room, so the room is half of the question. The
    // answer replaces the initial in place.
    memberAvatar.mockResolvedValue(PNG);
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([
          voice("!v:example.org", "Lounge", [person("@ada:example.org", "Ada")]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(memberAvatar).toHaveBeenCalledWith(
        "!v:example.org",
        "@ada:example.org",
      );
    });
    await waitFor(() => {
      expect(document.querySelector(".avatar__image")).toHaveAttribute(
        "src",
        PNG,
      );
    });
  });

  it("falls back to an initial for somebody with no picture", async () => {
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([
          voice("!v:example.org", "Lounge", [person("@ada:example.org", "Ada")]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    await waitFor(() => expect(memberAvatar).toHaveBeenCalled());
    expect(document.querySelector(".avatar__image")).toBeNull();
    expect(screen.getByText("A")).toBeVisible();
  });

  it("never puts a room ID where a name goes", () => {
    // The whole reason `name` is nullable rather than defaulting to the id.
    render(
      <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
        space={space([text("!never:example.org", null, false)])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(screen.queryByText(/!never:example\.org/)).not.toBeInTheDocument();
  });

  describe("the call this session is in", () => {

    function withLounge() {
      return space([
        text("!a:example.org", "general"),
        voice(LOUNGE, "Lounge"),
        voice("!b:example.org", "Music"),
      ]);
    }

    function entryFor(name: string | RegExp): HTMLElement {
      return screen.getByRole("button", { name });
    }

    it("marks the channel it is connected to", () => {
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={withLounge()}
          selectedId={null}
          call={{
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    }}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Lounge")).toHaveAttribute("data-call", "connected");
      expect(entryFor("Music")).not.toHaveAttribute("data-call");
    });

    it("marks the channel it is still joining", () => {
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={withLounge()}
          selectedId={null}
          call={{ state: "connecting", roomId: LOUNGE }}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Lounge")).toHaveAttribute("data-call", "connecting");
    });

    it("keeps being in a channel separate from having it selected", () => {
      // A voice channel stays joined while somebody clicks around the list.
      // If the two looked the same, clicking elsewhere would read as leaving.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={withLounge()}
          selectedId="!a:example.org"
          call={{
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    }}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Lounge")).toHaveAttribute("data-selected", "false");
      expect(entryFor("Lounge")).toHaveAttribute("data-call", "connected");
      expect(entryFor(/general/)).toHaveAttribute("data-selected", "true");
      expect(entryFor(/general/)).not.toHaveAttribute("data-call");
    });

    it("puts the reason a join failed beside the channel that refused it", () => {
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={withLounge()}
          selectedId={null}
          call={{
            state: "failed",
            roomId: LOUNGE,
            error: "no voice server would take this call",
          }}
          onSelect={vi.fn()}
        />,
      );

      const problem = screen.getByRole("alert");
      expect(problem).toHaveTextContent("no voice server would take this call");
      // The channel's own list item, which is what "beside" means here: the
      // row above it, and the people in it below.
      expect(entryFor("Lounge").closest("li")).toContainElement(problem);
    });

    it("says nothing about a failure in the channel that did not fail", () => {
      // The room id on the failure is what makes this possible. A second
      // channel can be clicked while the first is still connecting.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={withLounge()}
          selectedId={null}
          call={{ state: "failed", roomId: LOUNGE, error: "no voice server" }}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Music").parentElement).not.toHaveTextContent(
        "no voice server",
      );
    });

    it("draws the call's own roster for the channel it is in", () => {
      // Better than room state for this one channel, and only this one. The
      // call roster comes from MatrixRTC signalling, so it is right in every
      // generation; room state is only right in the oldest.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge", [person("@stale:example.org", "Stale")])])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada")],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      const people = screen.getByRole("list", { name: "In Lounge" });
      expect(people).toHaveTextContent("Ada");
      expect(people).not.toHaveTextContent("Stale");
    });

    it("marks somebody who has muted themselves", () => {
      // Comes from the SFU rather than from anything this session did, so it
      // is the one thing here that is true of other people. Named as well as
      // drawn: a colour with no glyph beside it is nothing to a screen reader,
      // and a glyph with no name on it is nothing either.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              person("@ada:example.org", "Ada", true),
              person("@bob:example.org", "Bob"),
            ],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is muted")).toBeVisible();
      expect(screen.queryByLabelText("Bob is muted")).toBeNull();
    });

    it("shows headphones rather than a microphone for somebody deafened", () => {
      // One icon, not two. Deafening mutes, so both flags are set on the same
      // person, and the headphones are the stronger claim: somebody muted may
      // still be listening, somebody deafened is not.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              { id: "@ada:example.org", name: "Ada", muted: true, deafened: true },
            ],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is deafened")).toBeVisible();
      expect(screen.queryByLabelText("Ada is muted")).toBeNull();
    });

    it("shows a clock for somebody who is away", () => {
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              { id: "@ada:example.org", name: "Ada", muted: true, away: true },
            ],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is away")).toBeVisible();
      // Beside the microphone rather than in place of it. Away mutes, so both
      // flags are set, and where somebody went is not what they switched off.
      expect(screen.getByLabelText("Ada is muted")).toBeVisible();
    });

    it("shows both headphones and a clock for somebody away and deafened", () => {
      // The one point where all three flags are true. Headphones outrank the
      // microphone, which is one fact about audio drawn once, and the clock is
      // a different fact and keeps its own place at the end of the row.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              {
                id: "@ada:example.org",
                name: "Ada",
                muted: true,
                deafened: true,
                away: true,
              },
            ],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is deafened")).toBeVisible();
      expect(screen.getByLabelText("Ada is away")).toBeVisible();
    });

    it("says nothing about being away for somebody who has only muted", () => {
      // Away is Consort clients telling each other over the call's data
      // channel. Somebody in Element Call says nothing, and guessing would put
      // a clock beside a person sitting right there.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada", true)],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.queryByLabelText(/is away/)).toBeNull();
    });

    it("says nothing about deafening for somebody who has only muted", () => {
      // Deafening is Consort clients telling each other over the call's data
      // channel. Somebody in Element Call says nothing, and guessing would put
      // headphones beside a person who can hear perfectly well.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada", true)],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is muted")).toBeVisible();
      expect(screen.queryByLabelText(/is deafened/)).toBeNull();
    });

    it("marks who is talking", () => {
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              person("@ada:example.org", "Ada"),
              person("@bob:example.org", "Bob"),
            ],
            trouble: null,
          }}
          speaking={new Set(["@ada:example.org"])}
          onSelect={vi.fn()}
        />,
      );

      const people = within(
        screen.getByRole("list", { name: "In Lounge" }),
      ).getAllByRole("listitem");

      expect(people[0]).toHaveAttribute("data-speaking", "true");
      expect(people[1]).toHaveAttribute("data-speaking", "false");
    });

    it("marks this session's own user when they are the one talking", () => {
      // We are measured the same way everybody else is, from the frames on
      // their way out. Excluding ourselves would leave the one person most
      // likely to be looking for the ring as the only one who never gets it.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@me:example.org", "Me")],
            trouble: null,
          }}
          speaking={new Set(["@me:example.org"])}
          onSelect={vi.fn()}
        />,
      );

      const [me] = within(
        screen.getByRole("list", { name: "In Lounge" }),
      ).getAllByRole("listitem");

      expect(me).toHaveAttribute("data-speaking", "true");
    });

    it("marks nobody when nobody is talking", () => {
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada")],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      const [ada] = within(
        screen.getByRole("list", { name: "In Lounge" }),
      ).getAllByRole("listitem");

      expect(ada).toHaveAttribute("data-speaking", "false");
    });

    it("says nothing about mute for a channel this session is not in", () => {
      // Room state lists who is in a channel and nothing else about them. An
      // unmuted mark there would be a finding rather than a silence, and it
      // would be wrong for anybody who had in fact muted.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge", [person("@ada:example.org", "Ada")])])}
          selectedId={null}
          call={{ state: "disconnected" }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByRole("list", { name: "In Lounge" })).toHaveTextContent(
        "Ada",
      );
      expect(screen.queryByLabelText(/is muted/)).toBeNull();
    });

    it("leaves every other channel on room state", () => {
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([
            voice(LOUNGE, "Lounge", [person("@ada:example.org", "Ada")]),
            voice("!b:example.org", "Music", [person("@bob:example.org", "Bob")]),
          ])}
          selectedId={null}
          call={{
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByRole("list", { name: "In Music" })).toHaveTextContent(
        "Bob",
      );
      // And the joined channel really did take the call's answer, which is
      // that nobody is in it yet.
      expect(
        screen.queryByRole("list", { name: "In Lounge" }),
      ).toBeNull();
    });

    it("keeps room state while a join is still in flight", () => {
      // A connecting call has no roster yet and a failed one never will.
      // Blanking the list for either would wipe something that was correct a
      // second ago and put it back when the join lands.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge", [person("@ada:example.org", "Ada")])])}
          selectedId={null}
          call={{ state: "connecting", roomId: LOUNGE }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByRole("list", { name: "In Lounge" })).toHaveTextContent(
        "Ada",
      );
    });

    it("marks nothing when there is no call", () => {
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={withLounge()}
          selectedId={null}
          call={IDLE}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Lounge")).not.toHaveAttribute("data-call");
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });

  describe("cameras", () => {
    /** The call this session is in, with `people` in it. */
    function inTheCall(people: Participant[]) {
      return (
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
        onJoin={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: people,
            trouble: null,
          }}
          onSelect={vi.fn()}
        />
      );
    }

    it("crosses out the camera of somebody who has theirs off", () => {
      render(inTheCall([person("@ada:example.org", "Ada")]));

      expect(
        screen.getByLabelText("Ada has their camera off"),
      ).toBeVisible();
    });

    it("draws a plain camera for somebody who has theirs on", () => {
      render(inTheCall([onCamera("@ada:example.org", "Ada")]));

      expect(screen.getByLabelText("Ada has their camera on")).toBeVisible();
      expect(
        screen.queryByLabelText("Ada has their camera off"),
      ).toBeNull();
    });

    it("says nothing about the camera of somebody in another channel", () => {
      // The one place a cross would be an invention rather than a reading.
      // Room state carries nothing about cameras, so "off" and "nobody looked"
      // would be drawn identically and only one of them would be true.
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
          space={space([
            voice(LOUNGE, "Lounge", [person("@ada:example.org", "Ada")]),
          ])}
          selectedId={null}
          call={IDLE}
          onSelect={vi.fn()}
          onJoin={vi.fn()}
        />,
      );

      expect(
        screen.queryByLabelText("Ada has their camera off"),
      ).toBeNull();
      expect(screen.queryByLabelText("Ada has their camera on")).toBeNull();
    });

    it("draws the camera beside the mute rather than instead of it", () => {
      // Two questions, not one slot. Somebody muted with their camera on is
      // ordinary, and a precedence between them would hide one of the two.
      render(inTheCall([{ id: "@ada:example.org", name: "Ada", muted: true, camera: true }]));

      expect(screen.getByLabelText("Ada is muted")).toBeVisible();
      expect(screen.getByLabelText("Ada has their camera on")).toBeVisible();
    });
  });

  describe("the menu on a name", () => {
    function withAda() {
      return (
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada")],
            trouble: null,
          }}
          onSelect={vi.fn()}
          onJoin={vi.fn()}
        />
      );
    }

    it("opens on a right-click, which is where anybody looks for it", async () => {
      render(withAda());

      await userEvent.pointer({
        keys: "[MouseRight]",
        target: screen.getByRole("button", { name: /Ada/ }),
      });

      expect(await screen.findByRole("dialog", { name: /Ada/ })).toBeVisible();
    });

    it("opens on an ordinary click too", async () => {
      // A touchpad without a second button, and a keyboard, both have to be
      // able to reach it. Making the row a button is what buys that.
      render(withAda());

      await userEvent.click(screen.getByRole("button", { name: /Ada/ }));

      expect(await screen.findByRole("dialog", { name: /Ada/ })).toBeVisible();
    });

    it("does not join the channel when a name is clicked", async () => {
      // The names sit under the control that opens the channel. Clicking one
      // and finding yourself in a call would be the worst possible surprise.
      const onSelect = vi.fn();
      render(
        <ChannelList
        selfId="@bob:example.org"
        onOpenRoom={vi.fn()}
        onFold={vi.fn()}
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada")],
            trouble: null,
          }}
          onSelect={onSelect}
          onJoin={vi.fn()}
        />,
      );

      await userEvent.click(screen.getByRole("button", { name: /Ada/ }));

      await screen.findByRole("dialog", { name: /Ada/ });
      expect(onSelect).not.toHaveBeenCalled();
    });

    it("closes again", async () => {
      render(withAda());
      await userEvent.click(screen.getByRole("button", { name: /Ada/ }));
      await screen.findByRole("dialog", { name: /Ada/ });

      await userEvent.keyboard("{Escape}");

      await waitFor(() =>
        expect(screen.queryByRole("dialog", { name: /Ada/ })).toBeNull(),
      );
    });
  });

  describe("folding a section away", () => {
    /** One of each kind, which is what folding one of them is visible against. */
    function both() {
      list([text("!a:example.org", "general"), voice(LOUNGE, "Lounge")]);
    }

    /** The control in a section's heading. */
    function heading(label: string): HTMLElement {
      return screen.getByRole("button", { name: label });
    }

    it("hides the channels in the section that was pressed", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      await userEvent.click(heading("Voice"));

      expect(screen.queryByRole("button", { name: "Lounge" })).toBeNull();
      // The other section is untouched. A control that folded both would be
      // one control for two sections.
      expect(screen.getByRole("button", { name: "#general" })).toBeVisible();
    });

    it("draws a section the settings file says is folded", async () => {
      // The whole point of writing it down. A fold that came back open is the
      // preference not being remembered.
      sidebarSettings.mockResolvedValue({ ...STORED, folded: ["voice"] });
      both();

      await waitFor(() =>
        expect(screen.queryByRole("button", { name: "Lounge" })).toBeNull(),
      );
      expect(screen.getByRole("button", { name: "#general" })).toBeVisible();
    });

    it("writes a fold down", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      await userEvent.click(heading("Voice"));

      expect(setSectionFolded).toHaveBeenCalledWith("voice", true);
    });

    it("unfolds a folded one, and writes that down too", async () => {
      sidebarSettings.mockResolvedValue({ ...STORED, folded: ["voice"] });
      both();
      await waitFor(() =>
        expect(screen.queryByRole("button", { name: "Lounge" })).toBeNull(),
      );

      await userEvent.click(heading("Voice"));

      expect(setSectionFolded).toHaveBeenCalledWith("voice", false);
      expect(screen.getByRole("button", { name: "Lounge" })).toBeVisible();
    });

    it("says whether the section it heads is open", async () => {
      // The state is on the control rather than in its name, the way the call
      // panel does it: a button renamed under the cursor is announced as a
      // different button each press.
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      expect(heading("Voice")).toHaveAttribute("aria-expanded", "true");

      await userEvent.click(heading("Voice"));

      expect(heading("Voice")).toHaveAttribute("aria-expanded", "false");
    });

    it("names the list it folds, so the heading points at it", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      const controls = heading("Voice").getAttribute("aria-controls");

      expect(controls).not.toBeNull();
      expect(document.getElementById(controls ?? "")).toBeVisible();
    });

    it("draws every section open when the preference cannot be read", async () => {
      // Failing to remember a fold is not a reason to draw a sidebar with
      // nothing in it.
      sidebarSettings.mockReset().mockRejectedValue(new Error("no file"));
      const logged = vi.spyOn(console, "error").mockImplementation(() => {});
      both();

      expect(screen.getByRole("button", { name: "Lounge" })).toBeVisible();
      expect(screen.getByRole("button", { name: "#general" })).toBeVisible();
      await waitFor(() =>
        expect(logged).toHaveBeenCalledWith(
          expect.stringContaining("folded"),
          expect.anything(),
        ),
      );
    });

    it("stays folded when the write fails", async () => {
      // The press has already moved the list by then. Putting it back because
      // the file would not take the fold would be a press that undoes itself.
      setSectionFolded.mockReset().mockRejectedValue(new Error("read only"));
      const logged = vi.spyOn(console, "error").mockImplementation(() => {});
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      await userEvent.click(heading("Voice"));

      expect(screen.queryByRole("button", { name: "Lounge" })).toBeNull();
      await waitFor(() =>
        expect(logged).toHaveBeenCalledWith(
          expect.stringContaining("remember"),
          expect.anything(),
        ),
      );
    });
  });
  describe("dragging a section into a different order", () => {
    /** One of each kind, so an order is something that can be read off. */
    function both() {
      list([text("!a:example.org", "general"), voice(LOUNGE, "Lounge")]);
    }

    /** The headings in the order they are drawn. */
    function order(): string[] {
      return screen
        .getAllByRole("region")
        .map((section) => section.getAttribute("aria-label") ?? "");
    }

    /** The handle in a section's heading. */
    function grip(label: string): HTMLElement {
      return screen.getByRole("button", { name: `Move ${label}` });
    }

    /** The section itself, which is what a drag is dropped on. */
    function region(label: string): HTMLElement {
      return screen.getByRole("region", { name: label });
    }

    /**
     * A clipboard for a drag, which jsdom does not provide.
     *
     * It really stores, so that a drag started on a handle is carried by
     * whatever that handle wrote rather than by what the test wanted. `types`
     * is a getter for the same reason: it is the half readable mid-drag, and
     * it is what decides whether a drop is offered at all.
     */
    function carrying(payload: Record<string, string> = {}) {
      return {
        get types() {
          return Object.keys(payload);
        },
        getData: (type: string) => payload[type] ?? "",
        setData: (type: string, value: string) => {
          payload[type] = value;
        },
        effectAllowed: "all",
        dropEffect: "none",
      };
    }

    /** Drag one section onto another, by heading, the way a pointer would. */
    function drag(from: string, onto: string) {
      const dataTransfer = carrying();
      fireEvent.dragStart(grip(from), { dataTransfer });
      fireEvent.dragOver(region(onto), { dataTransfer });
      fireEvent.drop(region(onto), { dataTransfer });
    }

    it("draws text above voice until somebody says otherwise", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      expect(order()).toEqual(["Text", "Voice"]);
    });

    it("draws the order the settings file holds", async () => {
      // The whole point of writing it down. Sections back where they started
      // is the drag not being remembered.
      sidebarSettings.mockResolvedValue({ ...STORED, order: ["voice", "text"] });
      both();

      await waitFor(() => expect(order()).toEqual(["Voice", "Text"]));
    });

    it("moves a section dropped on another one into its place", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      drag("Voice", "Text");

      expect(order()).toEqual(["Voice", "Text"]);
    });

    it("writes the whole new order down", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      drag("Voice", "Text");

      expect(setSectionOrder).toHaveBeenCalledWith(["voice", "text"]);
    });

    it("ignores a drag carrying something that is not a section", async () => {
      // A drag can come from outside the application, and the sidebar is what
      // it would otherwise rearrange.
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());
      const dataTransfer = carrying({ "text/plain": "a file" });

      fireEvent.dragOver(region("Text"), { dataTransfer });
      fireEvent.drop(region("Text"), { dataTransfer });

      expect(order()).toEqual(["Text", "Voice"]);
      expect(setSectionOrder).not.toHaveBeenCalled();
    });

    it("moves a section up with the arrow keys", async () => {
      // Drag is not a route everybody has. See the handle's own label.
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      grip("Voice").focus();
      await userEvent.keyboard("{ArrowUp}");

      expect(order()).toEqual(["Voice", "Text"]);
      expect(setSectionOrder).toHaveBeenCalledWith(["voice", "text"]);
    });

    it("moves a section down with the arrow keys", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      grip("Text").focus();
      await userEvent.keyboard("{ArrowDown}");

      expect(order()).toEqual(["Voice", "Text"]);
    });

    it("keeps the handle focused across a move, so a second press follows", async () => {
      // Otherwise the list moves out from under the keyboard and the next
      // press goes to the page.
      list([
        text("!a:example.org", "general"),
        voice(LOUNGE, "Lounge"),
      ]);
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      grip("Voice").focus();
      await userEvent.keyboard("{ArrowUp}");

      expect(grip("Voice")).toHaveFocus();
    });

    it("does nothing at the top of the list", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      grip("Text").focus();
      await userEvent.keyboard("{ArrowUp}");

      expect(order()).toEqual(["Text", "Voice"]);
      expect(setSectionOrder).not.toHaveBeenCalled();
    });

    it("does nothing at the bottom of the list", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      grip("Voice").focus();
      await userEvent.keyboard("{ArrowDown}");

      expect(order()).toEqual(["Text", "Voice"]);
      expect(setSectionOrder).not.toHaveBeenCalled();
    });

    it("stays where it was dropped when the write fails", async () => {
      // The list has already moved by then, on the fold's terms: putting it
      // back because the file would not take it is a drag that undoes itself.
      setSectionOrder.mockReset().mockRejectedValue(new Error("read only"));
      const logged = vi.spyOn(console, "error").mockImplementation(() => {});
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      drag("Voice", "Text");

      expect(order()).toEqual(["Voice", "Text"]);
      await waitFor(() =>
        expect(logged).toHaveBeenCalledWith(
          expect.stringContaining("order"),
          expect.anything(),
        ),
      );
    });

    it("offers itself as a place to drop a section", async () => {
      // `preventDefault` on the drag over is the whole of what makes a drop
      // possible, and nothing else in these tests would notice it missing.
      // `fireEvent` answers false when the handler prevented the default.
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());
      const dataTransfer = carrying();
      fireEvent.dragStart(grip("Voice"), { dataTransfer });

      expect(fireEvent.dragOver(region("Text"), { dataTransfer })).toBe(false);
    });

    it("does not offer itself as a place to drop a file", async () => {
      // The other half. A file dragged into the window must not be told the
      // sidebar will take it.
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());
      const dataTransfer = carrying({ Files: "" });

      expect(fireEvent.dragOver(region("Text"), { dataTransfer })).toBe(true);
    });

    it("marks the section a dragged one would land on", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());
      const dataTransfer = carrying();

      fireEvent.dragStart(grip("Voice"), { dataTransfer });
      fireEvent.dragOver(region("Text"), { dataTransfer });

      expect(region("Text")).toHaveAttribute("data-over", "true");
      // Not the one being dragged, which is where it already is.
      fireEvent.dragOver(region("Voice"), { dataTransfer });
      expect(region("Voice")).toHaveAttribute("data-over", "false");
    });

    it("stops looking carried once a drag is abandoned", async () => {
      // A drag let go of over nothing never reaches a drop, so this is the
      // only thing that puts the section back.
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());
      const dataTransfer = carrying();
      fireEvent.dragStart(grip("Voice"), { dataTransfer });
      expect(region("Voice")).toHaveAttribute("data-dragged", "true");

      fireEvent.dragEnd(grip("Voice"), { dataTransfer });

      expect(region("Voice")).toHaveAttribute("data-dragged", "false");
      expect(order()).toEqual(["Text", "Voice"]);
    });

    it("stops marking a section once the drag leaves it", async () => {
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());
      const dataTransfer = carrying();
      fireEvent.dragStart(grip("Voice"), { dataTransfer });
      fireEvent.dragOver(region("Text"), { dataTransfer });

      fireEvent.dragLeave(region("Text"), { dataTransfer });

      expect(region("Text")).toHaveAttribute("data-over", "false");
    });

    it("does nothing when it is the only section drawn", async () => {
      // The arrow keys move a section past the ones somebody can see. An
      // empty section is not drawn, so moving past one would be a press that
      // rearranges the file and changes nothing on screen.
      list([text("!a:example.org", "general")]);
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      grip("Text").focus();
      await userEvent.keyboard("{ArrowDown}");

      expect(order()).toEqual(["Text"]);
      expect(setSectionOrder).not.toHaveBeenCalled();
    });

    it("says which section a handle moves", async () => {
      // A row of identical grips is a row of controls a screen reader reads
      // out as the same thing.
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      expect(grip("Voice")).toHaveAccessibleName("Move Voice");
      expect(grip("Text")).toHaveAccessibleName("Move Text");
    });

    it("says that the arrow keys are a way to move it", async () => {
      // The only place the keyboard route is announced. Dragging is the
      // obvious half and the one some people cannot use.
      both();
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      expect(grip("Voice")).toHaveAccessibleDescription(
        expect.stringContaining("arrow"),
      );
    });
  });

  describe("sections somebody made", () => {
    /** Drag a channel onto a section's heading, as the browser would. */
    function dragOnto(section: string, roomId: string) {
      const data = { "application/x-consort-room": roomId };
      const transfer = {
        types: Object.keys(data),
        getData: (type: string) => data[type as keyof typeof data] ?? "",
      };
      const heading = screen.getByRole("region", { name: section });
      fireEvent.dragOver(heading, { dataTransfer: transfer });
      fireEvent.drop(heading, { dataTransfer: transfer });
    }

    it("draws a stored section with the name somebody gave it", async () => {
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects", ["!a:example.org"])],
      });
      list([text("!a:example.org", "general")]);

      expect(
        await screen.findByRole("region", { name: "Projects" }),
      ).toBeInTheDocument();
      expect(namesIn("Projects")).toEqual(["#general"]);
    });

    it("takes a channel out of Text once it is in a section of its own", async () => {
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects", ["!a:example.org"])],
      });
      list([text("!a:example.org", "general"), text("!b:example.org", "random")]);

      await screen.findByRole("region", { name: "Projects" });
      expect(namesIn("Text")).toEqual(["#random"]);
    });

    it("draws a section with nothing in it, so something can be put there", async () => {
      // The one place the rule differs from Text and Voice, which are dropped
      // when empty: a section nothing drew would be one nothing could fill.
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects")],
      });
      list([text("!a:example.org", "general")]);

      expect(
        await screen.findByRole("region", { name: "Projects" }),
      ).toBeInTheDocument();
    });

    it("makes a section with the name that was typed", async () => {
      const user = userEvent.setup();
      list([text("!a:example.org", "general")]);
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      await user.click(screen.getByRole("button", { name: "New section" }));
      await user.type(
        screen.getByLabelText("Name for the new section"),
        "Projects",
      );
      await user.click(screen.getByRole("button", { name: "Add" }));

      expect(createSection).toHaveBeenCalledWith("!s:example.org", "Projects");
      expect(
        await screen.findByRole("region", { name: "Projects" }),
      ).toBeInTheDocument();
    });

    it("will not make a section with no name", async () => {
      const user = userEvent.setup();
      list([text("!a:example.org", "general")]);

      await user.click(screen.getByRole("button", { name: "New section" }));
      await user.type(screen.getByLabelText("Name for the new section"), "  ");

      expect(screen.getByRole("button", { name: "Add" })).toBeDisabled();
      expect(createSection).not.toHaveBeenCalled();
    });

    it("says so when the section could not be made", async () => {
      const user = userEvent.setup();
      createSection.mockImplementation(() =>
        Promise.reject({ message: "A section needs a name, and a short one." }),
      );
      list([text("!a:example.org", "general")]);

      await user.click(screen.getByRole("button", { name: "New section" }));
      await user.type(
        screen.getByLabelText("Name for the new section"),
        "Projects",
      );
      await user.click(screen.getByRole("button", { name: "Add" }));

      expect(await screen.findByRole("alert")).toHaveTextContent(
        "A section needs a name",
      );
    });

    it("renames a section somebody made", async () => {
      const user = userEvent.setup();
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects")],
      });
      list([text("!a:example.org", "general")]);
      await screen.findByRole("region", { name: "Projects" });

      await user.click(screen.getByRole("button", { name: "Rename Projects" }));
      await user.clear(screen.getByLabelText("Rename Projects"));
      await user.type(screen.getByLabelText("Rename Projects"), "Clients");
      await user.click(screen.getByRole("button", { name: "Save" }));

      expect(renameSection).toHaveBeenCalledWith("custom-1", "Clients");
      expect(
        await screen.findByRole("region", { name: "Clients" }),
      ).toBeInTheDocument();
    });

    it("leaves the name alone when a rename is abandoned", async () => {
      const user = userEvent.setup();
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects")],
      });
      list([text("!a:example.org", "general")]);
      await screen.findByRole("region", { name: "Projects" });

      await user.click(screen.getByRole("button", { name: "Rename Projects" }));
      await user.type(screen.getByLabelText("Rename Projects"), "nonsense");
      await user.click(screen.getByRole("button", { name: "Cancel" }));

      expect(renameSection).not.toHaveBeenCalled();
      expect(
        screen.getByRole("region", { name: "Projects" }),
      ).toBeInTheDocument();
    });

    it("gives a deleted section's channels back to Text", async () => {
      const user = userEvent.setup();
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects", ["!a:example.org"])],
      });
      list([text("!a:example.org", "general")]);
      await screen.findByRole("region", { name: "Projects" });

      await user.click(screen.getByRole("button", { name: "Delete Projects" }));

      expect(deleteSection).toHaveBeenCalledWith("custom-1");
      await waitFor(() => expect(namesIn("Text")).toEqual(["#general"]));
      expect(
        screen.queryByRole("region", { name: "Projects" }),
      ).not.toBeInTheDocument();
    });

    it("offers no rename or delete on Text and Voice", async () => {
      // They follow `m.room.type` rather than a choice, so there is no name of
      // theirs to change and nothing to delete.
      list([text("!a:example.org", "general")]);
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      expect(
        screen.queryByRole("button", { name: "Rename Text" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Delete Text" }),
      ).not.toBeInTheDocument();
    });

    it("puts a channel in a section when its box is ticked", async () => {
      const user = userEvent.setup();
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects")],
      });
      list([text("!a:example.org", "general")]);
      await screen.findByRole("region", { name: "Projects" });

      await user.click(
        screen.getByRole("button", { name: "Choose channels for Projects" }),
      );
      await user.click(screen.getByRole("checkbox", { name: "general" }));

      expect(setRoomSection).toHaveBeenCalledWith("!a:example.org", "custom-1");
      await waitFor(() => expect(namesIn("Projects")).toEqual(["#general"]));
    });

    it("takes a channel back out when its box is unticked", async () => {
      const user = userEvent.setup();
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects", ["!a:example.org"])],
      });
      list([text("!a:example.org", "general")]);
      await screen.findByRole("region", { name: "Projects" });

      await user.click(
        screen.getByRole("button", { name: "Choose channels for Projects" }),
      );
      await user.click(screen.getByRole("checkbox", { name: "general" }));

      expect(setRoomSection).toHaveBeenCalledWith("!a:example.org", null);
      await waitFor(() => expect(namesIn("Text")).toEqual(["#general"]));
    });

    it("holds a text and a voice channel side by side", async () => {
      // The point of the feature: a project is the channel and the call about
      // it, and splitting those by kind is what #170 undoes.
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [
          made("custom-1", "Projects", ["!a:example.org", LOUNGE]),
        ],
      });
      list([text("!a:example.org", "general"), voice(LOUNGE, "Lounge")]);

      await screen.findByRole("region", { name: "Projects" });
      expect(namesIn("Projects")).toEqual(["#general", "Lounge"]);
    });

    it("takes a channel dragged onto its heading", async () => {
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects")],
      });
      list([text("!a:example.org", "general")]);
      await screen.findByRole("region", { name: "Projects" });

      dragOnto("Projects", "!a:example.org");

      expect(setRoomSection).toHaveBeenCalledWith("!a:example.org", "custom-1");
      await waitFor(() => expect(namesIn("Projects")).toEqual(["#general"]));
    });

    it("gives a channel dragged onto Text back to Text", async () => {
      // Text and Voice hold whatever is left over, so dropping on one is how a
      // pointer takes a channel out of a section.
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects", ["!a:example.org"])],
        order: ["text", "custom-1"],
      });
      list([text("!a:example.org", "general"), text("!b:example.org", "random")]);
      await screen.findByRole("region", { name: "Projects" });

      dragOnto("Text", "!a:example.org");

      expect(setRoomSection).toHaveBeenCalledWith("!a:example.org", null);
      await waitFor(() =>
        expect(namesIn("Text")).toEqual(["#general", "#random"]),
      );
    });

    it("folds a section somebody made like any other", async () => {
      const user = userEvent.setup();
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [made("custom-1", "Projects", ["!a:example.org"])],
      });
      list([text("!a:example.org", "general")]);
      await screen.findByRole("region", { name: "Projects" });

      await user.click(
        screen.getByRole("button", { name: "Projects" }),
      );

      expect(setSectionFolded).toHaveBeenCalledWith("custom-1", true);
    });

    it("keeps a section out of a space it was not made in", async () => {
      // Otherwise "Projects" would follow somebody into every other space,
      // where none of its channels are.
      sidebarSettings.mockResolvedValue({
        ...STORED,
        sections: [
          { key: "custom-1", name: "Projects", space: "!other:example.org", rooms: [] },
        ],
      });
      list([text("!a:example.org", "general")]);
      await waitFor(() => expect(sidebarSettings).toHaveBeenCalled());

      expect(
        screen.queryByRole("region", { name: "Projects" }),
      ).not.toBeInTheDocument();
    });
  });
});
