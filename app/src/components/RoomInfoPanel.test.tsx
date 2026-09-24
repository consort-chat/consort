import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const roomCopyLink = vi.hoisted(() => vi.fn());
// The panel draws the room's picture, which asks for bytes of its own.
const roomAvatar = vi.hoisted(() => vi.fn());
// And one per person in it, once the People section has somebody to draw.
const memberAvatar = vi.hoisted(() => vi.fn());
const roomMembers = vi.hoisted(() => vi.fn());
// What a person's card asks for the moment a row opens one.
const memberProfile = vi.hoisted(() => vi.fn());
const audioSettings = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  roomCopyLink,
  roomAvatar,
  memberAvatar,
  roomMembers,
  memberProfile,
  audioSettings,
}));

import { RoomInfoPanel } from "./RoomInfoPanel";
import { resetAvatarCache } from "../lib/avatars";
import type { Channel, Member, Members, Roster } from "../lib/api";

const GENERAL = "!general:example.org";
const SELF = "@bob:example.org";

function channel(over: Partial<Channel> = {}): Channel {
  return {
    id: GENERAL,
    name: "general",
    kind: "text",
    avatar: null,
    joined: true,
    participants: [],
    unread: 0,
    mentions: 0,
    ...over,
  };
}

/** Everything the panel needs, with the parts a test does not care about. */
function props() {
  return {
    channel: channel(),
    selfId: SELF,
    onClose: vi.fn(),
    onOpenRoom: vi.fn(),
  };
}

function named(id: string, name: string): Member {
  return { person: { id, name } };
}

/** A group small enough that the cap never came into it. */
function roster(shown: Member[]): Roster {
  return { count: shown.length, shown };
}

function people(over: Partial<Members> = {}): Members {
  return { joined: roster([]), invited: roster([]), ...over };
}

