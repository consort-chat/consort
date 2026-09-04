import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const memberAvatar = vi.hoisted(() => vi.fn());
const memberProfile = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberAvatar,
  memberProfile,
}));

import { MessageGroups, group } from "./MessageGroups";
import { resetAvatarCache } from "../lib/avatars";
import { resetPresenceCache } from "../lib/presence";
import type { Message } from "../lib/api";

beforeEach(() => {
  // Both caches are module-level, so one test's answers would otherwise be
  // the next one's.
  resetAvatarCache();
  resetPresenceCache();
  memberAvatar.mockReset().mockResolvedValue(null);
  memberProfile.mockReset().mockResolvedValue(null);
});

const GENERAL = "!general:example.org";
const ADA = "@ada:example.org";
const BOB = "@bob:example.org";
const NOON = Date.parse("2026-01-01T12:00:00Z");

function said(
  id: string,
  sender: string,
  body: string,
  at = NOON,
  extra: Partial<Message> = {},
): Message {
  return { id, sender, at, body, kind: "text", ...extra };
}

/** Draw the messages as a room does, with somewhere for a thread to open. */
function draw(
  messages: Message[],
  onOpenThread: (rootId: string) => void = vi.fn(),
) {
  return render(
    <MessageGroups
      groups={group(messages)}
      names={{ [ADA]: "Ada" }}
      roomId={GENERAL}
      selfId={BOB}
      known={known(messages)}
      onAbout={vi.fn()}
      onOpenThread={onOpenThread}
      onReact={vi.fn()}
    />,
  );
}

/** The same, with a way to see what reacting asked for. */
function drawReactable(messages: Message[], onReact: Reacted) {
  return render(
    <MessageGroups
      groups={group(messages)}
      names={{ [ADA]: "Ada" }}
      roomId={GENERAL}
      selfId={BOB}
      known={known(messages)}
      onAbout={vi.fn()}
      onOpenThread={vi.fn()}
      onReact={onReact}
    />,
  );
}

type Reacted = (eventId: string, key: string, mine: string | undefined) => void;

/** The same, with the two controls a room offers and a thread panel does not. */
function drawWithActions(
  messages: Message[],
  props: {
    onReply?: (message: Message) => void;
    onCopyLink?: (eventId: string) => void;
    copiedId?: string | null;
  },
) {
  return render(
    <MessageGroups
      groups={group(messages)}
      names={{ [ADA]: "Ada" }}
      roomId={GENERAL}
      selfId={BOB}
      known={known(messages)}
      onAbout={vi.fn()}
      {...props}
    />,
  );
}

/**
 * Draw them as the thread panel does, with nowhere further to go.
 *
 * Its own function rather than passing `undefined` to the one above. A default
 * parameter fires on `undefined` too, so that call would quietly get a handler
 * and the test would be asserting nothing.
 */
function drawInAPanel(messages: Message[]) {
  return render(
    <MessageGroups
      groups={group(messages)}
      names={{ [ADA]: "Ada" }}
      roomId={GENERAL}
      selfId={BOB}
      known={known(messages)}
      onAbout={vi.fn()}
    />,
  );
}

/** The messages a reply may point at, which is whatever is being drawn. */
function known(messages: Message[]): ReadonlyMap<string, Message> {
  return new Map(messages.map((message) => [message.id, message]));
}

describe("grouping", () => {
  it("collapses consecutive messages from one person", () => {
    const groups = group([
      said("$1", ADA, "one"),
      said("$2", ADA, "two", NOON + 1_000),
      said("$3", BOB, "three", NOON + 2_000),
    ]);

    expect(
      groups.map((one) => [one.sender, one.messages.map((said) => said.body)]),
    ).toEqual([
      [ADA, ["one", "two"]],
      [BOB, ["three"]],
    ]);
  });

  it("starts a new group after a long silence", () => {
    // The same person answering an hour later is a new thing to read, and
    // repeating their name is how a reader is told which.
    const groups = group([
      said("$1", ADA, "one"),
      said("$2", ADA, "two", NOON + 60 * 60 * 1000),
    ]);

    expect(groups).toHaveLength(2);
  });

  it("measures the gap from the last message rather than the group's first", () => {
    // Somebody talking steadily for ten minutes is one conversation, not two,
    // and comparing against the first message would split it in the middle.
    const minute = 60 * 1000;
    const groups = group([
      said("$1", ADA, "one"),
      said("$2", ADA, "two", NOON + 4 * minute),
      said("$3", ADA, "three", NOON + 8 * minute),
    ]);

    expect(groups).toHaveLength(1);
  });

  it("groups nothing out of nothing", () => {
    expect(group([])).toEqual([]);
  });
});

