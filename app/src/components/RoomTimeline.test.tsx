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

const onTimeline = vi.hoisted(() => vi.fn());
const timelineOpen = vi.hoisted(() => vi.fn());
const timelineClose = vi.hoisted(() => vi.fn());
const timelineEarlier = vi.hoisted(() => vi.fn());
const timelineSend = vi.hoisted(() => vi.fn());
const memberNames = vi.hoisted(() => vi.fn());
const memberAvatar = vi.hoisted(() => vi.fn());
const resendState = vi.hoisted(() => vi.fn());
// For the card a name opens, which reads its own saved volume.
const audioSettings = vi.hoisted(() => vi.fn());
const setPersonVolume = vi.hoisted(() => vi.fn());
// For the dot on a sender's picture, and for the card, which asks the same
// thing when it opens.
const memberProfile = vi.hoisted(() => vi.fn());
// The pane watches the thread channel as well, so that a pressed reply count
// can stop turning when the panel it asked for is actually there.
const onThread = vi.hoisted(() => vi.fn());
const threadOpen = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  onTimeline,
  onThread,
  threadOpen,
  timelineOpen,
  timelineClose,
  timelineEarlier,
  timelineSend,
  memberNames,
  memberAvatar,
  resendState,
  audioSettings,
  setPersonVolume,
  memberProfile,
}));

import { RoomTimeline } from "./RoomTimeline";
import { fakeScrolling } from "../test/scrolling";
import { resetAvatarCache } from "../lib/avatars";
import { resetPresenceCache } from "../lib/presence";
import type { Channel, Message, Thread, Timeline } from "../lib/api";

const GENERAL = "!general:example.org";
const ADA = "@ada:example.org";
const BOB = "@bob:example.org";

const general: Channel = {
  id: GENERAL,
  name: "general",
  kind: "text",
  avatar: null,
  joined: true,
  participants: [],
};

const lounge: Channel = { ...general, id: "!lounge:example.org", name: "Lounge", kind: "voice" };

/** One minute past midnight, so the clock time is stable wherever this runs. */
const NOON = Date.UTC(2026, 0, 1, 12, 0, 0);

