import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const roomCopyLink = vi.hoisted(() => vi.fn());
const roomLeave = vi.hoisted(() => vi.fn());
const roomInvite = vi.hoisted(() => vi.fn());
const roomCanInvite = vi.hoisted(() => vi.fn());
// The panel draws the room's picture, which asks for bytes of its own.
const roomAvatar = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  roomCopyLink,
  roomAvatar,
  roomLeave,
  roomInvite,
  roomCanInvite,
}));

import { RoomInfoPanel } from "./RoomInfoPanel";
import { resetAvatarCache } from "../lib/avatars";
import type { Channel } from "../lib/api";

const GENERAL = "!general:example.org";

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

describe("RoomInfoPanel", () => {
  beforeEach(() => {
    resetAvatarCache();
    roomAvatar.mockReset().mockResolvedValue(null);
    roomCopyLink.mockReset().mockResolvedValue(undefined);
    roomLeave.mockReset().mockResolvedValue(undefined);
    roomInvite.mockReset().mockResolvedValue(undefined);
    roomCanInvite.mockReset().mockResolvedValue(true);
  });

  it("names the room the way the heading above it does", () => {
    // The hash included. A panel saying `general` under a heading saying
    // `#general` reads as two rooms.
    render(<RoomInfoPanel channel={channel()} onClose={vi.fn()} />);

    expect(screen.getByText("#general")).toBeVisible();
  });

  it("leaves the hash off a voice channel, as the heading does", () => {
    render(
      <RoomInfoPanel
        channel={channel({ name: "Lounge", kind: "voice" })}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("Lounge")).toBeVisible();
  });

  it("draws the address the room published", () => {
    render(
      <RoomInfoPanel
        channel={channel({ alias: "#general:example.org" })}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("#general:example.org")).toBeVisible();
  });

  it("draws no address for a room that publishes none", () => {
    // Most private rooms. An absence is not a failure and gets no sentence.
    const { container } = render(
      <RoomInfoPanel channel={channel()} onClose={vi.fn()} />,
    );

    expect(container.querySelector(".info__alias")).toBeNull();
  });

  it("draws the whole topic rather than the header's one line", () => {
    // The reason the panel exists. The header truncates, and a topic with two
    // paragraphs in it is a topic nobody could read until now.
    const topic = "Where the good links go.\nAnd the bad ones too.";
    const { container } = render(
      <RoomInfoPanel channel={channel({ topic })} onClose={vi.fn()} />,
    );

    // Compared against the whole string rather than matched loosely, because
    // the line break somebody put in a topic is part of what they wrote.
    expect(container.querySelector(".info__topic")?.textContent).toBe(topic);
  });

  it("says so when the room has not set a topic", () => {
    // Rather than an empty section, which reads as something that failed to
    // load.
    render(<RoomInfoPanel channel={channel()} onClose={vi.fn()} />);

    expect(screen.getByText("This room has not set a topic.")).toBeVisible();
  });

  it("puts the room's address on the clipboard", async () => {
    render(<RoomInfoPanel channel={channel()} onClose={vi.fn()} />);

    await userEvent.click(screen.getByRole("button", { name: "Copy room link" }));

    expect(roomCopyLink).toHaveBeenCalledWith(GENERAL);
  });

  it("says the copy worked, and stops saying it", async () => {
    // A copy is silent otherwise, and a control that says nothing invites a
    // second press.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    render(<RoomInfoPanel channel={channel()} onClose={vi.fn()} />);

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
    render(<RoomInfoPanel channel={channel()} onClose={vi.fn()} />);

    await userEvent.click(screen.getByRole("button", { name: "Copy room link" }));

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "could not reach this desktop's clipboard",
      ),
    );
  });

  it("closes when asked", async () => {
    const onClose = vi.fn();
    render(<RoomInfoPanel channel={channel()} onClose={onClose} />);

    await userEvent.click(screen.getByRole("button", { name: "Close room info" }));

    expect(onClose).toHaveBeenCalled();
  });

  it("closes on Escape, the way everything else here is dismissed", async () => {
    const onClose = vi.fn();
    render(<RoomInfoPanel channel={channel()} onClose={onClose} />);

    await userEvent.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalled();
  });

  it("stops answering the key once it is gone", async () => {
    // A listener at the window outlives the panel if nothing takes it off,
    // and the shell would then close a panel that is already closed on every
    // later press.
    const onClose = vi.fn();
    const { unmount } = render(
      <RoomInfoPanel channel={channel()} onClose={onClose} />,
    );

    unmount();
    await userEvent.keyboard("{Escape}");

    expect(onClose).not.toHaveBeenCalled();
  });
});

