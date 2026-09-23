import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const onThread = vi.hoisted(() => vi.fn());
const threadOpen = vi.hoisted(() => vi.fn());
const threadSend = vi.hoisted(() => vi.fn());
const resendState = vi.hoisted(() => vi.fn());
const memberNames = vi.hoisted(() => vi.fn());
const timelineCopyLink = vi.hoisted(() => vi.fn());
const timelineEdit = vi.hoisted(() => vi.fn());
const timelineDelete = vi.hoisted(() => vi.fn());
const memberAvatar = vi.hoisted(() => vi.fn());
const memberProfile = vi.hoisted(() => vi.fn());
// For the card a name opens, which reads its own saved volume.
const audioSettings = vi.hoisted(() => vi.fn());
const setPersonVolume = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  onThread,
  threadOpen,
  threadSend,
  resendState,
  memberNames,
  memberAvatar,
  timelineCopyLink,
  timelineEdit,
  timelineDelete,
  memberProfile,
  audioSettings,
  setPersonVolume,
}));

import { ThreadPanel } from "./ThreadPanel";
import { fakeScrolling } from "../test/scrolling";
import { resetAvatarCache } from "../lib/avatars";
import { resetPresenceCache } from "../lib/presence";
import type { Message, Thread } from "../lib/api";

const GENERAL = "!general:example.org";
const ADA = "@ada:example.org";
const LIN = "@lin:example.org";
const NOON = Date.parse("2026-01-01T12:00:00Z");

function said(id: string, body: string, at = NOON): Message {
  return { id, sender: ADA, at, body, kind: "text" };
}

/** A reply with a picture on it, which is the thing that arrives late. */
function picture(id: string): Message {
  return {
    ...said(id, ""),
    kind: "image",
    media: {
      source: '{"url":"mxc://example.org/abc"}',
      name: "screenshot.png",
      mime: "image/png",
      width: 800,
      height: 600,
    },
  };
}

const OPEN: Thread = {
  roomId: GENERAL,
  rootId: "$root:example.org",
  root: said("$root:example.org", "what shall we call it"),
  messages: [said("$a:example.org", "Consort", NOON + 1_000)],
  moreBefore: false,
};

/** Hand the subscribed panel a thread, or the news that none is open. */
let publish: (thread: Thread | null) => void;

beforeEach(() => {
  resetAvatarCache();
  resetPresenceCache();
  memberAvatar.mockReset().mockResolvedValue(null);
  memberProfile.mockReset().mockResolvedValue(null);
  memberNames.mockReset().mockResolvedValue({ [ADA]: "Ada" });
  timelineCopyLink.mockReset().mockResolvedValue(undefined);
  timelineEdit.mockReset().mockResolvedValue(undefined);
  timelineDelete.mockReset().mockResolvedValue(undefined);
  resendState.mockReset().mockResolvedValue(undefined);
  threadOpen.mockReset().mockResolvedValue(undefined);
  threadSend.mockReset().mockResolvedValue(undefined);
  audioSettings.mockReset().mockResolvedValue({ people: {} });
  setPersonVolume.mockReset().mockResolvedValue(undefined);
  onThread.mockReset().mockImplementation((handler: typeof publish) => {
    publish = handler;
    return Promise.resolve(() => {});
  });
  resized.mockReset();
});

const resized = vi.fn();

function draw() {
  return render(
    <ThreadPanel
      selfId={ADA}
      onOpenRoom={vi.fn()}
      onOpen={vi.fn()}
      width={400}
      onResize={resized}
    />,
  );
}

/**
 * One message's action control, counting from the root down.
 *
 * The panel draws the root above the rule and the replies below it, so nth 0
 * is the message the thread hangs from and 1 is its first reply.
 */
function action(name: string, nth: number): HTMLElement {
  const all = screen.getAllByRole("button", { name });
  const one = all[nth];
  if (one === undefined) {
    throw new Error(`there is no ${name} control on message ${nth}`);
  }
  return one;
}

async function opened(thread: Thread | null = OPEN) {
  draw();
  await waitFor(() => expect(onThread).toHaveBeenCalled());
  await act(async () => {
    publish(thread);
  });
}