describe("a message with a thread", () => {
  const threaded = said("$1", ADA, "what shall we call it", NOON, {
    thread: { count: 3, participated: false },
  });

  it("says how many replies are in it", () => {
    draw([threaded]);

    expect(
      screen.getByRole("button", { name: /3 replies/i }),
    ).toBeVisible();
  });

  it("counts one reply in the singular", () => {
    // "1 replies" is the kind of thing that makes an interface look unfinished
    // to everybody who reads it.
    draw([said("$1", ADA, "hello", NOON, { thread: { count: 1, participated: false } })]);

    expect(screen.getByRole("button", { name: /^1 reply$/i })).toBeVisible();
  });

  it("opens the thread when the control is pressed", async () => {
    const onOpenThread = vi.fn();
    draw([threaded], onOpenThread);

    await userEvent.click(screen.getByRole("button", { name: /3 replies/i }));

    expect(onOpenThread).toHaveBeenCalledWith("$1");
  });

  it("opens the thread when the message itself is pressed", async () => {
    // The whole message is the target. The control below it is what makes the
    // same thing reachable from the keyboard.
    const onOpenThread = vi.fn();
    draw([threaded], onOpenThread);

    await userEvent.click(screen.getByText("what shall we call it"));

    expect(onOpenThread).toHaveBeenCalledWith("$1");
  });

  it("says a thread this session has spoken in is one of theirs", () => {
    draw([said("$1", ADA, "hello", NOON, { thread: { count: 2, participated: true } })]);

    expect(screen.getByRole("button", { name: /2 replies/i })).toHaveAttribute(
      "data-participated",
      "true",
    );
  });
});

describe("a message with no thread", () => {
  it("shows no count, because a count of zero is not information", () => {
    // "0 replies" under every line in a room says "no thread here" on every
    // one of them, which is not news about any of them.
    draw([said("$1", ADA, "hello")]);

    expect(screen.queryByRole("button", { name: /\d+ repl/i })).toBeNull();
  });

  it("offers a way to start one", async () => {
    // Otherwise a thread can only ever be joined, and every thread in the room
    // was begun somewhere else.
    const onOpenThread = vi.fn();
    draw([said("$1", ADA, "hello")], onOpenThread);

    await userEvent.click(screen.getByRole("button", { name: "Reply in thread" }));

    expect(onOpenThread).toHaveBeenCalledWith("$1");
  });

  it("offers none inside a thread panel", () => {
    // Every message in there is already in the thread being read.
    drawInAPanel([said("$1", ADA, "hello")]);

    expect(screen.queryByRole("button", { name: "Reply in thread" })).toBeNull();
  });

  it("does nothing when the words themselves are pressed", async () => {
    // Only the control starts a thread. A click anywhere in a message is how
    // somebody finishes selecting one.
    const onOpenThread = vi.fn();
    draw([said("$1", ADA, "hello")], onOpenThread);

    await userEvent.click(screen.getByText("hello"));

    expect(onOpenThread).not.toHaveBeenCalled();
  });
});

describe("selecting a threaded message", () => {
  it("does not open the thread when the press finished a selection", async () => {
    // Dragging across a message to copy it ends in a click, and opening a
    // panel on that would take the words out from under what was selected.
    const onOpenThread = vi.fn();
    draw(
      [
        said("$1", ADA, "what shall we call it", NOON, {
          thread: { count: 3, participated: false },
        }),
      ],
      onOpenThread,
    );
    vi.spyOn(window, "getSelection").mockReturnValue({
      isCollapsed: false,
    } as Selection);

    await userEvent.click(screen.getByText("what shall we call it"));

    expect(onOpenThread).not.toHaveBeenCalled();
    vi.mocked(window.getSelection).mockRestore();
  });
});

