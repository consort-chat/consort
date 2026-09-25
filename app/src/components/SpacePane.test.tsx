import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";

// A row draws the room's picture, which is a command.
const roomAvatar = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  roomAvatar,
}));

import { SpacePane } from "./SpacePane";
import type { Channel, Space } from "../lib/api";
import { resetAvatarCache } from "../lib/avatars";

function text(
  name: string | null,
  { joined = true, topic }: { joined?: boolean; topic?: string } = {},
): Channel {
  return {
    id: `!${name ?? "none"}:example.org`,
    name,
    kind: "text",
    avatar: null,
    joined,
    ...(topic === undefined ? {} : { topic }),
    participants: [],
    unread: 0,
    mentions: 0,
  };
}

function voice(name: string, joined = true): Channel {
  return {
    id: `!${name}:example.org`,
    name,
    kind: "voice",
    avatar: null,
    joined,
    participants: [],
    unread: 0,
    mentions: 0,
  };
}

function space(channels: Channel[], name = "Kahu HQ"): Space {
  return { id: "!s:example.org", name, avatar: null, channels };
}

function pane({
  channels = [text("general"), text("announcements"), text("design-crit")],
  joining = null,
  onOpen = vi.fn(),
  onJoin = vi.fn(),
}: {
  channels?: Channel[];
  joining?: { roomId: string; problem: string | null } | null;
  onOpen?: Mock<(roomId: string) => void>;
  onJoin?: Mock<(roomId: string) => void>;
} = {}) {
  render(
    <SpacePane
      space={space(channels)}
      joining={joining}
      onOpen={onOpen}
      onJoin={onJoin}
    />,
  );
  return { onOpen, onJoin };
}

/** The channel names drawn, in the order they are drawn. */
function listed(): string[] {
  return within(screen.getByRole("list", { name: "Channels" }))
    .getAllByRole("button")
    .map((row) => row.getAttribute("aria-label") ?? "");
}

describe("SpacePane", () => {
  beforeEach(() => {
    resetAvatarCache();
    roomAvatar.mockReset().mockResolvedValue(null);
  });

  it("names the space it is about", () => {
    pane();

    expect(
      screen.getByRole("heading", { level: 1, name: "Kahu HQ" }),
    ).toBeInTheDocument();
  });

  it("lists every channel in the space", () => {
    pane();

    expect(listed()).toEqual(["#general", "#announcements", "#design-crit"]);
  });

  it("draws a voice channel without the hash a text one carries", () => {
    // The same distinction the list beside it and the room's own heading make.
    pane({ channels: [voice("Lounge")] });

    expect(listed()).toEqual(["Lounge"]);
  });

  it("narrows the list to the channels holding what was typed", async () => {
    pane();

    await userEvent.type(screen.getByRole("searchbox"), "an");

    expect(listed()).toEqual(["#announcements"]);
  });

  it("puts the whole list back when the search is cleared", async () => {
    pane();
    const box = screen.getByRole("searchbox");
    await userEvent.type(box, "an");

    await userEvent.clear(box);

    expect(listed()).toEqual(["#general", "#announcements", "#design-crit"]);
  });

  it("says when nothing matches, and keeps the box to correct it in", async () => {
    pane();

    await userEvent.type(screen.getByRole("searchbox"), "zzz");

    expect(screen.getByText(/nothing here matches/i)).toBeInTheDocument();
    expect(screen.getByRole("searchbox")).toBeInTheDocument();
  });

  it("opens a channel that was picked", async () => {
    const { onOpen } = pane();

    await userEvent.click(screen.getByRole("button", { name: "#general" }));

    expect(onOpen).toHaveBeenCalledWith("!general:example.org");
  });

  it("joins a channel this account is not in rather than opening it", async () => {
    const { onOpen, onJoin } = pane({
      channels: [text("announcements", { joined: false })],
    });

    await userEvent.click(
      screen.getByRole("button", { name: /join announcements/i }),
    );

    expect(onJoin).toHaveBeenCalledWith("!announcements:example.org");
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("says which channels this account is not in", () => {
    // The one thing this pane says that the space itself does not: which of
    // the rooms on offer are rooms somebody is already in.
    pane({
      channels: [text("general"), text("announcements", { joined: false })],
    });

    expect(listed()).toEqual(["#general", "Join announcements"]);
  });

  it("says a join is in flight, and will not send a second one", async () => {
    const { onJoin } = pane({
      channels: [text("announcements", { joined: false })],
      joining: { roomId: "!announcements:example.org", problem: null },
    });

    const row = screen.getByRole("button", { name: /joining announcements/i });
    expect(row).toBeDisabled();

    await userEvent.click(row);
    expect(onJoin).not.toHaveBeenCalled();
  });

  it("says why a join did not work, beside the channel it was for", () => {
    pane({
      channels: [
        text("announcements", { joined: false }),
        text("notices", { joined: false }),
      ],
      joining: {
        roomId: "!announcements:example.org",
        problem: "That channel is invite only.",
      },
    });

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("That channel is invite only.");
    const row = alert.closest("li");
    expect(row).toHaveTextContent("announcements");
    expect(row).not.toHaveTextContent("notices");
  });

  it("draws a channel's topic under its name", () => {
    // What the column beside this one has no room for, and the reason a wide
    // pane full of channels is worth more than the same names again.
    pane({ channels: [text("general", { topic: "Anything and everything" })] });

    expect(screen.getByText("Anything and everything")).toBeInTheDocument();
  });

  it("says a space with nothing in it has nothing in it", () => {
    pane({ channels: [] });

    expect(screen.getByText(/nothing in here yet/i)).toBeInTheDocument();
  });

  it("offers no search box to a space with nothing to search", () => {
    // A field over an empty list is a control that cannot do anything.
    pane({ channels: [] });

    expect(screen.queryByRole("searchbox")).toBeNull();
  });

  it("says how many channels are showing once the list has been narrowed", async () => {
    // Without it a search that hid nine of twelve rows reads as a space that
    // has three channels.
    pane();

    await userEvent.type(screen.getByRole("searchbox"), "an");

    expect(screen.getByText("1 of 3")).toBeInTheDocument();
  });

  it("counts the channels without the of when nothing is hidden", () => {
    pane();

    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("says how many channels the space has", () => {
    pane();

    expect(screen.getByText(/3 channels/)).toBeInTheDocument();
  });

  it("says how many of them this account is not in", () => {
    // The fact #128 is about, said once at the top rather than only as a pill
    // per row: a space with thirty channels and two unjoined ones should not
    // need scrolling to find that out.
    pane({
      channels: [
        text("general"),
        text("announcements", { joined: false }),
        text("notices", { joined: false }),
      ],
    });

    expect(
      screen.getByText(/2 you have not joined/),
    ).toBeInTheDocument();
  });

  it("says nothing about unjoined channels when there are none", () => {
    pane();

    expect(screen.queryByText(/you have not joined/)).toBeNull();
  });

  it("says one channel rather than 1 channels", () => {
    pane({ channels: [text("general")] });

    expect(screen.getByText(/1 channel[^s]/)).toBeInTheDocument();
  });
});
