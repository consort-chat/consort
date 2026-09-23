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

import {
  MessageGroups,
  dayLabel,
  firstOfEachDay,
  firstUnread,
  group,
  timeOf,
} from "./MessageGroups";
import { ConfirmDelete } from "./ConfirmDelete";
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
    onEdit?: (message: Message) => void;
    onDelete?: (message: Message) => void;
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

describe("when something was said", () => {
  const MINUTE = 60 * 1000;

  it("puts a time on every message in a group, not only the first", () => {
    // A group is one person talking without pause, so six messages used to
    // carry one time between them. "When was that said" is a question about
    // the message rather than about the burst it arrived in.
    const { container } = draw([
      said("$1", ADA, "one"),
      said("$2", ADA, "two", NOON + MINUTE),
      said("$3", ADA, "three", NOON + 2 * MINUTE),
    ]);

    expect(container.querySelectorAll("time")).toHaveLength(3);
    expect(screen.getByText(timeOf(NOON + MINUTE))).toBeVisible();
    expect(screen.getByText(timeOf(NOON + 2 * MINUTE))).toBeVisible();
  });

  it("says a group's first time once, in the byline", () => {
    // The byline sits directly above it. The same clock time on both lines
    // would be the same fact twice.
    const { container } = draw([
      said("$1", ADA, "one"),
      said("$2", ADA, "two", NOON + MINUTE),
    ]);

    expect(screen.getAllByText(timeOf(NOON))).toHaveLength(1);
    expect(container.querySelectorAll("time")).toHaveLength(2);
  });

  it("draws one time for one message", () => {
    const { container } = draw([said("$1", ADA, "alone")]);

    expect(container.querySelectorAll("time")).toHaveLength(1);
    expect(screen.getByText(timeOf(NOON))).toBeVisible();
  });

  it("leaves two groups with a byline each and nothing in the gutter", () => {
    // Far enough apart to be two things to read, so each already has its own
    // byline and there is no continuation to put a time beside.
    const { container } = draw([
      said("$1", ADA, "one"),
      said("$2", ADA, "two", NOON + 60 * MINUTE),
    ]);

    expect(container.querySelectorAll("time")).toHaveLength(2);
  });

  it("carries the whole date, for the times a clock face cannot say", () => {
    // The tooltip is on the time rather than on the words, so a pointer
    // crossing a room does not raise one over the thing being read.
    draw([said("$1", ADA, "one"), said("$2", ADA, "two", NOON + MINUTE)]);

    expect(screen.getByText(timeOf(NOON + MINUTE))).toHaveAttribute(
      "title",
      new Date(NOON + MINUTE).toLocaleString(),
    );
  });
});

describe("where reading stopped", () => {
  const conversation = [
    said("$1", ADA, "one"),
    said("$2", BOB, "two", NOON + 1_000),
    said("$3", ADA, "three", NOON + 2_000),
  ];

  it("names the message after the one that was read", () => {
    expect(firstUnread(conversation, "$1")).toBe("$2");
  });

  it("names nothing when the last message read is the newest one", () => {
    // Nothing new to point at. A line drawn anyway would sit below the end of
    // the conversation.
    expect(firstUnread(conversation, "$3")).toBeUndefined();
  });

  it("names nothing when the marker is outside what is loaded", () => {
    // A room somebody read a year ago and has not opened since. The marker is
    // real; the message it names is not on screen, and a line drawn anyway
    // would sit above the whole window.
    expect(firstUnread(conversation, "$last-year")).toBeUndefined();
  });

  it("names nothing for a room this account has never read", () => {
    expect(firstUnread(conversation, undefined)).toBeUndefined();
  });

  it("names nothing in an empty room", () => {
    expect(firstUnread([], "$1")).toBeUndefined();
  });

  it("draws the line above the first message that is new", () => {
    render(
      <MessageGroups
        groups={group(conversation)}
        names={{ [ADA]: "Ada", [BOB]: "Bob" }}
        roomId={GENERAL}
        selfId={BOB}
        known={known(conversation)}
        onAbout={vi.fn()}
        newFrom="$2"
      />,
    );

    expect(screen.getByText("New messages")).toBeInTheDocument();
  });

  it("draws one line, not one per group it could fall between", () => {
    // A group is one person talking without pause, so the last thing read and
    // the first thing new are regularly two messages inside one of them. Both
    // the group check and the message check have to be able to draw it, and
    // exactly one of them may.
    render(
      <MessageGroups
        groups={group([
          said("$1", ADA, "one"),
          said("$2", ADA, "two", NOON + 1_000),
        ])}
        names={{ [ADA]: "Ada" }}
        roomId={GENERAL}
        selfId={BOB}
        known={known(conversation)}
        onAbout={vi.fn()}
        newFrom="$2"
      />,
    );

    expect(screen.getAllByText("New messages")).toHaveLength(1);
  });

  it("draws no line when nothing is new", () => {
    render(
      <MessageGroups
        groups={group(conversation)}
        names={{ [ADA]: "Ada", [BOB]: "Bob" }}
        roomId={GENERAL}
        selfId={BOB}
        known={known(conversation)}
        onAbout={vi.fn()}
      />,
    );

    expect(screen.queryByText("New messages")).not.toBeInTheDocument();
  });
});