/*
  The two things this panel does that change who is in a room.

  Their own block because the ones above are about drawing what the shell
  already holds, and nothing above can fail in a way somebody has to be told
  about. These both can.
*/
describe("leaving a room", () => {
  beforeEach(() => {
    resetAvatarCache();
    roomAvatar.mockReset().mockResolvedValue(null);
    roomCopyLink.mockReset().mockResolvedValue(undefined);
    roomLeave.mockReset().mockResolvedValue(undefined);
    roomInvite.mockReset().mockResolvedValue(undefined);
    roomCanInvite.mockReset().mockResolvedValue(true);
  });

  /** Draw the panel and hand back the control that starts a leave. */
  async function leaveControl() {
    render(<RoomInfoPanel channel={channel()} onClose={vi.fn()} />);
    return screen.getByRole("button", { name: "Leave room" });
  }

  it("does not leave on the first press", async () => {
    // The rule the whole control is shaped by. Leaving is not reversible from
    // here: an invite-only room left by mistake needs somebody still in it to
    // ask you back.
    await userEvent.click(await leaveControl());

    expect(roomLeave).not.toHaveBeenCalled();
    expect(
      screen.getByRole("dialog", { name: "Leave #general?" }),
    ).toBeVisible();
  });

  it("says what leaving costs before it happens", async () => {
    await userEvent.click(await leaveControl());

    expect(
      screen.getByText(/need an invitation to come back/),
    ).toBeVisible();
  });

  it("puts the focus on the half that does nothing", async () => {
    // The same rule one layer down. Enter is the key somebody presses without
    // reading, and it must not land on the irreversible answer.
    await userEvent.click(await leaveControl());

    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });

  it("leaves on the second press", async () => {
    await userEvent.click(await leaveControl());

    await userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Leave" }),
    );

    expect(roomLeave).toHaveBeenCalledWith(GENERAL);
  });

  it("leaves the room joined when the question is answered no", async () => {
    // The test the issue asked for by name. A refused confirmation has to
    // leave the account exactly where it was.
    await userEvent.click(await leaveControl());

    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(roomLeave).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("leaves the room joined when the question is dismissed with Escape", async () => {
    await userEvent.click(await leaveControl());

    await userEvent.keyboard("{Escape}");

    expect(roomLeave).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("keeps the panel open when Escape answers the question", async () => {
    // One press, one thing. The panel closes on Escape too, and a single
    // press that shut both would take the room's details away as the price of
    // changing your mind about leaving it.
    const onClose = vi.fn();
    render(<RoomInfoPanel channel={channel()} onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "Leave room" }));

    await userEvent.keyboard("{Escape}");

    expect(onClose).not.toHaveBeenCalled();
  });

  it("says why a leave did not work", async () => {
    roomLeave.mockRejectedValue({
      message: "The homeserver could not complete that request.",
      detail: "500",
    });
    await userEvent.click(await leaveControl());

    await userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Leave" }),
    );

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "could not complete that request",
      ),
    );
  });

  it("names the room in the question rather than asking about a room", async () => {
    // Two panels apart is one click, and a question that did not say which
    // room would be answered about the wrong one.
    render(
      <RoomInfoPanel
        channel={channel({ name: "Lounge", kind: "voice" })}
        onClose={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Leave room" }));

    expect(screen.getByRole("dialog", { name: "Leave Lounge?" })).toBeVisible();
  });
});