describe("ThreadPanel", () => {
  it("draws nothing at all until a thread is opened", async () => {
    draw();
    await waitFor(() => expect(onThread).toHaveBeenCalled());

    expect(screen.queryByRole("complementary")).toBeNull();
  });

  it("catches itself up when it mounts", async () => {
    // The panel can be remounted with a thread already open in Rust, and the
    // thing that would otherwise fill it is somebody replying, which in a
    // finished conversation is never.
    draw();

    await waitFor(() => expect(resendState).toHaveBeenCalled());
  });

  it("asks to be caught up only once it is listening", async () => {
    // The race the resend exists for. Asking first is answered into the void,
    // and the panel then sits shut with a thread open behind it.
    let attached = false;
    onThread.mockImplementation((handler: typeof publish) => {
      publish = handler;
      return Promise.resolve(() => {
        attached = false;
      }).then((stop) => {
        attached = true;
        return stop;
      });
    });
    resendState.mockImplementation(() => {
      expect(attached).toBe(true);
      return Promise.resolve();
    });

    draw();

    await waitFor(() => expect(resendState).toHaveBeenCalledTimes(1));
  });

  it("draws the message the thread hangs from and its replies", async () => {
    await opened();

    // Twice over, since the head is named after the root as well as drawing
    // it below the rule. The second is the message itself.
    const [, root] = screen.getAllByText("what shall we call it");
    expect(root).toBeVisible();
    expect(screen.getByText("Consort")).toBeVisible();
  });

  it("separates no days, however far apart the root and its replies are", async () => {
    // A thread is one conversation read as a unit. A root from last month
    // with today's replies under it is the ordinary case, and a line between
    // them would be drawn almost every time the panel opened.
    await opened({
      ...OPEN,
      root: said(
        "$root:example.org",
        "what shall we call it",
        new Date(2026, 0, 1, 9, 0).getTime(),
      ),
      messages: [
        said("$a:example.org", "Consort", new Date(2026, 1, 3, 9, 0).getTime()),
      ],
    });

    expect(document.querySelectorAll("[data-day-line]")).toHaveLength(0);
  });

  it("shuts when the close control is pressed", async () => {
    await opened();

    await userEvent.click(screen.getByRole("button", { name: /close thread/i }));

    expect(threadOpen).toHaveBeenCalledWith(null);
  });

  it("shuts on Escape, the way everything else here is dismissed", async () => {
    await opened();

    await userEvent.keyboard("{Escape}");

    expect(threadOpen).toHaveBeenCalledWith(null);
  });

  it("leaves the thread open when a card over it takes the Escape", async () => {
    // One press, one thing. The card is on top, so it is what closes, and a
    // panel that went with it would take the conversation being read away.
    await opened();
    const [byline] = await screen.findAllByText("Ada");
    await userEvent.click(byline!);
    expect(await screen.findByRole("dialog")).toBeVisible();

    await userEvent.keyboard("{Escape}");

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(threadOpen).not.toHaveBeenCalled();
    expect(screen.getByRole("complementary")).toBeVisible();
  });

  it("goes away when Rust says nothing is open", async () => {
    await opened();
    await act(async () => {
      publish(null);
    });

    expect(screen.queryByRole("complementary")).toBeNull();
  });

  it("says when it is not showing the whole thread", async () => {
    // The recent end of a long thread is what comes back, and a panel that
    // drew it as though it were the whole would be lying about the top of it.
    await opened({ ...OPEN, moreBefore: true });

    expect(screen.getByText(/earlier replies/i)).toBeVisible();
  });

  it("says nothing about earlier replies when it has them all", async () => {
    await opened();

    expect(screen.queryByText(/earlier replies/i)).toBeNull();
  });

  it("still draws the replies when the root could not be fetched", async () => {
    // A redacted root and one this session has no key for both look like this,
    // and the replies are what somebody opened the panel to read.
    const { root: _root, ...rootless } = OPEN;
    await opened(rootless);

    expect(screen.getByText("Consort")).toBeVisible();
  });

  it("resolves the names of everybody in it", async () => {
    await opened();

    await waitFor(() =>
      expect(memberNames).toHaveBeenCalledWith(GENERAL, [ADA]),
    );
    // Twice: the root has a byline and so does the reply under it.
    expect(await screen.findAllByText("Ada")).toHaveLength(2);
  });

  it("opens somebody's card from a byline in it", async () => {
    // The panel is a place where somebody's name appears, so it has to be the
    // same name to press as it is in the room.
    await opened();

    const [byline] = await screen.findAllByText("Ada");
    await userEvent.click(byline!);

    expect(await screen.findByRole("dialog")).toBeVisible();
  });

  it("replies into the thread it is showing", async () => {
    await opened();

    await userEvent.type(screen.getByRole("textbox"), "Consort");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() =>
      expect(threadSend).toHaveBeenCalledWith(
        GENERAL,
        "$root:example.org",
        "$a:example.org",
        null,
        "Consort",
      ),
    );
  });

  it("answers the root itself when nothing has been replied yet", async () => {
    // The fallback has to point at something, and in an empty thread the only
    // thing said so far is the message it hangs from.
    await opened({ ...OPEN, messages: [] });

    await userEvent.type(screen.getByRole("textbox"), "first");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() =>
      expect(threadSend).toHaveBeenCalledWith(
        GENERAL,
        "$root:example.org",
        "$root:example.org",
        null,
        "first",
      ),
    );
  });

  it("offers a way to answer one reply, and to correct one", async () => {
    // Both were missing in here while the room beside it had them, which made
    // a thread the one place a typo could not be fixed.
    await opened();

    expect(screen.getAllByRole("button", { name: "Reply" })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "Edit" })).toHaveLength(2);
  });

  it("offers no correction of somebody else's message", async () => {
    await opened({
      ...OPEN,
      messages: [{ ...said("$a:example.org", "Consort"), sender: LIN }],
    });

    // The root is this account's own, so the count is what says the reply was
    // passed over rather than that nothing is drawn at all.
    expect(screen.getAllByRole("button", { name: "Edit" })).toHaveLength(1);
  });

  it("names the message being answered, and who wrote it", async () => {
    // The address alone would be the fallback every threaded reply carries,
    // which Rust reads as decoration and no client draws a quoted row for.
    await opened();

    await userEvent.click(action("Reply", 1));
    await userEvent.type(screen.getByRole("textbox"), "quite");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() =>
      expect(threadSend).toHaveBeenCalledWith(
        GENERAL,
        "$root:example.org",
        "$a:example.org",
        ADA,
        "quite",
      ),
    );
  });

  it("says above the box what the next reply will answer", async () => {
    await opened();

    await userEvent.click(action("Reply", 1));

    // Scoped to the line itself, because what it quotes is also still drawn
    // in the thread above it.
    const line = screen
      .getByRole("button", { name: "Stop replying" })
      .closest("div");
    expect(line).toHaveTextContent("Ada");
    expect(line).toHaveTextContent("Consort");
  });

  it("goes back to answering the thread when the reply is called off", async () => {
    await opened();

    await userEvent.click(action("Reply", 1));
    await userEvent.click(screen.getByRole("button", { name: "Stop replying" }));
    await userEvent.type(screen.getByRole("textbox"), "anyway");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() =>
      expect(threadSend).toHaveBeenCalledWith(
        GENERAL,
        "$root:example.org",
        "$a:example.org",
        null,
        "anyway",
      ),
    );
  });

  it("opens the box on what a message says when it is corrected", async () => {
    await opened();

    await userEvent.click(action("Edit", 1));

    expect(screen.getByRole("textbox")).toHaveValue("Consort");
  });

  it("corrects the message rather than saying the same thing twice", async () => {
    await opened();

    await userEvent.click(action("Edit", 1));
    await userEvent.clear(screen.getByRole("textbox"));
    await userEvent.type(screen.getByRole("textbox"), "Consort, with a t");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() =>
      expect(timelineEdit).toHaveBeenCalledWith(
        GENERAL,
        "$a:example.org",
        "Consort, with a t",
      ),
    );
    expect(threadSend).not.toHaveBeenCalled();
  });

  it("corrects the message the thread hangs from as readily as a reply", async () => {
    // It is drawn above the rule rather than in the list, and it is still a
    // message this account sent.
    await opened();

    await userEvent.click(action("Edit", 0));
    await userEvent.clear(screen.getByRole("textbox"));
    await userEvent.type(screen.getByRole("textbox"), "what shall we call it?");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() =>
      expect(timelineEdit).toHaveBeenCalledWith(
        GENERAL,
        "$root:example.org",
        "what shall we call it?",
      ),
    );
  });

  it("empties the box when a correction is called off", async () => {
    // What is in there is the old message rather than something somebody
    // typed, so leaving it would put a sentence already in the thread into
    // the next reply.
    await opened();

    await userEvent.click(action("Edit", 1));
    await userEvent.click(screen.getByRole("button", { name: "Stop editing" }));

    expect(screen.getByRole("textbox")).toHaveValue("");
  });

  it("keeps a correction and a reply from both being on at once", async () => {
    // One box with one Send cannot have two things to do.
    await opened();

    await userEvent.click(action("Reply", 1));
    await userEvent.click(action("Edit", 1));

    expect(screen.queryByRole("button", { name: "Stop replying" })).toBeNull();
    expect(screen.getByRole("button", { name: "Stop editing" })).toBeVisible();
  });

  it("says so when the correction is refused, and keeps what was typed", async () => {
    timelineEdit.mockRejectedValue({ message: "The homeserver refused that." });
    await opened();

    await userEvent.click(action("Edit", 1));
    await userEvent.type(screen.getByRole("textbox"), "!");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(
      await screen.findByText("The homeserver refused that."),
    ).toBeVisible();
    expect(screen.getByRole("textbox")).toHaveValue("Consort!");
  });

  it("deletes a reply once the question hanging off the control is answered", async () => {
    await opened();

    await userEvent.click(action("Delete", 1));
    await userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }),
    );

    await waitFor(() =>
      expect(timelineDelete).toHaveBeenCalledWith(GENERAL, "$a:example.org"),
    );
  });

  it("sends nothing on the first press of Delete", async () => {
    await opened();

    await userEvent.click(action("Delete", 1));

    expect(timelineDelete).not.toHaveBeenCalled();
  });

  it("deletes the message the thread hangs from as readily as a reply", async () => {
    // Drawn above the rule rather than in the list, and still a message this
    // account sent. What it leaves behind is a thread with no way into it
    // from the room, which is recorded on the pull request rather than
    // guarded against here.
    await opened();

    await userEvent.click(action("Delete", 0));
    await userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }),
    );

    await waitFor(() =>
      expect(timelineDelete).toHaveBeenCalledWith(GENERAL, "$root:example.org"),
    );
  });

  it("says so when the homeserver refuses the deletion", async () => {
    timelineDelete.mockRejectedValue({ message: "The homeserver refused that." });
    await opened();

    await userEvent.click(action("Delete", 1));
    await userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }),
    );

    expect(
      await screen.findByText("The homeserver refused that."),
    ).toBeVisible();
  });

  it("puts the box back to an ordinary reply once the correction lands", async () => {
    await opened();

    await userEvent.click(action("Edit", 1));
    await userEvent.type(screen.getByRole("textbox"), "!");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Stop editing" })).toBeNull(),
    );
    expect(screen.getByRole("textbox")).toHaveValue("");
  });

  it("ends the correction on escape rather than shutting the panel", async () => {
    // The panel's own escape listener is on the window, one step further out,
    // so without stopping the key here one press would do both.
    await opened();

    await userEvent.click(action("Edit", 1));
    await userEvent.type(screen.getByRole("textbox"), "{Escape}");

    expect(screen.queryByRole("button", { name: "Stop editing" })).toBeNull();
    expect(threadOpen).not.toHaveBeenCalled();
  });

  it("keeps a half-written reply when the reply it answers is called off", async () => {
    // Calling off a reply is not calling off what somebody typed. Only a
    // correction empties the box, because there it is the old message.
    await opened();

    await userEvent.click(action("Reply", 1));
    await userEvent.type(screen.getByRole("textbox"), "most of a sentence");
    await userEvent.type(screen.getByRole("textbox"), "{Escape}");

    expect(screen.queryByRole("button", { name: "Stop replying" })).toBeNull();
    expect(screen.getByRole("textbox")).toHaveValue("most of a sentence");
  });

  it("still shuts on escape when the box is an ordinary reply", async () => {
    await opened();

    await userEvent.type(screen.getByRole("textbox"), "{Escape}");

    expect(threadOpen).toHaveBeenCalledWith(null);
  });

  it("leaves nothing of one thread in the box when another is opened", async () => {
    // A message from the thread that was closed is not something the one now
    // open can answer, and the old text would go out as a new reply.
    await opened();

    await userEvent.click(action("Edit", 1));
    await act(async () => {
      publish({
        roomId: GENERAL,
        rootId: "$other:example.org",
        messages: [said("$b:example.org", "somewhere else")],
        moreBefore: false,
      });
    });

    expect(screen.queryByRole("button", { name: "Stop editing" })).toBeNull();
    expect(screen.getByRole("textbox")).toHaveValue("");
  });

  it("keeps what was typed when the send failed", async () => {
    // Retyping a message is the one thing an interface must never ask for.
    threadSend.mockRejectedValue({ message: "The homeserver refused that." });
    await opened();

    await userEvent.type(screen.getByRole("textbox"), "Consort");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(
      await screen.findByText("The homeserver refused that."),
    ).toBeVisible();
    expect(screen.getByRole("textbox")).toHaveValue("Consort");
  });

  it("empties the box once the homeserver has it", async () => {
    await opened();

    await userEvent.type(screen.getByRole("textbox"), "Consort");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => expect(screen.getByRole("textbox")).toHaveValue(""));
  });

  it("sends nothing when nothing has been typed", async () => {
    await opened();

    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(threadSend).not.toHaveBeenCalled();
  });

  it("sends on Enter and breaks the line on Shift+Enter", async () => {
    await opened();
    const box = screen.getByRole("textbox");

    await userEvent.type(box, "one{Shift>}{Enter}{/Shift}two");
    expect(threadSend).not.toHaveBeenCalled();

    await userEvent.type(box, "{Enter}");
    await waitFor(() => expect(threadSend).toHaveBeenCalled());
  });

  it("offers no way further in, because there is nowhere further to go", async () => {
    // Every message here is already in the thread being read.
    await opened({
      ...OPEN,
      messages: [
        { ...said("$a:example.org", "Consort"), thread: { count: 2, participated: false } },
      ],
    });

    // The count, not the composer's own Reply, which is how anything gets
    // said in here.
    expect(screen.queryByRole("button", { name: /\d+ repl/i })).toBeNull();
  });
});