/*
  Every timestamp below is built with the local constructor rather than a UTC
  string or a bare epoch number, because the rule being pinned is about the
  local calendar day. `Date.UTC(2026, 0, 1, 23, 58)` is the 1st in London and
  the 2nd in Sydney, so a test written that way asserts something different on
  every machine that runs it.
*/
const LATE = new Date(2026, 0, 1, 23, 58).getTime();
const JUST_AFTER = new Date(2026, 0, 2, 0, 1).getTime();

describe("the day a message was said", () => {
  it("names only the first message of a run inside one day", () => {
    const opens = firstOfEachDay([
      said("$1", ADA, "one", new Date(2026, 0, 1, 9, 0).getTime()),
      said("$2", BOB, "two", new Date(2026, 0, 1, 14, 0).getTime()),
      said("$3", ADA, "three", new Date(2026, 0, 1, 22, 0).getTime()),
    ]);

    expect([...opens]).toEqual(["$1"]);
  });

  it("names the first message of the second day when a room spans midnight", () => {
    const opens = firstOfEachDay([
      said("$1", ADA, "one", LATE),
      said("$2", BOB, "two", JUST_AFTER),
      said("$3", BOB, "three", new Date(2026, 0, 2, 9, 0).getTime()),
    ]);

    expect([...opens]).toEqual(["$1", "$2"]);
  });

  it("names the first loaded message, because the day it opens has to be said", () => {
    // Whatever is at the top of the window starts a day as far as a reader is
    // concerned. Paging older history in may take the line away again, which
    // is the right answer recomputing rather than a flicker to suppress.
    expect([...firstOfEachDay([said("$1", ADA, "alone")])]).toEqual(["$1"]);
  });

  it("names nothing in an empty room", () => {
    expect(firstOfEachDay([]).size).toBe(0);
  });

  it("names a message inside a group when one person talks across midnight", () => {
    // Three minutes apart and the same sender, so `group` reads them as one
    // burst. The day changed in the middle of it all the same.
    const messages = [
      said("$1", ADA, "one", LATE),
      said("$2", ADA, "two", JUST_AFTER),
    ];

    expect(group(messages)).toHaveLength(1);
    expect([...firstOfEachDay(messages)]).toEqual(["$1", "$2"]);
  });

  it("names one message when a day change and a group change land together", () => {
    // Different people either side of midnight, so both rules fire on `$2`.
    // The set holds IDs, so there is one of it and the drawing cannot double.
    const opens = firstOfEachDay([
      said("$1", ADA, "one", LATE),
      said("$2", BOB, "two", JUST_AFTER),
    ]);

    expect([...opens]).toEqual(["$1", "$2"]);
  });
});

