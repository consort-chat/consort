import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { CallPanel } from "./CallPanel";
import { HEARING, type Call, type SelfAudio } from "../lib/api";

const LOUNGE = "!lounge:example.org";

function panel(
  call: Call,
  channelName: string | null = "Lounge",
  selfAudio: SelfAudio = HEARING,
) {
  const onDisconnect = vi.fn();
  const onSetMuted = vi.fn();
  const onSetDeafened = vi.fn();
  const { container } = render(
    <CallPanel
      call={call}
      channelName={channelName}
      selfAudio={selfAudio}
      onDisconnect={onDisconnect}
      onSetMuted={onSetMuted}
      onSetDeafened={onSetDeafened}
    />,
  );
  return { container, onDisconnect, onSetMuted, onSetDeafened };
}

/** A call that is up, which is the only state the controls are drawn in. */
const CONNECTED: Call = {
  state: "connected",
  roomId: LOUNGE,
  participants: [],
  trouble: null,
};

describe("CallPanel", () => {
  it("names the channel and says the call is up", () => {
    panel({
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    });

    const group = screen.getByRole("group", { name: /voice connection/i });
    expect(group).toHaveTextContent(/voice connected/i);
    expect(group).toHaveTextContent("Lounge");
  });

  it("says it is still working while a join is in flight", () => {
    panel({ state: "connecting", roomId: LOUNGE });

    expect(
      screen.getByRole("group", { name: /voice connection/i }),
    ).toHaveTextContent(/connecting/i);
  });

  it("writes the state out rather than only colouring it", () => {
    // Mint against amber reinforces the label. Somebody who cannot tell the
    // two apart has to be able to read the answer.
    const { container } = panel({ state: "connecting", roomId: LOUNGE });

    expect(container.querySelector(".call-panel__state")).toHaveTextContent(
      /connecting/i,
    );
  });

  it("marks which state it is in for the stylesheet", () => {
    const { container } = panel({ state: "connecting", roomId: LOUNGE });

    expect(container.querySelector(".call-panel")).toHaveAttribute(
      "data-state",
      "connecting",
    );
  });

  it("draws nothing at all when there is no call", () => {
    const { container } = panel({ state: "disconnected" });

    expect(container).toBeEmptyDOMElement();
  });

  it("draws nothing for a join that failed", () => {
    // The failure belongs beside the channel that would not take it. There is
    // no connection here to put in a connection panel.
    const { container } = panel({
      state: "failed",
      roomId: LOUNGE,
      error: "no voice server",
    });

    expect(container).toBeEmptyDOMElement();
  });

  it("leaves the call when the disconnect control is used", async () => {
    const { onDisconnect } = panel({
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    });

    await userEvent.click(
      screen.getByRole("button", { name: /disconnect from voice/i }),
    );

    expect(onDisconnect).toHaveBeenCalledTimes(1);
  });

  it("says why a call cannot be heard", () => {
    // The quiet failure: the membership published, the roster is right, the
    // packets are arriving, and neither side can decrypt a word. Every other
    // thing on this strip says the call is working.
    panel({
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: "Somebody's audio cannot be read: their media key never arrived.",
    });

    expect(screen.getByRole("alert")).toHaveTextContent(
      "their media key never arrived",
    );
  });

  it("says nothing about trouble when there is none", () => {
    // The overwhelmingly common case. A permanent line of reassurance is a
    // line people learn to stop reading.
    panel({ state: "connected", roomId: LOUNGE, participants: [], trouble: null });

    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("still offers the way out of a call that cannot be heard", () => {
    // The most likely thing somebody does next.
    const { onDisconnect } = panel({
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: "Your audio could not be encrypted.",
    });

    screen.getByRole("button", { name: /disconnect from voice/i }).click();

    expect(onDisconnect).toHaveBeenCalledTimes(1);
  });

  it("gives the disconnect control a name that is not its glyph", () => {
    // It is an icon, and it is the only control here that ends something.
    panel({
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    });

    const leave = screen.getByRole("button", { name: /disconnect from voice/i });
    expect(leave).toHaveAttribute("title");
  });

  it("draws a placeholder rather than a room id when it cannot name the channel", () => {
    const { container } = panel({
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    }, null);

    expect(container).toHaveTextContent(/voice channel/i);
    expect(container).not.toHaveTextContent(LOUNGE);
  });

  it("offers mute and deafen beside the way out", () => {
    panel(CONNECTED);

    expect(screen.getByRole("button", { name: /mute microphone/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /deafen/i })).toBeVisible();
    expect(
      screen.getByRole("button", { name: /disconnect from voice/i }),
    ).toBeVisible();
  });

  it("asks to mute when the microphone is live", async () => {
    const { onSetMuted } = panel(CONNECTED);

    await userEvent.click(screen.getByRole("button", { name: /mute microphone/i }));

    expect(onSetMuted).toHaveBeenCalledWith(true);
  });

  it("asks to unmute when it is already muted", async () => {
    const { onSetMuted } = panel(CONNECTED, "Lounge", {
      muted: true,
      deafened: false,
    });

    await userEvent.click(screen.getByRole("button", { name: /mute microphone/i }));

    expect(onSetMuted).toHaveBeenCalledWith(false);
  });

  it("says which way each switch is set rather than leaving it to the glyph", () => {
    // The one thing on this strip a screen reader cannot get from the drawing.
    // Without it, somebody who has just pressed mute has no way to find out
    // whether it took.
    panel(CONNECTED, "Lounge", { muted: true, deafened: false });

    expect(screen.getByRole("button", { name: /mute microphone/i })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: /deafen/i })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("shows the microphone as off while deafened, without saying it was muted", async () => {
    // Deafening stops the microphone, so drawing it live would be a lie. The
    // mute button is still not the one that was pressed, which is why
    // undeafening hands the microphone back.
    const { onSetMuted } = panel(CONNECTED, "Lounge", {
      muted: false,
      deafened: true,
    });

    await userEvent.click(screen.getByRole("button", { name: /mute microphone/i }));

    expect(screen.getByRole("button", { name: /mute microphone/i })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      onSetMuted,
      "the microphone button still works while deafened, and what it asks \
       for is a mute, because that is the switch it is",
    ).toHaveBeenCalledWith(true);
  });

  it("asks to undeafen when it is already deafened", async () => {
    const { onSetDeafened } = panel(CONNECTED, "Lounge", {
      muted: false,
      deafened: true,
    });

    await userEvent.click(screen.getByRole("button", { name: /deafen/i }));

    expect(onSetDeafened).toHaveBeenCalledWith(false);
  });

  it("keeps each control named the same whichever way it is set", () => {
    // A button whose accessible name changes under the cursor is announced as
    // a new button, and somebody toggling one twice hears two different
    // controls rather than one they pressed twice. The tooltip is where the
    // wording is allowed to follow the state, because a pointer reads it fresh
    // every time.
    panel(CONNECTED, "Lounge", { muted: false, deafened: false });
    panel(CONNECTED, "Lounge", { muted: true, deafened: false });

    const [live, silenced] = screen.getAllByRole("button", {
      name: /mute microphone/i,
    });
    expect(live).toHaveAttribute("title", "Mute");
    expect(silenced).toHaveAttribute("title", "Unmute");
  });

  it("draws no controls at all when there is no call", () => {
    const { container } = panel({ state: "disconnected" });

    expect(container).toBeEmptyDOMElement();
  });
});