describe("what the panel is called", () => {
  it("is named after the message the thread hangs from", async () => {
    // "Thread" is what this said, which is true of every thread and so says
    // nothing about the one somebody is reading.
    await opened();

    expect(
      screen.getByRole("heading", { name: "what shall we call it" }),
    ).toBeVisible();
  });

  it("says what was written rather than how it was written", async () => {
    await opened({
      ...OPEN,
      root: {
        ...said("$root:example.org", "**ship it** on [friday](https://example.org)"),
        html: '<p><strong>ship it</strong> on <a href="https://example.org">friday</a></p>',
      },
    });

    expect(screen.getByRole("heading", { name: "ship it on friday" })).toBeVisible();
  });

  it("goes back to the plain word when the root could not be fetched", async () => {
    // A redaction and a missing key both look like this, and a blank head
    // above a column of replies reads as a panel that failed to load.
    const { root: _root, ...rootless } = OPEN;
    await opened(rootless);

    expect(screen.getByRole("heading", { name: "Thread" })).toBeVisible();
  });

  it("reads out a message that is nothing but a custom emoji", async () => {
    // There are no words in an `img`, and the shortcode the sender typed is
    // what somebody would say out loud.
    await opened({
      ...OPEN,
      root: {
        ...said("$root:example.org", ":shipit:"),
        html: '<img data-mx-emoticon src="mxc://example.org/ship" alt=":shipit:">',
      },
    });

    expect(screen.getByRole("heading", { name: ":shipit:" })).toBeVisible();
  });

  it("names an attachment nobody captioned", async () => {
    await opened({ ...OPEN, root: picture("$root:example.org") });

    expect(screen.getByRole("heading", { name: "screenshot.png" })).toBeVisible();
  });
});

