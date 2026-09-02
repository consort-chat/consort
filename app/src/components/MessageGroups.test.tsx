import { render, screen } from "@testing-library/react";
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

/**
 * Draw the messages, with a handler by default.
 *
 * The handler is what a room passes and a thread panel does not, and a message
 * drawn without one offers nothing to open. Every test here that is about the
 * control rather than about its absence needs one.
 */
function draw(
  messages: Message[],
  onOpenThread: ((rootId: string) => void) | undefined = vi.fn(),
) {
  return render(
    <MessageGroups
      groups={group(messages)}
      names={{ [ADA]: "Ada" }}
      roomId={GENERAL}
      onAbout={vi.fn()}
      onOpenThread={onOpenThread}
    />,
  );
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
  it("offers nothing to open", () => {
    // A count of zero under every line in a room would say "no thread here" on
    // every one of them, which is not information.
    draw([said("$1", ADA, "hello")]);

    expect(screen.queryByRole("button", { name: /repl/i })).toBeNull();
  });

  it("does nothing when it is pressed", async () => {
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
