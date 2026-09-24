import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const recentRooms = vi.hoisted(() => vi.fn());
// Each row draws the room's picture, which asks for bytes of its own.
const roomAvatar = vi.hoisted(() => vi.fn());
// The link out is a command, because the webview has no way back to the page.
const openLink = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  recentRooms,
  roomAvatar,
  openLink,
}));

import { OpeningPane } from "./OpeningPane";
import { resetAvatarCache } from "../lib/avatars";
import type { Call, Channel, Participant, Rooms } from "../lib/api";

const GENERAL = "!general:example.org";
const RANDOM = "!random:example.org";
const LOUNGE = "!lounge:example.org";
const STANDUP = "!standup:example.org";

function text(id: string, name: string): Channel {
  return {
    id,
    name,
    kind: "text",
    avatar: null,
    joined: true,
    participants: [],
    unread: 0,
    mentions: 0,
  };
}

function voice(
  id: string,
  name: string,
  participants: Participant[] = [],
): Channel {
  return { ...text(id, name), kind: "voice", participants };
}

function person(name: string): Participant {
  return { id: `@${name.toLowerCase()}:example.org`, name };
}

const HOME = {
  id: "home",
  name: "Home",
  avatar: null,
  channels: [text(GENERAL, "general"), text(RANDOM, "random")],
};

const KAHU = {
  id: "!kahu:example.org",
  name: "Kahu HQ",
  avatar: null,
  channels: [voice(LOUNGE, "Lounge")],
};

const ROOMS: Rooms = { spaces: [HOME, KAHU] };

const NOWHERE: Rooms = {
  spaces: [{ id: "home", name: "Home", avatar: null, channels: [] }],
};

const IDLE: Call = { state: "disconnected" };

function pane({
  rooms = ROOMS,
  call = IDLE,
  onOpen = vi.fn(),
  onUnfold,
}: {
  rooms?: Rooms;
  call?: Call;
  onOpen?: (spaceId: string, channel: Channel) => void;
  onUnfold?: () => void;
} = {}) {
  render(
    <OpeningPane
      rooms={rooms}
      call={call}
      onOpen={onOpen}
      {...(onUnfold === undefined ? {} : { onUnfold })}
    />,
  );
  return { onOpen };
}

/** The Lounge, with these people standing in it. */
function busyWith(people: Participant[]): Rooms {
  return {
    spaces: [HOME, { ...KAHU, channels: [voice(LOUNGE, "Lounge", people)] }],
  };
}

/** The rows under a section, by the words on them. */
function rowsUnder(label: string): string[] {
  return within(screen.getByRole("list", { name: label }))
    .getAllByRole("button")
    .map((row) => row.textContent ?? "");
}

