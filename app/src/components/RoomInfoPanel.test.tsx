import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const roomCopyLink = vi.hoisted(() => vi.fn());
// The panel draws the room's picture, which asks for bytes of its own.
const roomAvatar = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  roomCopyLink,
  roomAvatar,
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