describe("inviting somebody", () => {
  beforeEach(() => {
    resetAvatarCache();
    roomAvatar.mockReset().mockResolvedValue(null);
    roomCopyLink.mockReset().mockResolvedValue(undefined);
    roomLeave.mockReset().mockResolvedValue(undefined);
    roomInvite.mockReset().mockResolvedValue(undefined);
    roomCanInvite.mockReset().mockResolvedValue(true);
  });

  /** Draw the panel, without waiting for the permission answer. */
  function draw() {
    render(<RoomInfoPanel channel={channel()} onClose={vi.fn()} />);
    return screen.getByRole("button", { name: "Invite" });
  }

  /** The same, waiting until the room has said it will take one. */
  async function ready() {
    const invite = draw();
    await waitFor(() => expect(invite).toBeEnabled());
    return invite;
  }

  it("asks whether this account may invite into this room", async () => {
    draw();

    await waitFor(() => expect(roomCanInvite).toHaveBeenCalledWith(GENERAL));
  });

  it("offers the control once the room says it will take one", async () => {
    const invite = draw();

    await waitFor(() => expect(invite).toBeEnabled());
  });

  it("offers nothing to press until the answer is back", async () => {
    // Disabled rather than enabled-and-then-refused. Nothing here knows yet,
    // and a control that looked ready would be answering a question that has
    // not come back.
    roomCanInvite.mockReturnValue(new Promise(() => {}));

    expect(draw()).toBeDisabled();
  });

  it("draws the control disabled with a reason when the room will not take one", async () => {
    // The decision this issue left open, made this way round on purpose. A
    // control that is absent reads as something Consort cannot do; this is
    // something this room will not let this account do, and only the second
    // has an answer worth putting on screen.
    roomCanInvite.mockResolvedValue(false);
    const invite = draw();

    await waitFor(() => expect(invite).toBeDisabled());
    expect(
      screen.getByText("You do not have permission to invite people here."),
    ).toBeVisible();
  });

  it("says why when the room itself has gone", async () => {
    // Rather than the permission sentence, which would be the wrong reason on
    // screen. A room this account is not in is not a room that said no.
    roomCanInvite.mockRejectedValue({
      message: "That room is not one this account is in.",
      detail: "NoSuchRoom",
    });
    draw();

    await waitFor(() =>
      expect(
        screen.getByText("That room is not one this account is in."),
      ).toBeVisible(),
    );
  });

  it("asks for the person by user ID", async () => {
    // A plain field rather than a directory search, which is a larger thing
    // and its own issue.
    await ready();

    await userEvent.type(
      screen.getByRole("textbox", { name: "Matrix user ID" }),
      "@ada:example.org",
    );
    await userEvent.click(screen.getByRole("button", { name: "Invite" }));

    expect(roomInvite).toHaveBeenCalledWith(GENERAL, "@ada:example.org");
  });

  it("sends on Enter, so the field can be typed and left", async () => {
    await ready();

    await userEvent.type(
      screen.getByRole("textbox", { name: "Matrix user ID" }),
      "@ada:example.org{Enter}",
    );

    expect(roomInvite).toHaveBeenCalledWith(GENERAL, "@ada:example.org");
  });

  it("sends nothing when the box is empty", async () => {
    await ready();

    await userEvent.click(screen.getByRole("button", { name: "Invite" }));

    expect(roomInvite).not.toHaveBeenCalled();
  });

  it("says the invitation went, and names who it went to", async () => {
    // An invitation is silent otherwise, and a control that says nothing
    // invites a second press that comes back "already invited".
    await ready();

    await userEvent.type(
      screen.getByRole("textbox", { name: "Matrix user ID" }),
      "@ada:example.org{Enter}",
    );

    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("@ada:example.org"),
    );
  });

  it("empties the box once the invitation has gone", async () => {
    // So the next one is typed rather than edited, and so a second Enter does
    // not send the same invitation again.
    await ready();
    const box = screen.getByRole("textbox", { name: "Matrix user ID" });

    await userEvent.type(box, "@ada:example.org{Enter}");

    await waitFor(() => expect(box).toHaveValue(""));
  });

  it("says why an invitation did not go, in the words Rust chose", async () => {
    // The whole point of five sentences rather than one. "That did not work"
    // is useless when the reason is that somebody here banned them.
    roomInvite.mockRejectedValue({
      message: "They are banned from this room.",
      detail: "BannedFromRoom",
    });
    await ready();

    await userEvent.type(
      screen.getByRole("textbox", { name: "Matrix user ID" }),
      "@ada:example.org{Enter}",
    );

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "They are banned from this room.",
      ),
    );
  });

  it("keeps what was typed when the invitation is refused", async () => {
    // A user ID is a long thing to type twice, and the likeliest fix is one
    // character of it.
    roomInvite.mockRejectedValue({ message: "No.", detail: "" });
    await ready();
    const box = screen.getByRole("textbox", { name: "Matrix user ID" });

    await userEvent.type(box, "@ada:exmaple.org{Enter}");

    await waitFor(() => expect(screen.getByRole("alert")).toBeVisible());
    expect(box).toHaveValue("@ada:exmaple.org");
  });

  it("asks again when the panel is pointed at another room", async () => {
    // The answer is per room. Carrying the last room's permission across
    // would draw the control enabled in a room that will refuse it.
    const { rerender } = render(
      <RoomInfoPanel channel={channel()} onClose={vi.fn()} />,
    );
    await waitFor(() => expect(roomCanInvite).toHaveBeenCalledWith(GENERAL));
    roomCanInvite.mockResolvedValue(false);

    rerender(
      <RoomInfoPanel
        channel={channel({ id: "!other:example.org", name: "other" })}
        onClose={vi.fn()}
      />,
    );

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Invite" })).toBeDisabled(),
    );
  });
});