describe("what a date separator says", () => {
  const NOW = new Date(2026, 2, 15, 12, 0).getTime();
  const on = (year: number, month: number, day: number) =>
    new Date(year, month, day, 10, 0).getTime();

  it("says Today for the day being read", () => {
    expect(dayLabel(on(2026, 2, 15), NOW)).toBe("Today");
  });

  it("says Today for a message earlier in the same day", () => {
    // The boundary is the calendar day, not twenty-four hours. Something said
    // at one in the morning is still today at noon.
    expect(dayLabel(new Date(2026, 2, 15, 1, 0).getTime(), NOW)).toBe("Today");
  });

  it("says Yesterday for the day before", () => {
    expect(dayLabel(on(2026, 2, 14), NOW)).toBe("Yesterday");
  });

  it("says the weekday inside the last week", () => {
    const at = on(2026, 2, 10);

    expect(dayLabel(at, NOW)).toBe(
      new Date(at).toLocaleDateString(undefined, { weekday: "long" }),
    );
    // A weekday and nothing else. The date proper starts once a weekday stops
    // being enough to place it.
    expect(dayLabel(at, NOW)).not.toMatch(/\d/);
  });

  it("says the whole date beyond that", () => {
    const at = on(2026, 1, 3);

    expect(dayLabel(at, NOW)).toBe(
      new Date(at).toLocaleDateString(undefined, {
        weekday: "long",
        day: "numeric",
        month: "long",
      }),
    );
  });

  it("drops the year when it is the current one", () => {
    expect(dayLabel(on(2026, 1, 3), NOW)).not.toContain("2026");
  });

  it("says the whole date for a message dated ahead of this clock", () => {
    // `origin_server_ts` is the homeserver's clock, not this machine's, so a
    // local clock a day behind makes the newest message tomorrow. A weekday
    // would read as last week; the date says what it actually claims.
    const at = on(2026, 2, 17);

    expect(dayLabel(at, NOW)).toBe(
      new Date(at).toLocaleDateString(undefined, {
        weekday: "long",
        day: "numeric",
        month: "long",
      }),
    );
  });

  it("says the year when it is not", () => {
    expect(dayLabel(on(2025, 10, 20), NOW)).toContain("2025");
  });
});

