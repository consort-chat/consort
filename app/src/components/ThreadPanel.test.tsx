import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const onThread = vi.hoisted(() => vi.fn());
const threadOpen = vi.hoisted(() => vi.fn());
const threadSend = vi.hoisted(() => vi.fn());
const resendState = vi.hoisted(() => vi.fn());
const memberNames = vi.hoisted(() => vi.fn());
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
      width={400}
      onResize={resized}
    />,
  );
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

    expect(screen.getByText("what shall we call it")).toBeVisible();
    expect(screen.getByText("Consort")).toBeVisible();
  });

  it("shuts when the close control is pressed", async () => {
    await opened();

    await userEvent.click(screen.getByRole("button", { name: /close thread/i }));

    expect(threadOpen).toHaveBeenCalledWith(null);
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
    await userEvent.click(screen.getByRole("button", { name: "Reply" }));

    await waitFor(() =>
      expect(threadSend).toHaveBeenCalledWith(
        GENERAL,
        "$root:example.org",
        "$a:example.org",
        "Consort",
      ),
    );
  });

  it("answers the root itself when nothing has been replied yet", async () => {
    // The fallback has to point at something, and in an empty thread the only
    // thing said so far is the message it hangs from.
    await opened({ ...OPEN, messages: [] });

    await userEvent.type(screen.getByRole("textbox"), "first");
    await userEvent.click(screen.getByRole("button", { name: "Reply" }));

    await waitFor(() =>
      expect(threadSend).toHaveBeenCalledWith(
        GENERAL,
        "$root:example.org",
        "$root:example.org",
        "first",
      ),
    );
  });

  it("keeps what was typed when the send failed", async () => {
    // Retyping a message is the one thing an interface must never ask for.
    threadSend.mockRejectedValue({ message: "The homeserver refused that." });
    await opened();

    await userEvent.type(screen.getByRole("textbox"), "Consort");
    await userEvent.click(screen.getByRole("button", { name: "Reply" }));

    expect(
      await screen.findByText("The homeserver refused that."),
    ).toBeVisible();
    expect(screen.getByRole("textbox")).toHaveValue("Consort");
  });

  it("empties the box once the homeserver has it", async () => {
    await opened();

    await userEvent.type(screen.getByRole("textbox"), "Consort");
    await userEvent.click(screen.getByRole("button", { name: "Reply" }));

    await waitFor(() => expect(screen.getByRole("textbox")).toHaveValue(""));
  });

  it("sends nothing when nothing has been typed", async () => {
    await opened();

    await userEvent.click(screen.getByRole("button", { name: "Reply" }));

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

describe("the panel's width", () => {
  it("is drawn at whatever it was given", async () => {
    const { container } = render(
      <ThreadPanel
        selfId={ADA}
        onOpenRoom={vi.fn()}
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
