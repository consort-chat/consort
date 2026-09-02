import { act, render, screen, waitFor, within } from "@testing-library/react";
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

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  onTimeline,
  timelineOpen,
  timelineClose,
  timelineEarlier,
  timelineSend,
  memberNames,
  memberAvatar,
  resendState,
  audioSettings,
  setPersonVolume,
}));

import { RoomTimeline, group } from "./RoomTimeline";
import { resetAvatarCache } from "../lib/avatars";
import type { Channel, Message, Timeline } from "../lib/api";

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

beforeEach(() => {
  resetAvatarCache();
  publish = () => {};
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
});

/** Render the pane and hand back a way to publish into it. */
async function pane(channel: Channel = general) {
  render(<RoomTimeline channel={channel} />);
  await waitFor(() => expect(timelineOpen).toHaveBeenCalled());
}

/** Publish `next` and let React settle. */
async function arrive(next: Timeline) {
  await act(async () => {
    publish(next);
  });
}

describe("RoomTimeline grouping", () => {
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
    const { unmount } = render(<RoomTimeline channel={general} />);
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
    const { unmount } = render(<RoomTimeline channel={general} />);
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("#general");
    unmount();

    render(<RoomTimeline channel={lounge} />);

    const heading = screen.getByRole("heading", { level: 1 });
    expect(heading).toHaveTextContent("Lounge");
    expect(heading).not.toHaveTextContent("#");
  });

  it("offers older messages only when there are some", async () => {
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")]));
    expect(screen.queryByRole("button", { name: /older/i })).toBeNull();

    await arrive(timeline([said("$1", ADA, "hello")], { moreBefore: true }));
    expect(screen.getByRole("button", { name: /older/i })).toBeVisible();
  });

  it("asks for older messages when the control is pressed", async () => {
    await pane();
    await arrive(timeline([said("$1", ADA, "hello")], { moreBefore: true }));

    await userEvent.click(screen.getByRole("button", { name: /older/i }));

    expect(timelineEarlier).toHaveBeenCalled();
  });

  it("says a page is on its way rather than looking like nothing happened", async () => {
    await pane();

    await arrive(timeline([said("$1", ADA, "hello")], { moreBefore: true, loading: true }));

    expect(screen.getByRole("button", { name: /loading/i })).toBeDisabled();
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