describe("OpeningPane", () => {
  beforeEach(() => {
    resetAvatarCache();
    roomAvatar.mockReset().mockResolvedValue(null);
    openLink.mockReset().mockResolvedValue(undefined);
    recentRooms.mockReset().mockResolvedValue([]);
  });

  describe("the rooms somebody was last in", () => {
    it("lists them in the order they were opened", async () => {
      recentRooms.mockResolvedValue([LOUNGE, GENERAL]);

      pane();

      await waitFor(() => {
        expect(rowsUnder("Recent")).toEqual([
          expect.stringContaining("Lounge"),
          expect.stringContaining("#general"),
        ]);
      });
    });

    it("names a channel the way the rest of the application does", async () => {
      // The hash on a text channel and nothing on a voice one. A row saying
      // `general` above a heading saying `#general` reads as two rooms.
      recentRooms.mockResolvedValue([GENERAL, LOUNGE]);

      pane();

      expect(await screen.findByRole("button", { name: /#general/ })).toBeVisible();
      expect(screen.getByRole("button", { name: /Lounge/ })).toBeVisible();
    });

    it("says which rail entry each room is under", async () => {
      // The one thing the sidebar beside this cannot say. It draws the
      // selected space only, so a room in any other one is a room somebody
      // would have to go looking through the rail for.
      recentRooms.mockResolvedValue([LOUNGE]);

      pane();

      const row = await screen.findByRole("button", { name: /Lounge/ });
      expect(row).toHaveTextContent("Kahu HQ");
    });

    it("leaves out a room that is no longer in the list", async () => {
      // A room this account has left. Its ID stays in the file until it ages
      // out, and a row for it would be a control that does nothing.
      recentRooms.mockResolvedValue(["!gone:example.org", GENERAL]);

      pane();

      await waitFor(() => {
        expect(rowsUnder("Recent")).toEqual([expect.stringContaining("#general")]);
      });
    });

    it("leaves out a room that is already listed as live", async () => {
      // Two identical rows a few centimetres apart read as a mistake, and the
      // live row is the better of the two: it says who is in there.
      recentRooms.mockResolvedValue([LOUNGE, GENERAL]);

      pane({ rooms: busyWith([person("Ada")]) });

      await waitFor(() => {
        expect(rowsUnder("Recent")).toEqual([
          expect.stringContaining("#general"),
        ]);
      });
      expect(rowsUnder("Live now")).toEqual([expect.stringContaining("Lounge")]);
    });

    it("fills the room a live one gave up from further down the list", async () => {
      // Dropping the row rather than the entry. Six is what this list is for,
      // and a screen that showed five because one room happened to have
      // somebody in it would be shorter for no reason a reader could see.
      const many = Array.from({ length: 8 }, (_, n) => `!room${n}:example.org`);
      recentRooms.mockResolvedValue([LOUNGE, ...many]);

      pane({
        rooms: {
          spaces: [
            { ...HOME, channels: many.map((id, n) => text(id, `room${n}`)) },
            { ...KAHU, channels: [voice(LOUNGE, "Lounge", [person("Ada")])] },
          ],
        },
      });

      await waitFor(() => expect(rowsUnder("Recent")).toHaveLength(6));
    });

    it("stops at six, however many were remembered", async () => {
      // More than fills the screen is a history rather than a shortcut, and
      // the point of this list is that it can be read at a glance.
      const many = Array.from({ length: 9 }, (_, n) => `!room${n}:example.org`);
      recentRooms.mockResolvedValue(many);

      pane({
        rooms: {
          spaces: [
            {
              ...HOME,
              channels: many.map((id, n) => text(id, `room${n}`)),
            },
          ],
        },
      });

      await waitFor(() => expect(rowsUnder("Recent")).toHaveLength(6));
    });

    it("opens the room a row was pressed for", async () => {
      recentRooms.mockResolvedValue([LOUNGE]);
      const { onOpen } = pane();

      await userEvent.click(await screen.findByRole("button", { name: /Lounge/ }));

      expect(onOpen).toHaveBeenCalledWith(KAHU.id, KAHU.channels[0]);
    });

    it("says what will fill the list when nothing has been opened yet", async () => {
      // The state every account is in on the launch after this ships, and the
      // one a new account meets. An empty box under a heading reads as
      // something that failed to load.
      recentRooms.mockResolvedValue([]);

      pane();

      expect(
        await screen.findByText(/channels you open show up here/i),
      ).toBeVisible();
    });

    it("says nothing about the list until the answer is in", async () => {
      // Not knowing and knowing there is nothing are different states. The
      // line about what will fill the list would otherwise flash up on the
      // screen of somebody who has six rooms waiting to be drawn.
      let answer: (ids: string[]) => void = () => {};
      recentRooms.mockReturnValue(
        new Promise<string[]>((resolve) => {
          answer = resolve;
        }),
      );

      pane();

      expect(screen.getByRole("heading", { name: "Recent" })).toBeVisible();
      expect(
        screen.queryByText(/channels you open show up here/i),
      ).not.toBeInTheDocument();

      await act(async () => {
        answer([GENERAL]);
      });

      expect(screen.getByRole("button", { name: /#general/ })).toBeVisible();
    });

    it("draws no list at all for an account in no rooms", async () => {
      // Nothing to be recently in. A heading saying Recent over a line saying
      // nothing is here yet is the screen this replaced, twice.
      pane({ rooms: NOWHERE });

      await waitFor(() => expect(recentRooms).toHaveBeenCalled());
      expect(screen.queryByRole("list", { name: "Recent" })).not.toBeInTheDocument();
      expect(screen.queryByText(/channels you open show up here/i)).not.toBeInTheDocument();
    });
  });

  describe("what a person has to be quick about", () => {
    it("lists a voice channel somebody is sitting in", async () => {
      pane({
        rooms: {
          spaces: [
            HOME,
            { ...KAHU, channels: [voice(LOUNGE, "Lounge", [person("Ada")])] },
          ],
        },
      });

      const row = await screen.findByRole("button", { name: /Lounge/ });
      expect(row).toHaveTextContent("Ada");
    });

    it("names both people when two are in one", async () => {
      pane({ rooms: busyWith([person("Ada"), person("Grace")]) });

      const row = await screen.findByRole("button", { name: /Lounge/ });
      expect(row).toHaveTextContent("Ada and Grace");
    });

    it("names two and counts the rest", async () => {
      // A row is a nudge rather than a roster. Six names is wider than this
      // pane on a laptop, and which six was never the useful part.
      pane({
        rooms: busyWith([
          person("Ada"),
          person("Grace"),
          person("Alan"),
          person("Edsger"),
        ]),
      });

      const row = await screen.findByRole("button", { name: /Lounge/ });
      expect(row).toHaveTextContent("Ada, Grace and 2 others");
    });

    it("counts a single other in the singular", async () => {
      pane({
        rooms: busyWith([person("Ada"), person("Grace"), person("Alan")]),
      });

      const row = await screen.findByRole("button", { name: /Lounge/ });
      expect(row).toHaveTextContent("Ada, Grace and 1 other");
    });

    it("finds one in a rail entry that is not selected", async () => {
      // The whole reason this is here rather than left to the sidebar, which
      // only ever draws one space.
      pane({
        rooms: {
          spaces: [
            HOME,
            {
              ...KAHU,
              channels: [voice(STANDUP, "Standup", [person("Grace")])],
            },
          ],
        },
      });

      expect(
        await screen.findByRole("button", { name: /Standup/ }),
      ).toBeVisible();
    });

    it("draws nothing about voice while nobody is in one", async () => {
      pane();

      await waitFor(() => expect(recentRooms).toHaveBeenCalled());
      expect(screen.queryByRole("list", { name: "Live now" })).not.toBeInTheDocument();
    });

    it("leaves out the channel this session is already in", async () => {
      // The call panel and the call card both say where this session is. A
      // third invitation to walk into the room somebody is standing in is one
      // control that cannot do anything.
      pane({
        rooms: {
          spaces: [
            HOME,
            { ...KAHU, channels: [voice(LOUNGE, "Lounge", [person("Ada")])] },
          ],
        },
        call: {
          state: "connected",
          roomId: LOUNGE,
          participants: [],
          trouble: null,
        },
      });

      await waitFor(() => expect(recentRooms).toHaveBeenCalled());
      expect(screen.queryByRole("list", { name: "Live now" })).not.toBeInTheDocument();
    });

    it("joins the channel a row was pressed for", async () => {
      const busy = voice(LOUNGE, "Lounge", [person("Ada")]);
      const { onOpen } = pane({
        rooms: { spaces: [HOME, { ...KAHU, channels: [busy] }] },
      });

      await userEvent.click(await screen.findByRole("button", { name: /Lounge/ }));

      expect(onOpen).toHaveBeenCalledWith(KAHU.id, busy);
    });
  });

  describe("the rest of the screen", () => {
    it("says where rooms come from to an account in none", async () => {
      // Honest rather than encouraging. Consort reads the rooms an account
      // has already joined and has no way to join one, so a screen inviting
      // somebody to find a room here would be inviting them to fail.
      pane({ rooms: NOWHERE });

      expect(
        await screen.findByText(/join one from another client/i),
      ).toBeVisible();
    });

    it("says what to do with the rail to an account that has rooms", async () => {
      pane();

      expect(await screen.findByText(/pick a channel to read it/i)).toBeVisible();
    });

    it("opens the repository outside the application", async () => {
      // Never in place. The webview holds one page and has no way back to it.
      pane();

      await userEvent.click(screen.getByRole("link", { name: /github/i }));

      expect(openLink).toHaveBeenCalledWith(
        "https://github.com/consort-chat/consort",
      );
    });

    it("offers the folded sidebar back", async () => {
      // This pane has no header to put the control in, so folding the list
      // before picking a channel would otherwise be a one-way door.
      const onUnfold = vi.fn();
      pane({ onUnfold });

      await userEvent.click(screen.getByRole("button", { name: /channel list/i }));

      expect(onUnfold).toHaveBeenCalled();
    });

    it("has no such control while the sidebar is where it was", () => {
      pane();

      expect(
        screen.queryByRole("button", { name: /channel list/i }),
      ).not.toBeInTheDocument();
    });

    it("settles the list when the recent rooms cannot be read", async () => {
      // A preferences file that could not be read is not a reason for the
      // screen the application opens on to sit under a heading with nothing
      // beneath it for the rest of the session.
      recentRooms.mockRejectedValue(new Error("nope"));

      pane();

      expect(
        await screen.findByText(/channels you open show up here/i),
      ).toBeVisible();
      expect(screen.getByRole("heading", { level: 1 })).toBeVisible();
      expect(screen.getByRole("link", { name: /github/i })).toBeVisible();
    });
  });
});