describe("a reply", () => {
  it("names who is being answered and what they said", () => {
    draw([
      said("$1", ADA, "the original"),
      said("$2", BOB, "agreed", NOON + 1_000, { replyTo: "$1" }),
    ]);

    const row = screen.getByRole("button", { name: /go to ada's message/i });
    expect(row).toHaveTextContent("Ada");
    expect(row).toHaveTextContent("the original");
  });

  it("does not say the words in reply to", () => {
    // That was the sender's own fallback passing through the formatter. It is
    // an arrow now, and the row itself is the thing to press.
    draw([
      said("$1", ADA, "the original"),
      said("$2", BOB, "agreed", NOON + 1_000, { replyTo: "$1" }),
    ]);

    expect(screen.queryByText(/in reply to/i)).not.toBeInTheDocument();
  });

  it("marks every message with its own ID, so one can be scrolled to", () => {
    const { container } = draw([said("$1", ADA, "the original")]);

    expect(
      container.querySelector('[data-message-id="$1"]'),
    ).toHaveTextContent("the original");
  });

  it("scrolls the answered message into view when the row is pressed", async () => {
    const box = document.createElement("div");
    document.body.append(box);
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;

    const messages = [
      said("$1", ADA, "the original"),
      said("$2", BOB, "agreed", NOON + 1_000, { replyTo: "$1" }),
    ];
    render(
      <MessageGroups
        groups={group(messages)}
        names={{ [ADA]: "Ada" }}
        roomId={GENERAL}
        selfId={BOB}
        known={known(messages)}
        container={{ current: box }}
        onAbout={vi.fn()}
        onOpenThread={vi.fn()}
      />,
      { container: box },
    );

    await userEvent.click(
      screen.getByRole("button", { name: /go to ada's message/i }),
    );

    expect(scrollIntoView).toHaveBeenCalled();
    expect(box.querySelector('[data-message-id="$1"]')).toHaveAttribute(
      "data-flash",
      "true",
    );
  });

  it("hands the whole message to whoever is going to answer it", async () => {
    // Not the ID. The composer quotes what is being answered and the reply
    // names who wrote it, and neither is reachable from an ID alone.
    const onReply = vi.fn();
    const message = said("$1", ADA, "the original");
    drawWithActions([message], { onReply });

    await userEvent.click(screen.getByRole("button", { name: "Reply" }));

    expect(onReply).toHaveBeenCalledWith(message);
  });

  it("offers no reply control where there is nothing to pass it to", () => {
    // The thread panel, where the box at the bottom is already the reply.
    drawWithActions([said("$1", ADA, "the original")], {});

    expect(
      screen.queryByRole("button", { name: "Reply" }),
    ).not.toBeInTheDocument();
  });

  it("asks for the address of the message whose control was pressed", async () => {
    const onCopyLink = vi.fn();
    drawWithActions(
      [said("$1", ADA, "one"), said("$2", BOB, "two", NOON + 1_000)],
      { onCopyLink },
    );

    const [, second] = screen.getAllByRole("button", { name: "Copy link" });
    await userEvent.click(second!);

    expect(onCopyLink).toHaveBeenCalledWith("$2");
  });

  it("says a copy worked, on that message and no other", () => {
    // A copy is silent otherwise, and a control that says nothing invites a
    // second press. Which message it was matters: the toolbar looks the same
    // on every row.
    drawWithActions(
      [said("$1", ADA, "one"), said("$2", BOB, "two", NOON + 1_000)],
      { onCopyLink: vi.fn(), copiedId: "$1" },
    );

    expect(screen.getByRole("button", { name: "Link copied" })).toBeVisible();
    expect(screen.getAllByRole("button", { name: "Copy link" })).toHaveLength(1);
  });

  it("quotes an answered permalink as words rather than as an address", () => {
    // The row is a button and a badge is a button too, so the quote cannot
    // hold one. What it can do is say the same thing.
    const messages = [
      said(
        "$1",
        ADA,
        "Testing https://matrix.to/#/!voice:example.org/$said:example.org",
      ),
      said("$2", BOB, "quite", NOON + 1_000, { replyTo: "$1" }),
    ];
    render(
      <MessageGroups
        groups={group(messages)}
        names={{ [ADA]: "Ada" }}
        roomId={GENERAL}
        selfId={BOB}
        known={known(messages)}
        onAbout={vi.fn()}
      />,
    );

    const row = screen.getByRole("button", { name: /go to ada's message/i });
    expect(row).toHaveTextContent("Testing A message");
    expect(row).not.toHaveTextContent("matrix.to");
  });

  it("says so plainly when the answered message is not loaded", () => {
    // A room shows a window of history and a reply can point outside it. The
    // event ID is known and the message is not, and a row that pretended
    // otherwise would be a control that goes nowhere.
    draw([said("$2", BOB, "agreed", NOON, { replyTo: "$missing" })]);

    expect(screen.getByText(/not loaded/i)).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /go to/i }),
    ).not.toBeInTheDocument();
  });

  it("names an attachment rather than drawing an empty quote", () => {
    draw([
      said("$1", ADA, "", NOON, {
        kind: "image",
        media: {
          source: '{"url":"mxc://example.org/a"}',
          name: "screenshot.png",
        },
      }),
      said("$2", BOB, "nice", NOON + 1_000, { replyTo: "$1" }),
    ]);

    expect(
      screen.getByRole("button", { name: /go to ada's message/i }),
    ).toHaveTextContent("screenshot.png");
  });
});

describe("a mention", () => {
  it("marks a message that names whoever is signed in", () => {
    const { container } = draw([
      said("$1", ADA, "bob: have a look", NOON, { mentions: [BOB] }),
    ]);

    expect(container.querySelector('[data-message-id="$1"]')).toHaveAttribute(
      "data-mentions-me",
      "true",
    );
  });

  it("leaves a message that names somebody else alone", () => {
    const { container } = draw([
      said("$1", ADA, "ada: have a look", NOON, { mentions: [ADA] }),
    ]);

    expect(
      container.querySelector('[data-message-id="$1"]'),
    ).not.toHaveAttribute("data-mentions-me");
  });

  it("leaves a message that names nobody alone", () => {
    const { container } = draw([said("$1", ADA, "morning")]);

    expect(
      container.querySelector('[data-message-id="$1"]'),
    ).not.toHaveAttribute("data-mentions-me");
  });
});

describe("a link", () => {
  it("draws an address somebody pasted as something to press", () => {
    // A pasted link arrives with no formatting on it at all, so this is the
    // path the commonest link in a room takes.
    draw([said("$1", ADA, "have a look at https://example.org/x")]);

    expect(
      screen.getByRole("link", { name: "https://example.org/x" }),
    ).toBeVisible();
  });
});

describe("reactions", () => {
  const cheered = said("$1", ADA, "it works", NOON, {
    reactions: [{ key: "🎉", count: 2 }],
  });

  it("says what people used and how many of them", () => {
    draw([cheered]);

    expect(screen.getByRole("button", { name: "🎉, 2" })).toBeVisible();
  });

  it("shows nothing at all on a message nobody reacted to", () => {
    // A row of empty space under every line is a row of empty space under
    // every line.
    const { container } = draw([said("$1", ADA, "hello")]);

    expect(container.querySelector(".timeline__reactions")).toBeNull();
  });

  it("adds one when a key this session has not used is pressed", () => {
    const onReact = vi.fn();
    drawReactable([cheered], onReact);

    fireEvent.click(screen.getByRole("button", { name: "🎉, 2" }));

    expect(onReact).toHaveBeenCalledWith("$1", "🎉", undefined);
  });

  it("offers this session's own back, with the event that undoes it", () => {
    // The whole reason `mine` is an event ID: taking a reaction back is
    // redacting that exact event.
    const onReact = vi.fn();
    drawReactable(
      [
        said("$1", ADA, "it works", NOON, {
          reactions: [{ key: "🎉", count: 2, mine: "$mine" }],
        }),
      ],
      onReact,
    );

    const pill = screen.getByRole("button", { name: "🎉, 2" });
    expect(pill).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(pill);
    expect(onReact).toHaveBeenCalledWith("$1", "🎉", "$mine");
  });

  it("picks a new key from the control on the message", async () => {
    const onReact = vi.fn();
    drawReactable([said("$1", ADA, "hello")], onReact);

    await userEvent.click(screen.getByRole("button", { name: "React" }));
    await userEvent.click(screen.getByRole("button", { name: "React with 👍" }));

    expect(onReact).toHaveBeenCalledWith("$1", "👍", undefined);
  });

  it("takes a key back when it is picked again from the same panel", async () => {
    // The picker draws what this session has already used as pressed, so
    // pressing it there has to mean the same thing as pressing the pill.
    const onReact = vi.fn();
    drawReactable(
      [
        said("$1", ADA, "hello", NOON, {
          reactions: [{ key: "👍", count: 1, mine: "$mine" }],
        }),
      ],
      onReact,
    );

    await userEvent.click(screen.getByRole("button", { name: "React" }));
    await userEvent.click(screen.getByRole("button", { name: "React with 👍" }));

    expect(onReact).toHaveBeenCalledWith("$1", "👍", "$mine");
  });

  it("closes the picker once a key has been chosen", async () => {
    drawReactable([said("$1", ADA, "hello")], vi.fn());
    await userEvent.click(screen.getByRole("button", { name: "React" }));

    await userEvent.click(screen.getByRole("button", { name: "React with 👍" }));

    expect(screen.queryByRole("group", { name: "React with" })).toBeNull();
  });

  it("closes the picker on Escape", async () => {
    drawReactable([said("$1", ADA, "hello")], vi.fn());
    await userEvent.click(screen.getByRole("button", { name: "React" }));

    await userEvent.keyboard("{Escape}");

    expect(screen.queryByRole("group", { name: "React with" })).toBeNull();
  });

  it("opens one picker at a time", async () => {
    // Two panels of the same twelve keys with nothing saying which message
    // either belongs to.
    drawReactable(
      [said("$1", ADA, "one"), said("$2", ADA, "two", NOON + 1_000)],
      vi.fn(),
    );

    const both = screen.getAllByRole("button", { name: "React" });
    await userEvent.click(both[0]!);
    await userEvent.click(both[1]!);

    expect(screen.getAllByRole("group", { name: "React with" })).toHaveLength(1);
  });
});

describe("adding another reaction", () => {
  const cheered = said("$1", ADA, "it works", NOON, {
    reactions: [{ key: "🎉", count: 2 }],
  });

  it("offers a control beside the reactions a message already has", () => {
    const { container } = drawReactable([cheered], vi.fn());

    const row = container.querySelector(".timeline__reactions");
    expect(row).not.toBeNull();
    expect(
      within(row as HTMLElement).getByRole("button", { name: "Add a reaction" }),
    ).toHaveAttribute("aria-expanded", "false");
  });

  it("offers none on a message nobody has reacted to", () => {
    // Nothing for it to sit beside. The toolbar is where that message's first
    // reaction comes from.
    drawReactable([said("$1", ADA, "hello")], vi.fn());

    expect(screen.queryByRole("button", { name: "Add a reaction" })).toBeNull();
  });

  it("picks a key from the control beside the pills", async () => {
    const onReact = vi.fn();
    drawReactable([cheered], onReact);

    await userEvent.click(screen.getByRole("button", { name: "Add a reaction" }));
    await userEvent.click(screen.getByRole("button", { name: "React with 👍" }));

    expect(onReact).toHaveBeenCalledWith("$1", "👍", undefined);
  });

  it("draws the panel beside the control that opened it", async () => {
    // The whole point of this control: a panel that opened by the toolbar
    // would be the journey it exists to remove.
    const { container } = drawReactable([cheered], vi.fn());

    await userEvent.click(screen.getByRole("button", { name: "Add a reaction" }));

    expect(container.querySelector(".timeline__reactions .picker")).not.toBeNull();
    expect(container.querySelector(".timeline__actions .picker")).toBeNull();
  });

  it("draws it by the toolbar when the toolbar is what was pressed", async () => {
    const { container } = drawReactable([cheered], vi.fn());

    await userEvent.click(screen.getByRole("button", { name: "React" }));

    expect(container.querySelector(".timeline__actions .picker")).not.toBeNull();
    expect(container.querySelector(".timeline__reactions .picker")).toBeNull();
  });

  it("opens one panel at a time, whichever control was pressed", async () => {
    drawReactable([cheered], vi.fn());

    await userEvent.click(screen.getByRole("button", { name: "React" }));
    await userEvent.click(screen.getByRole("button", { name: "Add a reaction" }));

    expect(screen.getAllByRole("group", { name: "React with" })).toHaveLength(1);
  });

  it("marks a key this session has already used", async () => {
    drawReactable(
      [
        said("$1", ADA, "it works", NOON, {
          reactions: [{ key: "🎉", count: 2, mine: "$mine" }],
        }),
      ],
      vi.fn(),
    );

    await userEvent.click(screen.getByRole("button", { name: "Add a reaction" }));

    expect(screen.getByRole("button", { name: "React with 🎉" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});