/** The clock time the component draws, formatted the way it formats it. */
function timeOf(at: number): string {
  return new Date(at).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function said(id: string, sender: string, body: string, at = NOON): Message {
  return { id, sender, body, at, kind: "text" };
}

function timeline(messages: Message[], rest: Partial<Timeline> = {}): Timeline {
  return {
    roomId: GENERAL,
    messages,
    moreBefore: false,
    loading: false,
    ...rest,
  };
}

/** Whatever the component subscribed with, so a test can publish to it. */
let publish: (timeline: Timeline) => void;
/** The same, for the thread channel. */
let publishThread: (thread: Thread | null) => void;

beforeEach(() => {
  resetAvatarCache();
  resetPresenceCache();
  publish = () => {};
  publishThread = () => {};
  onThread.mockReset().mockImplementation((handler: (t: Thread | null) => void) => {
    publishThread = handler;
    return Promise.resolve(() => {});
  });
  threadOpen.mockReset().mockResolvedValue(undefined);
  onTimeline.mockReset().mockImplementation((handler: (t: Timeline) => void) => {
    publish = handler;
    return Promise.resolve(() => {});
  });
  timelineOpen.mockReset().mockResolvedValue(undefined);
  timelineClose.mockReset().mockResolvedValue(undefined);
  timelineEarlier.mockReset().mockResolvedValue(undefined);
  timelineSend.mockReset().mockResolvedValue(undefined);
  memberNames.mockReset().mockResolvedValue({ [ADA]: "Ada", [BOB]: "Bob" });
  memberAvatar.mockReset().mockResolvedValue(null);
  resendState.mockReset().mockResolvedValue(undefined);
  audioSettings.mockReset().mockResolvedValue({
    input: null,
    output: null,
    gate: { open: -40, close: -50, hold: 200 },
    personVolumes: {},
  });
  setPersonVolume.mockReset().mockResolvedValue(undefined);
  memberProfile.mockReset().mockResolvedValue({
    presence: "online",
    status: null,
    lastActiveAgo: null,
    standing: "member",
  });
});

/** Render the pane and hand back a way to publish into it. */
async function pane(channel: Channel = general) {
  render(<RoomTimeline selfId="@bob:example.org" onOpenRoom={vi.fn()} channel={channel} />);
  await waitFor(() => expect(timelineOpen).toHaveBeenCalled());
}

/** Put the reader at `top` and let the pane notice. */
async function scrollTo(top: number) {
  const box = screen.getByRole("log");
  await act(async () => {
    box.scrollTop = top;
    fireEvent.scroll(box);
  });
}

/** Publish `next` and let React settle. */
async function arrive(next: Timeline) {
  await act(async () => {
    publish(next);
  });
}

describe("RoomTimeline", () => {
  it("opens the room it was given", async () => {
    await pane();

    expect(timelineOpen).toHaveBeenCalledWith(GENERAL);
  });

  it("asks to be caught up, for the room that was already open", async () => {
    // Opening the room already open deliberately publishes nothing, so that a
    // re-selection does not throw away everything scrolled back through. This
    // is how a remount gets the list anyway.
    await pane();

    await waitFor(() => expect(resendState).toHaveBeenCalled());
  });

  it("closes the room when it goes", async () => {
    const { unmount } = render(<RoomTimeline selfId="@bob:example.org" onOpenRoom={vi.fn()} channel={general} />);
    await waitFor(() => expect(timelineOpen).toHaveBeenCalled());

    unmount();

    await waitFor(() => expect(timelineClose).toHaveBeenCalled());
  });

  it("draws messages in the order they were said", async () => {
    await pane();

    await arrive(timeline([said("$1", ADA, "first"), said("$2", BOB, "second")]));

    const bodies = screen.getAllByText(/first|second/);
    expect(bodies.map((one) => one.textContent)).toEqual(["first", "second"]);
  });

  it("draws the formatting a message was sent with", async () => {
    // What was wrong before markdown: somebody typing a heading was shown
    // their own hashes back.
    await pane();

    await arrive(
      timeline([
        {
          ...said("$1", ADA, "### Heading"),
          html: "<h3>Heading</h3>",
        },
      ]),
    );

    expect(
      await screen.findByRole("heading", { name: "Heading" }),
    ).toBeVisible();
    expect(screen.queryByText("### Heading")).toBeNull();
  });

  it("draws the plain text of a message nobody formatted", async () => {
    // Most messages, and the reason the plain body is still the fallback: a
    // sentence with an asterisk in it is a sentence.
    await pane();

    await arrive(timeline([said("$1", ADA, "2 * 3 * 4")]));

    expect(await screen.findByText("2 * 3 * 4")).toBeVisible();
  });

  it("lets the words of a message be selected", async () => {
    // The shell turns selection off, because dragging across the chrome of a
    // desktop application is never what somebody meant. A message is the one
    // thing in the room a reader does mean to select, and opting back in is
    // also what puts a text cursor over it instead of an arrow.
    await pane();

    await arrive(timeline([said("$1", ADA, "worth quoting")]));

    expect(await screen.findByText("worth quoting")).toHaveAttribute(
      "data-selectable",
    );
  });

  it("does not tooltip a date over the words of a message", async () => {
    // A tooltip that follows the pointer across every sentence in a room is
    // noise, and it appears over the one thing somebody is trying to read.
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")]));

    expect(await screen.findByText("hello")).not.toHaveAttribute("title");
  });

  it("puts the whole date on the time beside the name", async () => {
    // Where a date belongs, and where it is out of the way of the words. The
    // clock time is already there, so hovering it asks about the date.
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")]));

    const at = await screen.findByText(timeOf(NOON));
    expect(at).toHaveAttribute("title", new Date(NOON).toLocaleString());
  });

  it("names the sender rather than printing their user ID", async () => {
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")]));

    expect(await screen.findByText("Ada")).toBeVisible();
    expect(screen.queryByText(ADA)).toBeNull();
  });

  it("falls back to the user ID when the room has never heard of them", async () => {
    // Their `m.room.member` has not arrived. A user ID is still something a
    // person recognises, which an empty byline is not.
    memberNames.mockResolvedValue({});
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")]));

    expect(await screen.findByText(ADA)).toBeVisible();
  });

  it("says where a sender is, beside their picture", async () => {
    // Answering the question a byline raises and a name does not: is the
    // person who said this here now.
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")]));

    expect(await screen.findByRole("img", { name: "Online" })).toBeVisible();
  });

  it("draws a picture somebody sent rather than naming the file", async () => {
    // The whole of what an attachment is for. A line reading "screenshot.png"
    // is what somebody sent a screenshot to avoid.
    await pane();

    await arrive(
      timeline([
        {
          // Uncaptioned, which is what an image with no `filename` beside its
          // `body` becomes: the name is on the card, not above the picture.
          ...said("$1", ADA, ""),
          kind: "image",
          media: {
            source: '{"url":"mxc://example.org/abc"}',
            name: "screenshot.png",
            mime: "image/png",
            width: 800,
            height: 600,
          },
        },
      ]),
    );

    expect(
      await screen.findByRole("img", { name: "screenshot.png" }),
    ).toBeVisible();
    expect(screen.queryByText("screenshot.png")).toBeNull();
  });

  it("draws the words that came with an attachment", async () => {
    // What the Lampshade bot does with a link: it uploads the clip and puts
    // the quoted post in the same event as a caption. Drawing only the card
    // threw the post away.
    await pane();

    await arrive(
      timeline([
        {
          ...said("$1", ADA, "Bunts (@soylennial): watch it until the end"),
          kind: "video",
          media: {
            source: '{"url":"mxc://example.org/reel"}',
            name: "video.mp4",
            size: 4_000_000,
          },
        },
      ]),
    );

    expect(
      await screen.findByText("Bunts (@soylennial): watch it until the end"),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: /video\.mp4/ })).toBeVisible();
  });

  it("draws a file as a card naming it rather than as a line of apology", async () => {
    await pane();

    await arrive(
      timeline([
        {
          ...said("$1", ADA, ""),
          kind: "file",
          media: {
            source: '{"url":"mxc://example.org/sheet"}',
            name: "accounts.ods",
            size: 51_200,
          },
        },
      ]),
    );

    expect(await screen.findByText("accounts.ods")).toBeVisible();
    expect(screen.queryByText(/cannot show these yet/i)).toBeNull();
  });

  it("offers a clip rather than fetching every one in a room", async () => {
    await pane();

    await arrive(
      timeline([
        {
          ...said("$1", ADA, ""),
          kind: "video",
          media: {
            source: '{"url":"mxc://example.org/reel"}',
            name: "clip.mp4",
            size: 12_400_000,
          },
        },
      ]),
    );

    expect(await screen.findByRole("button", { name: /clip\.mp4/ })).toBeVisible();
  });

  it("says nothing about where a sender is when the homeserver will not", async () => {
    // The ordinary case, because presence is off on most homeservers. A grey
    // dot on somebody sitting right there would be worse than no dot.
    memberProfile.mockResolvedValue({
      presence: "unknown",
      status: null,
      lastActiveAgo: null,
      standing: "member",
    });
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")]));

    await screen.findByText("hello");
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("asks for every sender at once rather than one per message", async () => {
    await pane();

    await arrive(
      timeline([
        said("$1", ADA, "one"),
        said("$2", ADA, "two"),
        said("$3", BOB, "three"),
      ]),
    );

    await waitFor(() => expect(memberNames).toHaveBeenCalled());
    expect(memberNames).toHaveBeenCalledWith(GENERAL, [ADA, BOB]);
  });

  it("ignores a timeline for a room it is not showing", async () => {
    // The moment between two clicks. Drawing it would put the last room's
    // conversation under this room's name.
    await pane();

    await arrive({
      ...timeline([said("$1", ADA, "elsewhere")]),
      roomId: "!other:example.org",
    });

    expect(screen.queryByText("elsewhere")).toBeNull();
  });

  it("says an empty room is empty rather than looking broken", async () => {
    await pane();

    await arrive(timeline([]));

    expect(await screen.findByText(/nothing has been said/i)).toBeVisible();
  });

  it("draws an emote as an action", async () => {
    await pane();

    await arrive(
      timeline([{ id: "$1", sender: ADA, body: "waves", at: NOON, kind: "emote" }]),
    );

    expect(await screen.findByText("Ada waves")).toBeVisible();
  });

  it("draws a message it cannot decrypt rather than leaving a hole", async () => {
    // A gap that says nothing about itself cannot be told apart from a quiet
    // room, and the two are very different things to be looking at.
    await pane();

    await arrive(
      timeline([
        {
          id: "$1",
          sender: ADA,
          body: "Waiting for the key to this message.",
          at: NOON,
          kind: "undecryptable",
        },
      ]),
    );

    expect(await screen.findByText(/waiting for the key/i)).toBeVisible();
  });

  it("puts the hash on a text channel and not on a voice one", async () => {
    // The hash is the text channel's, and only the text channel's. It is how
    // every client anybody already uses says which of the two this is.
    const { unmount } = render(<RoomTimeline selfId="@bob:example.org" onOpenRoom={vi.fn()} channel={general} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("#general");
    unmount();

    render(<RoomTimeline selfId="@bob:example.org" onOpenRoom={vi.fn()} channel={lounge} />);

    const heading = screen.getByRole("heading", { level: 1 });
    expect(heading).toHaveTextContent("Lounge");
    expect(heading).not.toHaveTextContent("#");
  });

  it("puts the room's topic under its name", async () => {
    await pane({ ...general, topic: "Where the good links go" });

    expect(screen.getByText("Where the good links go")).toBeVisible();
  });

  it("draws no subtitle for a room with no topic", async () => {
    const { container } = render(<RoomTimeline selfId="@bob:example.org" onOpenRoom={vi.fn()} channel={general} />);
    await waitFor(() => expect(timelineOpen).toHaveBeenCalled());

    expect(container.querySelector(".timeline__topic")).toBeNull();
  });

  it("asks for the page above when the reader gets near the top", async () => {
    fakeScrolling(900, 300);
    await pane();
    await arrive(timeline([said("$1", ADA, "hello")], { moreBefore: true }));

    await scrollTo(50);

    expect(timelineEarlier).toHaveBeenCalled();
  });

  it("asks once, however many scroll events land before the page does", async () => {
    // The reason the ask is in an effect rather than in the scroll handler. A
    // scroll handler runs at frame rate and `loading` has to travel out to
    // Rust and back before it is true here, so a check against it alone lets
    // twenty asks through for one page.
    fakeScrolling(900, 300);
    await pane();
    await arrive(timeline([said("$1", ADA, "hello")], { moreBefore: true }));

    await scrollTo(50);
    await scrollTo(40);
    await scrollTo(30);

    expect(timelineEarlier).toHaveBeenCalledTimes(1);
  });

  it("asks for nothing at the start of the room", async () => {
    fakeScrolling(900, 300);
    await pane();
    await arrive(timeline([said("$1", ADA, "hello")]));

    await scrollTo(0);

    expect(timelineEarlier).not.toHaveBeenCalled();
  });

  it("asks on its own in a room too short to scroll", async () => {
    // Nothing to scroll means no scroll event, so a pane that only asked from
    // the handler would sit for ever on a room whose first page held five
    // messages and a hundred state events.
    fakeScrolling(300, 300);
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")], { moreBefore: true }));

    expect(timelineEarlier).toHaveBeenCalled();
  });

  it("keeps the reader on the message they were reading when a page lands", async () => {
    // A page arrives above them and moves everything down by its own height.
    // Holding scrollTop instead would leave them at the top of a page they
    // have not read, which is the whole reason nobody scrolls back twice.
    const layout = fakeScrolling(900, 300);
    await pane();
    await arrive(timeline([said("$2", ADA, "hello")], { moreBefore: true }));
    await scrollTo(400);

    layout.scrollHeight = 1_500;
    await arrive(
      timeline([said("$1", ADA, "earlier"), said("$2", ADA, "hello")], {
        moreBefore: true,
      }),
    );

    // 500 from the bottom before, and 500 from the bottom after.
    expect(screen.getByRole("log").scrollTop).toBe(1_000);
  });

  it("does not drag a reader down when a message lands at the bottom", async () => {
    // The other half of the same rule. An append and a prepend need opposite
    // anchors, and the oldest message drawn is the only thing that says which
    // of the two just happened.
    const layout = fakeScrolling(900, 300);
    await pane();
    await arrive(timeline([said("$1", ADA, "hello")], { moreBefore: true }));
    await scrollTo(400);

    layout.scrollHeight = 1_000;
    await arrive(
      timeline([said("$1", ADA, "hello"), said("$2", BOB, "hi")], {
        moreBefore: true,
      }),
    );

    expect(screen.getByRole("log").scrollTop).toBe(400);
  });

  it("says a page is on its way rather than looking like nothing happened", async () => {
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")], { moreBefore: true, loading: true }));

    expect(screen.getByText(/loading earlier messages/i)).toBeVisible();
  });

  it("sends what was typed", async () => {
    await pane();

    await userEvent.type(screen.getByRole("textbox"), "hello");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(timelineSend).toHaveBeenCalledWith(GENERAL, "hello");
  });

  it("sends on Enter", async () => {
    await pane();

    await userEvent.type(screen.getByRole("textbox"), "hello{Enter}");

    expect(timelineSend).toHaveBeenCalledWith(GENERAL, "hello");
  });

  it("breaks the line on Shift+Enter instead of sending", async () => {
    // Without the modifier check a paragraph is impossible to type.
    await pane();

    await userEvent.type(screen.getByRole("textbox"), "one{Shift>}{Enter}{/Shift}two");

    expect(timelineSend).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox")).toHaveValue("one\ntwo");
  });

  it("empties the box once the homeserver has it", async () => {
    await pane();

    await userEvent.type(screen.getByRole("textbox"), "hello{Enter}");

    await waitFor(() => expect(screen.getByRole("textbox")).toHaveValue(""));
  });

  it("keeps what was typed when the send fails, and says why", async () => {
    // Retyping a message is the one thing an interface must never ask for.
    timelineSend.mockRejectedValue({ message: "the homeserver said no", detail: "no" });
    await pane();

    await userEvent.type(screen.getByRole("textbox"), "hello{Enter}");

    expect(await screen.findByRole("alert")).toHaveTextContent(/homeserver said no/);
    expect(screen.getByRole("textbox")).toHaveValue("hello");
  });

  it("refuses to send nothing", async () => {
    await pane();

    await userEvent.type(screen.getByRole("textbox"), "   ");

    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    await userEvent.type(screen.getByRole("textbox"), "{Enter}");
    expect(timelineSend).not.toHaveBeenCalled();
  });

  it("opens a card about whoever is being read when their name is pressed", async () => {
    // The same card a name in a voice channel opens. Somebody reading a room
    // and wondering who just said something should not have to find them in
    // the sidebar first, and they may not be in a call to be found in.
    await pane();
    await arrive(timeline([said("$1", ADA, "hello")]));

    await userEvent.click(await screen.findByRole("button", { name: "Ada" }));

    expect(await screen.findByRole("dialog", { name: "Ada" })).toBeVisible();
  });

  it("opens it from the face as well as the name", async () => {
    // Two targets for one thing, because both are what a hand goes for and
    // the avatar is the larger of them.
    await pane();
    await arrive(timeline([said("$1", ADA, "hello")]));

    await userEvent.click(
      await screen.findByRole("button", { name: "Ada's picture" }),
    );

    expect(await screen.findByRole("dialog", { name: "Ada" })).toBeVisible();
  });

  it("opens the card for a sender the room has no name for", async () => {
    // Their user ID is what the byline draws, so it is also what the card is
    // about. A control that only works for people with a display name is a
    // control that fails on exactly the person somebody wanted to look up.
    memberNames.mockResolvedValue({});
    await pane();
    await arrive(timeline([said("$1", ADA, "hello")]));

    await userEvent.click(await screen.findByRole("button", { name: ADA }));

    expect(await screen.findByRole("dialog", { name: ADA })).toBeVisible();
  });

  it("draws the clock time beside a group", async () => {
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")]));

    const group = await screen.findByRole("article");
    expect(within(group).getByRole("time")).toBeVisible();
  });
});

describe("opening a thread", () => {
  const withThread = (): Message => ({
    ...said("$1", ADA, "the question"),
    thread: { count: 3, participated: false },
  });

  it("stops taking presses until the panel is there", async () => {
    // The command that opens one answers immediately: it is a message to the
    // room's watcher, and the panel appears when the watcher publishes. A
    // control that stayed pressable in between invites a second press at a
    // thread that is already on its way.
    await pane();
    await arrive(timeline([withThread()]));

    const pill = screen.getByRole("button", { name: /3 replies/i });
    await userEvent.click(pill);

    expect(threadOpen).toHaveBeenCalledWith("$1");
    expect(screen.getByRole("button", { name: /3 replies/i })).toBeDisabled();
  });

  it("takes presses again once one has arrived", async () => {
    await pane();
    await arrive(timeline([withThread()]));
    await userEvent.click(screen.getByRole("button", { name: /3 replies/i }));

    await act(async () => {
      publishThread({
        roomId: GENERAL,
        rootId: "$1",
        messages: [],
        moreBefore: false,
      });
    });

    expect(
      screen.getByRole("button", { name: /3 replies/i }),
    ).not.toBeDisabled();
  });

  it("takes presses again when the panel says there is nothing to show", async () => {
    // A thread that could not be read publishes nothing rather than nothing at
    // all, so the control cannot be left turning for ever.
    await pane();
    await arrive(timeline([withThread()]));
    await userEvent.click(screen.getByRole("button", { name: /3 replies/i }));

    await act(async () => {
      publishThread(null);
    });

    expect(
      screen.getByRole("button", { name: /3 replies/i }),
    ).not.toBeDisabled();
  });
});