describe("RoomInfoPanel", () => {
  beforeEach(() => {
    resetAvatarCache();
    roomAvatar.mockReset().mockResolvedValue(null);
    memberAvatar.mockReset().mockResolvedValue(null);
    roomCopyLink.mockReset().mockResolvedValue(undefined);
    roomMembers.mockReset().mockResolvedValue(people({}));
    memberProfile
      .mockReset()
      .mockResolvedValue({ presence: "unknown", status: null, lastActiveAgo: null });
    audioSettings.mockReset().mockResolvedValue({ personVolumes: {} });
  });

  it("names the room the way the heading above it does", () => {
    // The hash included. A panel saying `general` under a heading saying
    // `#general` reads as two rooms.
    render(<RoomInfoPanel {...props()} />);

    expect(screen.getByText("#general")).toBeVisible();
  });

  it("leaves the hash off a voice channel, as the heading does", () => {
    render(
      <RoomInfoPanel
        {...props()}
        channel={channel({ name: "Lounge", kind: "voice" })}
      />,
    );

    expect(screen.getByText("Lounge")).toBeVisible();
  });

  it("draws the address the room published", () => {
    render(
      <RoomInfoPanel
        {...props()}
        channel={channel({ alias: "#general:example.org" })}
      />,
    );

    expect(screen.getByText("#general:example.org")).toBeVisible();
  });

  it("draws no address for a room that publishes none", () => {
    // Most private rooms. An absence is not a failure and gets no sentence.
    const { container } = render(<RoomInfoPanel {...props()} />);

    expect(container.querySelector(".info__alias")).toBeNull();
  });

  it("draws the whole topic rather than the header's one line", () => {
    // The reason the panel exists. The header truncates, and a topic with two
    // paragraphs in it is a topic nobody could read until now.
    const topic = "Where the good links go.\nAnd the bad ones too.";
    const { container } = render(
      <RoomInfoPanel {...props()} channel={channel({ topic })} />,
    );

    // Compared against the whole string rather than matched loosely, because
    // the line break somebody put in a topic is part of what they wrote.
    expect(container.querySelector(".info__topic")?.textContent).toBe(topic);
  });

  it("says so when the room has not set a topic", () => {
    // Rather than an empty section, which reads as something that failed to
    // load.
    render(<RoomInfoPanel {...props()} />);

    expect(screen.getByText("This room has not set a topic.")).toBeVisible();
  });

  it("puts the room's address on the clipboard", async () => {
    render(<RoomInfoPanel {...props()} />);

    await userEvent.click(screen.getByRole("button", { name: "Copy room link" }));

    expect(roomCopyLink).toHaveBeenCalledWith(GENERAL);
  });

  it("says the copy worked, and stops saying it", async () => {
    // A copy is silent otherwise, and a control that says nothing invites a
    // second press.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    render(<RoomInfoPanel {...props()} />);

    await userEvent.click(screen.getByRole("button", { name: "Copy room link" }));
    expect(await screen.findByRole("button", { name: "Link copied" })).toBeVisible();

    await act(async () => {
      vi.advanceTimersByTime(5_000);
    });
    expect(screen.getByRole("button", { name: "Copy room link" })).toBeVisible();
    vi.useRealTimers();
  });

  it("reports a clipboard this desktop would not give up", async () => {
    roomCopyLink.mockRejectedValue({
      message: "Consort could not reach this desktop's clipboard.",
      detail: "writing to the clipboard: no",
    });
    render(<RoomInfoPanel {...props()} />);

    await userEvent.click(screen.getByRole("button", { name: "Copy room link" }));

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "could not reach this desktop's clipboard",
      ),
    );
  });

  it("closes when asked", async () => {
    const onClose = vi.fn();
    render(<RoomInfoPanel {...props()} onClose={onClose} />);

    await userEvent.click(screen.getByRole("button", { name: "Close room info" }));

    expect(onClose).toHaveBeenCalled();
  });

  it("closes on Escape, the way everything else here is dismissed", async () => {
    const onClose = vi.fn();
    render(<RoomInfoPanel {...props()} onClose={onClose} />);

    await userEvent.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalled();
  });

  it("stops answering the key once it is gone", async () => {
    // A listener at the window outlives the panel if nothing takes it off,
    // and the shell would then close a panel that is already closed on every
    // later press.
    const onClose = vi.fn();
    const { unmount } = render(<RoomInfoPanel {...props()} onClose={onClose} />);

    unmount();
    await userEvent.keyboard("{Escape}");

    expect(onClose).not.toHaveBeenCalled();
  });

  describe("the people in it", () => {
    it("says how many people have joined", async () => {
      // Under the topic, which is where every other client puts it and where
      // somebody asking what a room is looks next.
      roomMembers.mockResolvedValue(
        people({ joined: roster([named("@ada:example.org", "Ada")]) }),
      );
      render(<RoomInfoPanel {...props()} />);

      expect(
        await screen.findByRole("heading", { name: "People 1" }),
      ).toBeVisible();
    });

    it("lists them in the order it was given them", async () => {
      // The order is decided once, in Rust, and it is total. Sorting again
      // here would be a second opinion free to disagree with the first.
      roomMembers.mockResolvedValue(
        people({
          joined: roster([
            named("@ada:example.org", "Ada"),
            named("@mel:example.org", "Mel"),
            named("@zoe:example.org", "Zoe"),
          ]),
        }),
      );
      const { container } = render(<RoomInfoPanel {...props()} />);

      await screen.findByRole("button", { name: "Ada" });
      expect(
        [...container.querySelectorAll(".info__who")].map(
          (node) => node.textContent,
        ),
      ).toEqual(["Ada", "Mel", "Zoe"]);
    });

    it("opens a person's card when their row is pressed", async () => {
      // The card a name in a voice channel already opens. Presence, a status
      // line, per-person volume and the Message button, none of which is
      // worth building a second time.
      roomMembers.mockResolvedValue(
        people({ joined: roster([named("@ada:example.org", "Ada")]) }),
      );
      render(<RoomInfoPanel {...props()} />);

      await userEvent.click(await screen.findByRole("button", { name: "Ada" }));

      expect(await screen.findByRole("dialog", { name: "Ada" })).toBeVisible();
    });

    it("opens a card about somebody who has only been invited", async () => {
      // The same card, because the question a name raises is the same one
      // whether or not they have answered the invitation.
      roomMembers.mockResolvedValue(
        people({ invited: roster([named("@mel:example.org", "Mel")]) }),
      );
      render(<RoomInfoPanel {...props()} />);

      await userEvent.click(await screen.findByRole("button", { name: "Mel" }));

      expect(await screen.findByRole("dialog", { name: "Mel" })).toBeVisible();
    });

    it("lets Escape close the card without closing the panel under it", async () => {
      // One press, one thing. The card stops the key at the document and the
      // panel listens at the window, which is what leaves the card the press
      // while it is open over the panel that opened it.
      const onClose = vi.fn();
      roomMembers.mockResolvedValue(
        people({ joined: roster([named("@ada:example.org", "Ada")]) }),
      );
      render(<RoomInfoPanel {...props()} onClose={onClose} />);
      await userEvent.click(await screen.findByRole("button", { name: "Ada" }));
      await screen.findByRole("dialog", { name: "Ada" });

      await userEvent.keyboard("{Escape}");

      expect(screen.queryByRole("dialog", { name: "Ada" })).toBeNull();
      expect(onClose).not.toHaveBeenCalled();
    });

    it("says how many more there are than it could list", async () => {
      // Two thousand beside a hundred rows is fine. A hundred rows with
      // nothing saying there are more is a room that looks smaller than it is.
      roomMembers.mockResolvedValue(
        people({
          joined: { count: 2_000, shown: [named("@ada:example.org", "Ada")] },
        }),
      );
      render(<RoomInfoPanel {...props()} />);

      expect(await screen.findByText("and 1999 more")).toBeVisible();
    });

    it("counts the room rather than the list", async () => {
      // The bug the cap could have introduced. Two thousand beside a hundred
      // rows is honest; a hundred beside a hundred rows, in a room of two
      // thousand, is the panel quietly agreeing with itself.
      roomMembers.mockResolvedValue(
        people({
          joined: { count: 2_000, shown: [named("@ada:example.org", "Ada")] },
        }),
      );
      render(<RoomInfoPanel {...props()} />);

      expect(
        await screen.findByRole("heading", { name: "People 2000" }),
      ).toBeVisible();
    });

    it("says nothing about more when it listed everybody", async () => {
      // Which is every room a team actually talks in.
      roomMembers.mockResolvedValue(
        people({ joined: roster([named("@ada:example.org", "Ada")]) }),
      );
      render(<RoomInfoPanel {...props()} />);

      await screen.findByRole("button", { name: "Ada" });
      expect(screen.queryByText(/more$/)).toBeNull();
    });

    it("keeps the invited apart and says what that means", async () => {
      // The reason they are a group of their own rather than a mark on a row.
      // Somebody invited cannot read what is being said here yet, which is
      // exactly the fact that matters when the next thing anybody does is
      // paste something into the room.
      roomMembers.mockResolvedValue(
        people({
          joined: roster([named("@ada:example.org", "Ada")]),
          invited: roster([named("@mel:example.org", "Mel")]),
        }),
      );
      render(<RoomInfoPanel {...props()} />);

      expect(
        await screen.findByRole("heading", { name: "Invited 1" }),
      ).toBeVisible();
      expect(
        screen.getByText(/cannot read what is said here yet/),
      ).toBeVisible();
      expect(screen.getByRole("button", { name: "Mel" })).toBeVisible();
    });

    it("draws no invited group for a room where nobody is waiting", async () => {
      // An empty heading reads as something that failed to load.
      roomMembers.mockResolvedValue(
        people({ joined: roster([named("@ada:example.org", "Ada")]) }),
      );
      render(<RoomInfoPanel {...props()} />);

      await screen.findByRole("button", { name: "Ada" });
      expect(screen.queryByRole("heading", { name: /Invited/ })).toBeNull();
    });

    it("says outright when two people in the room go by one name", async () => {
      // The impersonation surface. The user ID is already inside the name by
      // the time it arrives here, and a reader who does not know why it is
      // there has no reason to look at it.
      roomMembers.mockResolvedValue(
        people({
          joined: roster([
            {
              person: { id: "@ada:example.org", name: "Ada (@ada:example.org)" },
              naming: "shared",
            },
          ]),
        }),
      );
      render(<RoomInfoPanel {...props()} />);

      expect(
        await screen.findByText("Someone else here uses this name"),
      ).toBeVisible();
    });

    it("does not say it about somebody who simply has no name", async () => {
      // A different fact with the same symptom. Their row is their user ID
      // because there is nothing else to draw on it, not because anybody is
      // being impersonated.
      roomMembers.mockResolvedValue(
        people({
          joined: roster([
            {
              person: { id: "@ada:example.org", name: "@ada:example.org" },
              naming: "absent",
            },
          ]),
        }),
      );
      render(<RoomInfoPanel {...props()} />);

      await screen.findByRole("button", { name: "@ada:example.org" });
      expect(screen.queryByText("Someone else here uses this name")).toBeNull();
    });

    it("reports a room whose members could not be read", async () => {
      // Rather than an empty list, which would be drawn as a room with nobody
      // in it: a different and much more alarming thing to be told.
      roomMembers.mockRejectedValue({
        message: "That room is not one this account is in.",
        detail: "no such room",
      });
      render(<RoomInfoPanel {...props()} />);

      await waitFor(() =>
        expect(screen.getByRole("alert")).toHaveTextContent(
          "not one this account is in",
        ),
      );
    });

    it("asks about the room it is pointed at", async () => {
      const { rerender } = render(<RoomInfoPanel {...props()} />);
      await waitFor(() => expect(roomMembers).toHaveBeenCalledWith(GENERAL));

      rerender(
        <RoomInfoPanel
          {...props()}
          channel={channel({ id: "!other:example.org", name: "other" })}
        />,
      );

      await waitFor(() =>
        expect(roomMembers).toHaveBeenCalledWith("!other:example.org"),
      );
    });

    it("ignores an answer about a room it has already moved off", async () => {
      // The panel follows the selected room, and a slow homeserver answering
      // about the one before it would draw the wrong people under the right
      // name.
      let answerTheFirst: (members: Members) => void = () => {};
      roomMembers
        .mockImplementationOnce(
          () =>
            new Promise<Members>((resolve) => {
              answerTheFirst = resolve;
            }),
        )
        .mockResolvedValue(
          people({ joined: roster([named("@zoe:example.org", "Zoe")]) }),
        );

      const { rerender } = render(<RoomInfoPanel {...props()} />);
      rerender(
        <RoomInfoPanel
          {...props()}
          channel={channel({ id: "!other:example.org", name: "other" })}
        />,
      );
      await screen.findByRole("button", { name: "Zoe" });

      await act(async () => {
        answerTheFirst(
          people({ joined: roster([named("@ada:example.org", "Ada")]) }),
        );
      });

      expect(screen.queryByRole("button", { name: "Ada" })).toBeNull();
      expect(screen.getByRole("button", { name: "Zoe" })).toBeVisible();
    });

    it("ignores a refusal about a room it has already moved off", async () => {
      // The same staleness on the other path. A complaint about the room
      // before this one, drawn under this one's name, would send somebody
      // looking for a fault in a room that is perfectly fine.
      let refuseTheFirst: (why: unknown) => void = () => {};
      roomMembers
        .mockImplementationOnce(
          () =>
            new Promise<Members>((_resolve, reject) => {
              refuseTheFirst = reject;
            }),
        )
        .mockResolvedValue(
          people({ joined: roster([named("@zoe:example.org", "Zoe")]) }),
        );

      const { rerender } = render(<RoomInfoPanel {...props()} />);
      rerender(
        <RoomInfoPanel
          {...props()}
          channel={channel({ id: "!other:example.org", name: "other" })}
        />,
      );
      await screen.findByRole("button", { name: "Zoe" });

      await act(async () => {
        refuseTheFirst({ message: "That room is gone.", detail: "no" });
      });

      expect(screen.queryByRole("alert")).toBeNull();
      expect(screen.getByRole("button", { name: "Zoe" })).toBeVisible();
    });

    it("says it is looking while the answer is on its way", () => {
      // The one request this panel makes. Everything else on it was already
      // in the snapshot the shell holds.
      roomMembers.mockReturnValue(new Promise<Members>(() => {}));
      render(<RoomInfoPanel {...props()} />);

      expect(screen.getByText("Reading who is here…")).toBeVisible();
    });
  });
});