describe("the panel's width", () => {
  it("is drawn at whatever it was given", async () => {
    const { container } = render(
      <ThreadPanel
        selfId={ADA}
        onOpenRoom={vi.fn()}
      onOpen={vi.fn()}
        width={480}
        onResize={vi.fn()}
      />,
    );
    await waitFor(() => expect(onThread).toHaveBeenCalled());
    await act(async () => {
      publish(OPEN);
    });

    expect(container.querySelector(".thread")).toHaveStyle({ width: "480px" });
  });

  it("widens when the grip is dragged towards the conversation", async () => {
    await opened();

    const grip = screen.getByRole("separator", { name: /resize/i });
    fireEvent.pointerDown(grip, { clientX: 900 });
    fireEvent.pointerMove(window, { clientX: 800 });

    // Dragging left takes width from the room and gives it to the panel.
    expect(resized).toHaveBeenLastCalledWith(500);
  });

  it("stops listening once the pointer is let go", async () => {
    await opened();

    const grip = screen.getByRole("separator", { name: /resize/i });
    fireEvent.pointerDown(grip, { clientX: 900 });
    fireEvent.pointerUp(window, { clientX: 900 });
    resized.mockClear();
    fireEvent.pointerMove(window, { clientX: 700 });

    expect(resized).not.toHaveBeenCalled();
  });

  it("moves with the arrow keys, so a mouse is not the only way", async () => {
    await opened();
    const grip = screen.getByRole("separator", { name: /resize/i });
    grip.focus();

    await userEvent.keyboard("{ArrowLeft}");
    expect(resized).toHaveBeenLastCalledWith(416);

    await userEvent.keyboard("{ArrowRight}");
    expect(resized).toHaveBeenLastCalledWith(384);
  });

  it("refuses to be dragged narrower than a conversation reads at", async () => {
    await opened();

    const grip = screen.getByRole("separator", { name: /resize/i });
    fireEvent.pointerDown(grip, { clientX: 900 });
    fireEvent.pointerMove(window, { clientX: 2_000 });

    expect(resized).toHaveBeenLastCalledWith(300);
  });
});