describe("drawing the day", () => {
  /** Draw them as a room does, which is the only caller that asks for days. */
  function drawWithDays(messages: Message[], newFrom?: string) {
    return render(
      <MessageGroups
        groups={group(messages)}
        names={{ [ADA]: "Ada", [BOB]: "Bob" }}
        roomId={GENERAL}
        selfId={BOB}
        known={known(messages)}
        onAbout={vi.fn()}
        newDay={firstOfEachDay(messages)}
        newFrom={newFrom}
      />,
    );
  }

  it("draws a separator above the first message of a new day", () => {
    const { container } = drawWithDays([
      said("$1", ADA, "one", LATE),
      said("$2", BOB, "two", JUST_AFTER),
    ]);

    expect(container.querySelectorAll("[data-day-line]")).toHaveLength(2);
    expect(screen.getByText(dayLabel(JUST_AFTER))).toBeVisible();
  });

  it("draws it inside a group that straddles midnight", () => {
    // One person, one group, and the line still has to land on the second
    // message rather than above the pair of them.
    const { container } = drawWithDays([
      said("$1", ADA, "one", LATE),
      said("$2", ADA, "two", JUST_AFTER),
    ]);

    expect(container.querySelectorAll("article")).toHaveLength(1);
    expect(container.querySelectorAll("[data-day-line]")).toHaveLength(2);
  });

  it("draws the date above the new messages line when both land together", () => {
    // The day is the larger container and the unread mark belongs inside the
    // day it falls in, so the order on the page is date first.
    const { container } = drawWithDays(
      [said("$1", ADA, "one", LATE), said("$2", ADA, "two", JUST_AFTER)],
      "$2",
    );

    const lines = [
      ...container.querySelectorAll("[data-day-line], [data-unread-line]"),
    ];

    expect(lines.at(-2)).toHaveAttribute("data-day-line");
    expect(lines.at(-1)).toHaveAttribute("data-unread-line");
  });

  it("draws nothing when no day set is passed, as a thread panel does", () => {
    const { container } = draw([
      said("$1", ADA, "one", LATE),
      said("$2", BOB, "two", JUST_AFTER),
    ]);

    expect(container.querySelectorAll("[data-day-line]")).toHaveLength(0);
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

  it("draws the first message of a group right after the byline", () => {
    // What the flash reaches back over. A jump landing on the first thing
    // somebody said lights the name and the picture above it too, and the rule
    // doing that finds them from the message next to the byline. Anything
    // drawn between the two lights the words with whoever wrote them dark.
    const { container } = draw([
      said("$1", ADA, "the original"),
      said("$2", ADA, "and again", NOON + 1_000),
    ]);

    const byline = container.querySelector(".timeline__byline");

    expect(byline?.nextElementSibling).toHaveAttribute("data-message-id", "$1");
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

  it("draws a message that is named but not on screen like any other reply", () => {
    // The room looks it up, because a reply can name anything older than the
    // window of history loaded and a row saying only that it cannot say is a
    // row where every other reply has a name and a line of what was said.
    const answered = said("$old", ADA, "the one being answered");
    render(
      <MessageGroups
        groups={group([said("$2", BOB, "quite", NOON, { replyTo: "$old" })])}
        names={{ [ADA]: "Ada" }}
        roomId={GENERAL}
        selfId={BOB}
        known={known([answered])}
        onAbout={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: /go to ada's message/i }),
    ).toHaveTextContent("the one being answered");
  });

  it("hands back a message it names but does not draw, rather than scrolling", () => {
    // Nothing to scroll to: the message is not in the box. The room's own
    // answer to that is to fetch the history around it.
    const onGoTo = vi.fn();
    const answered = said("$old", ADA, "the one being answered");
    render(
      <MessageGroups
        groups={group([said("$2", BOB, "quite", NOON, { replyTo: "$old" })])}
        names={{ [ADA]: "Ada" }}
        roomId={GENERAL}
        selfId={BOB}
        known={known([answered])}
        onAbout={vi.fn()}
        onGoTo={onGoTo}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /go to ada's message/i }));

    expect(onGoTo).toHaveBeenCalledWith("$old");
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

describe("a message that was edited", () => {
  it("says so", () => {
    // A correction drawn as though it were what somebody first wrote is a
    // quiet way of putting words in their mouth.
    draw([said("$1", ADA, "corrected", NOON, { edited: true })]);

    expect(screen.getByText("(edited)")).toBeInTheDocument();
  });

  it("says nothing on a message nobody edited", () => {
    draw([said("$1", ADA, "as it was sent")]);

    expect(screen.queryByText("(edited)")).not.toBeInTheDocument();
  });

  it("puts the mark inside the words rather than on a line of its own", () => {
    // Inline, because a row of its own would space the conversation out by
    // the height of a line on every message anybody has ever corrected.
    draw([said("$1", ADA, "corrected", NOON, { edited: true })]);

    const body = screen.getByText("corrected").closest(".timeline__body");
    expect(body).toContainElement(screen.getByText("(edited)"));
  });
});

describe("correcting a message", () => {
  it("offers the control on this account's own message", () => {
    drawWithActions([said("$1", BOB, "teh typo")], { onEdit: vi.fn() });

    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
  });

  it("offers nothing on somebody else's", () => {
    // The second lock is in Rust, which reads who sent the target before it
    // will build anything. This is the first: no control to press.
    drawWithActions([said("$1", ADA, "what they said")], { onEdit: vi.fn() });

    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
  });

  it("offers nothing on an attachment of this account's own", () => {
    // An edit carries replacement text, and replacing a picture with a
    // sentence is what every client that folds one would then draw. There is
    // no caption editing surface, so there is nothing to offer here yet.
    drawWithActions(
      [
        said("$1", BOB, "what it is a picture of", NOON, {
          kind: "image",
          media: { source: "{}", name: "shot.png" },
        }),
      ],
      { onEdit: vi.fn() },
    );

    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
  });

  it("offers nothing on a message this session could not read", () => {
    drawWithActions(
      [said("$1", BOB, "Waiting for the key", NOON, { kind: "undecryptable" })],
      { onEdit: vi.fn() },
    );

    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
  });

  it("offers nothing where there is nothing to pass it to", () => {
    // The thread panel, which has no composer mode for an edit.
    drawWithActions([said("$1", BOB, "teh typo")], {});

    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
  });

  it("hands the whole message to whoever is going to correct it", async () => {
    // Not the ID. The composer opens on what the message currently says, and
    // an ID alone cannot fill the box.
    const onEdit = vi.fn();
    const message = said("$1", BOB, "teh typo");
    drawWithActions([message], { onEdit });

    await userEvent.click(screen.getByRole("button", { name: "Edit" }));

    expect(onEdit).toHaveBeenCalledWith(message);
  });
});
describe("deleting a message", () => {
  /** The message as Rust draws one the homeserver has emptied. */
  function emptied(sender: string, extra: Partial<Message> = {}): Message {
    return said("$gone", sender, "", NOON, { kind: "deleted", ...extra });
  }

  it("offers the control on this account's own message", () => {
    drawWithActions([said("$1", BOB, "wrong number")], { onDelete: vi.fn() });

    expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
  });

  it("offers nothing on somebody else's", () => {
    // A moderator removing one is a power level read and a different
    // question, so there is nothing here to press either way.
    drawWithActions([said("$1", ADA, "what they said")], { onDelete: vi.fn() });

    expect(
      screen.queryByRole("button", { name: "Delete" }),
    ).not.toBeInTheDocument();
  });

  it("offers the control on an attachment, where correcting one is refused", () => {
    // The case this rule exists for. An edit carries replacement text and
    // cannot replace a picture, where a redaction removes an event whatever
    // was inside it, and a file nobody meant to send is the thing people most
    // want this for.
    drawWithActions(
      [
        said("$1", BOB, "", NOON, {
          kind: "image",
          media: { source: "{}", name: "shot.png" },
        }),
      ],
      { onDelete: vi.fn(), onEdit: vi.fn() },
    );

    expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Edit" }),
    ).not.toBeInTheDocument();
  });

  it("does not delete on the first press", async () => {
    // The rule this whole control is shaped by. A redaction cannot be taken
    // back, and the control sits one button away from Edit in a row that
    // appears on hover.
    const onDelete = vi.fn();
    drawWithActions([said("$1", BOB, "wrong number")], { onDelete });

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));

    expect(onDelete).not.toHaveBeenCalled();
    expect(
      screen.getByRole("dialog", { name: "Delete this message?" }),
    ).toBeInTheDocument();
  });

  it("says what deleting actually does before it is done", () => {
    // Redacting is not erasing, and a sentence promising otherwise would be a
    // promise Consort cannot keep across federation.
    render(
      <ConfirmDelete onConfirm={vi.fn()} onCancel={vi.fn()} />,
    );

    expect(
      screen.getByText(/Servers and clients that already have a copy may keep it/),
    ).toBeInTheDocument();
  });

  it("puts the focus on the half that does nothing", () => {
    // The first press rule, one layer down: the key somebody hits without
    // reading is Enter, and it must not land on the irreversible answer to a
    // question they have not read.
    render(<ConfirmDelete onConfirm={vi.fn()} onCancel={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });

  it("deletes on the second press, and hands over the whole message", async () => {
    const onDelete = vi.fn();
    const message = said("$1", BOB, "wrong number");
    drawWithActions([message], { onDelete });

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    await userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }),
    );

    expect(onDelete).toHaveBeenCalledWith(message);
  });

  it("deletes nothing when the question is answered no", async () => {
    const onDelete = vi.fn();
    drawWithActions([said("$1", BOB, "wrong number")], { onDelete });

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onDelete).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("deletes nothing when the question is dismissed with Escape", async () => {
    const onDelete = vi.fn();
    drawWithActions([said("$1", BOB, "wrong number")], { onDelete });

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    await userEvent.keyboard("{Escape}");

    expect(onDelete).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("draws a mark where a deleted message was", () => {
    // The gap is the thing being avoided. A reply under a message that
    // vanished answers nothing, and every other client draws a mark here.
    draw([said("$1", ADA, "morning all"), emptied(ADA)]);

    expect(screen.getByText("Message deleted")).toBeInTheDocument();
    expect(screen.getByText("morning all")).toBeInTheDocument();
  });

  it("names whoever deleted it when it was not the author", () => {
    // A moderator removing somebody's message and that person removing their
    // own are one event with a different sender on it, and the mark sits
    // under the author's name either way.
    draw([emptied(ADA, { deletedBy: BOB })]);

    expect(
      screen.getByText(`Message deleted by ${BOB}`),
    ).toBeInTheDocument();
  });

  it("names nobody when the author deleted their own", () => {
    draw([emptied(ADA, { deletedBy: ADA })]);

    expect(screen.getByText("Message deleted")).toBeInTheDocument();
  });

  it("names nobody when the redaction said nothing about who sent it", () => {
    // Unknown is drawn as unknown rather than as the author, because of the
    // two readings only one can say something untrue.
    draw([emptied(ADA)]);

    expect(screen.getByText("Message deleted")).toBeInTheDocument();
  });

  it("offers nothing to do to a message that is gone", () => {
    // Not even deleting it again: there is nothing left to remove, and the
    // homeserver would refuse a second redaction of one event.
    drawWithActions([emptied(BOB)], {
      onDelete: vi.fn(),
      onEdit: vi.fn(),
      onReply: vi.fn(),
      onCopyLink: vi.fn(),
    });

    expect(
      screen.queryByRole("button", { name: "Delete" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Reply" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Edit" }),
    ).not.toBeInTheDocument();
  });

  it("says so in the row above a reply naming a deleted message", () => {
    // Without this the row falls through to the attachment fallback and tells
    // somebody a file was sent.
    draw([
      emptied(ADA),
      said("$reply", BOB, "quite", NOON, { replyTo: "$gone" }),
    ]);

    expect(screen.getAllByText("Message deleted").length).toBeGreaterThan(1);
  });
});