describe("where the panel opens", () => {
  it("opens at the newest reply rather than the oldest one loaded", async () => {
    // A panel that opened at the top put somebody at the start of a
    // conversation they pressed a reply count to see the end of.
    fakeScrolling(900, 300);

    const { container } = draw();
    await waitFor(() => expect(onThread).toHaveBeenCalled());
    await act(async () => {
      publish(OPEN);
    });

    expect(container.querySelector(".thread__scroll")?.scrollTop).toBe(600);
  });

  it("leaves a reader who has scrolled up where they are", async () => {
    fakeScrolling(900, 300);
    const { container } = draw();
    await waitFor(() => expect(onThread).toHaveBeenCalled());
    await act(async () => {
      publish(OPEN);
    });

    const box = container.querySelector(".thread__scroll");
    if (box === null) throw new Error("the panel drew no scrolling box");
    box.scrollTop = 100;
    fireEvent.scroll(box);
    await act(async () => {
      publish({
        ...OPEN,
        messages: [...OPEN.messages, said("$b:example.org", "seconded", NOON + 2_000)],
      });
    });

    expect(box.scrollTop).toBe(100);
  });

  it("stays at the bottom when a picture finishes loading", async () => {
    // Same fault as the room beside it, and worth its own test because the
    // panel keeps its own scroller and its own idea of following.
    const layout = fakeScrolling(900, 300);
    const { container } = draw();
    await waitFor(() => expect(onThread).toHaveBeenCalled());
    await act(async () => {
      publish({ ...OPEN, messages: [picture("$a:example.org")] });
    });

    const box = container.querySelector(".thread__scroll");
    if (box === null) throw new Error("the panel drew no scrolling box");
    expect(box.scrollTop).toBe(600);

    layout.scrollHeight = 1_500;
    await act(async () => {
      fireEvent.load(screen.getByRole("img", { name: "screenshot.png" }));
    });

    expect(box.scrollTop).toBe(1_200);
  });

  it("opens a second thread at its own bottom", async () => {
    // Following is per conversation. Carrying the last one's position over
    // would open a thread halfway up for no reason anybody could see.
    fakeScrolling(900, 300);
    const { container } = draw();
    await waitFor(() => expect(onThread).toHaveBeenCalled());
    await act(async () => {
      publish(OPEN);
    });

    const box = container.querySelector(".thread__scroll");
    if (box === null) throw new Error("the panel drew no scrolling box");
    box.scrollTop = 100;
    fireEvent.scroll(box);
    await act(async () => {
      publish({
        ...OPEN,
        rootId: "$other:example.org",
        root: said("$other:example.org", "and the icon"),
      });
    });

    expect(box.scrollTop).toBe(600);
  });
});

describe("copying a reply's address", () => {
  it("asks about the room the thread is in", async () => {
    // A reply in a thread has an address like anything else said in the room,
    // and somebody reading a long thread wants to link to one line of it.
    await opened();

    const [first] = screen.getAllByRole("button", { name: "Copy link" });
    await userEvent.click(first!);

    expect(timelineCopyLink).toHaveBeenCalledWith(GENERAL, "$root:example.org");
  });

  it("says it worked", async () => {
    await opened();

    const [first] = screen.getAllByRole("button", { name: "Copy link" });
    await userEvent.click(first!);

    expect(
      await screen.findByRole("button", { name: "Link copied" }),
    ).toBeVisible();
  });

  it("says so rather than claiming a copy that did not happen", async () => {
    timelineCopyLink.mockRejectedValue({ message: "no clipboard", detail: "no" });
    await opened();

    const [first] = screen.getAllByRole("button", { name: "Copy link" });
    await userEvent.click(first!);

    expect(await screen.findByRole("alert")).toHaveTextContent("no clipboard");
  });
});
