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
  cardShown = true,
) {
  const onDisconnect = vi.fn();
  const onSetMuted = vi.fn();
  const onSetDeafened = vi.fn();
  const onSetAway = vi.fn();
  const onToggleCard = vi.fn();
  const { container } = render(
    <CallPanel
      call={call}
      channelName={channelName}
      selfAudio={selfAudio}
      cardShown={cardShown}
      onToggleCard={onToggleCard}
      onDisconnect={onDisconnect}
      onSetMuted={onSetMuted}
      onSetDeafened={onSetDeafened}
      onSetAway={onSetAway}
    />,
  );
  return {
    container,
    onDisconnect,
    onSetMuted,
    onSetDeafened,
    onSetAway,
    onToggleCard,
  };
}

/** The state line, which is also the way the call card comes back. */
function stateLine() {
  return screen.queryByRole("button", {
    name: /^(voice connected|connecting)$/i,
  });
}

/** The control that holds the two actions that are not in the row. */
function disclosure() {
  return screen.getByRole("button", { name: /more voice actions/i });
}

/** Open it, which is what every test of what is behind it starts with. */
async function openMore() {
  await userEvent.click(disclosure());
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

  it("keeps the microphone and the way out in the row itself", () => {
    // The two a hand reaches for without reading anything. Everything else
    // moved behind one control so the state line has a column to sit in.
    panel(CONNECTED);

    expect(screen.getByRole("button", { name: /mute microphone/i })).toBeVisible();
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

    await openMore();
    await userEvent.click(screen.getByRole("button", { name: /^deafen$/i }));

    expect(onSetDeafened).toHaveBeenCalledWith(false);
  });

  it("asks to be marked away", async () => {
    const { onSetAway } = panel(CONNECTED, "Lounge", HEARING);

    await openMore();
    await userEvent.click(screen.getByRole("button", { name: /^away$/i }));

    expect(onSetAway).toHaveBeenCalledWith(true);
  });

  it("asks to come back when it is already away", async () => {
    const { onSetAway } = panel(CONNECTED, "Lounge", {
      muted: false,
      deafened: false,
      away: true,
    });

    await openMore();
    await userEvent.click(screen.getByRole("button", { name: /^away$/i }));

    expect(onSetAway).toHaveBeenCalledWith(false);
  });

  it("draws the microphone as off while away", async () => {
    // Away mutes. A microphone button that looked live would be telling
    // somebody the room can hear them when it cannot.
    panel(CONNECTED, "Lounge", { muted: false, deafened: false, away: true });

    expect(
      screen.getByRole("button", { name: /mute microphone/i }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("does not deafen when it marks somebody away", async () => {
    // The entire difference from the button beside it. Somebody away is still
    // listening, and a deafen indicator would say otherwise.
    panel(CONNECTED, "Lounge", { muted: false, deafened: false, away: true });

    await openMore();

    expect(screen.getByRole("button", { name: /^deafen$/i })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
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

  describe("the state line, which is the way the call card comes back", () => {
    it("is a button rather than a line of text", () => {
      // The card can be put away from its own corner, and before this there
      // was nothing anywhere that brought it back.
      panel(CONNECTED);

      expect(stateLine()).toBeVisible();
    });

    it("says whether the card is up, rather than only drawing it", () => {
      panel(CONNECTED, "Lounge", HEARING, true);

      expect(stateLine()).toHaveAttribute("aria-expanded", "true");
    });

    it("says when the card is away", () => {
      panel(CONNECTED, "Lounge", HEARING, false);

      expect(stateLine()).toHaveAttribute("aria-expanded", "false");
    });

    it("puts the card away when it is up", async () => {
      // Both directions on the one control. A press that only ever showed
      // would be a control that does nothing most of the time it is pressed.
      const { onToggleCard } = panel(CONNECTED, "Lounge", HEARING, true);

      await userEvent.click(stateLine()!);

      expect(onToggleCard).toHaveBeenCalledTimes(1);
    });

    it("brings the card back when it is away", async () => {
      const { onToggleCard } = panel(CONNECTED, "Lounge", HEARING, false);

      await userEvent.click(stateLine()!);

      expect(onToggleCard).toHaveBeenCalledTimes(1);
    });

    it("can be reached and pressed from the keyboard", async () => {
      // The first thing in the strip, so one Tab from the top of it. A
      // control only a pointer can reach is a control some people do not have.
      const { onToggleCard } = panel(CONNECTED);

      await userEvent.tab();
      expect(stateLine()).toHaveFocus();
      await userEvent.keyboard("{Enter}");
      await userEvent.keyboard(" ");

      expect(onToggleCard).toHaveBeenCalledTimes(2);
    });

    it("says what pressing it will do, and says it fresh each way", () => {
      panel(CONNECTED, "Lounge", HEARING, true);
      panel(CONNECTED, "Lounge", HEARING, false);

      const [up, away] = screen.getAllByRole("button", {
        name: /voice connected/i,
      });
      expect(up).toHaveAttribute("title", "Hide the call card");
      expect(away).toHaveAttribute("title", "Show the call card");
    });

    it("stands at least the 24px WCAG asks of a target", () => {
      /*
        Measured off the real stylesheet rather than trusted. jsdom resolves
        the cascade, so this catches a rule that stopped matching as well as
        one whose number went under: #82 was this exact failure on the thread
        pill and #102 is still open on the reaction pills, and both were text
        that took its height from the words in it.
      */
      panel(CONNECTED);

      const line = stateLine();
      expect(line).not.toBeNull();
      expect(
        parseFloat(getComputedStyle(line!).minHeight),
      ).toBeGreaterThanOrEqual(24);
    });

    it("is not there at all when there is no call", () => {
      // Nothing to show and nothing to bring back, so no control for it.
      panel({ state: "disconnected" });

      expect(stateLine()).toBeNull();
    });
  });

  describe("the control the quieter actions moved behind", () => {
    it("keeps deafen and away out of the row until they are asked for", () => {
      // The whole point of #104: four controls and a label do not fit the
      // column, and the label is what lost. These two are the ones that go.
      panel(CONNECTED);

      expect(screen.queryByRole("button", { name: /^deafen$/i })).toBeNull();
      expect(screen.queryByRole("button", { name: /^away$/i })).toBeNull();
    });

    it("hands them over when it is pressed", async () => {
      panel(CONNECTED);

      await openMore();

      expect(screen.getByRole("button", { name: /^deafen$/i })).toBeVisible();
      expect(screen.getByRole("button", { name: /^away$/i })).toBeVisible();
    });

    it("says whether they are out, rather than only drawing them", () => {
      panel(CONNECTED);

      expect(disclosure()).toHaveAttribute("aria-expanded", "false");
    });

    it("says so the other way once they are", async () => {
      panel(CONNECTED);

      await openMore();

      expect(disclosure()).toHaveAttribute("aria-expanded", "true");
    });

    it("puts them away again when it is pressed a second time", async () => {
      // One control, both directions, the same rule the state line follows.
      panel(CONNECTED);

      await openMore();
      await userEvent.click(disclosure());

      expect(screen.queryByRole("button", { name: /^deafen$/i })).toBeNull();
    });

    it("keeps its name the same whichever way it is set", async () => {
      // The convention the rest of this strip follows: a button whose
      // accessible name changes under the cursor is announced as a new
      // button. The tooltip is where the wording may follow the state.
      panel(CONNECTED);

      expect(disclosure()).toHaveAttribute("title", "More voice actions");

      await openMore();

      expect(disclosure()).toHaveAttribute("title", "Hide the other actions");
    });

    it("can be reached and opened from the keyboard", async () => {
      // Second in the strip, behind the state line and the microphone. A
      // control only a pointer can reach is a control some people do not have.
      panel(CONNECTED);

      await userEvent.tab();
      await userEvent.tab();
      await userEvent.tab();
      expect(disclosure()).toHaveFocus();

      await userEvent.keyboard("{Enter}");

      expect(screen.getByRole("button", { name: /^deafen$/i })).toBeVisible();
    });

    it("moves focus into what it opened", async () => {
      // Otherwise the panel is out and the next Tab is somewhere else
      // entirely, which for a keyboard is the same as it never having opened.
      panel(CONNECTED);

      await openMore();

      expect(screen.getByRole("button", { name: /^deafen$/i })).toHaveFocus();
    });

    it("closes on Escape and hands the focus back", async () => {
      panel(CONNECTED);

      await openMore();
      await userEvent.keyboard("{Escape}");

      expect(screen.queryByRole("button", { name: /^deafen$/i })).toBeNull();
      expect(disclosure()).toHaveFocus();
    });

    it("keeps its Escape to itself", async () => {
      /*
        `ThreadPanel` and `RoomInfoPanel` both shut on an Escape they hear on
        the window. One press should close the panel in front, not that one as
        well as whatever is open behind it.
      */
      const behind = vi.fn();
      window.addEventListener("keydown", behind);
      try {
        panel(CONNECTED);

        await openMore();
        await userEvent.keyboard("{Escape}");

        expect(behind).not.toHaveBeenCalled();
      } finally {
        window.removeEventListener("keydown", behind);
      }
    });

    it("lets Escape past while nothing is open", async () => {
      // The other half of the same rule. A call is up for hours, and a strip
      // that swallowed every Escape for the length of one would take the key
      // away from the thread panel it is meant to close.
      const behind = vi.fn();
      window.addEventListener("keydown", behind);
      try {
        panel(CONNECTED);

        await userEvent.keyboard("{Escape}");

        expect(behind).toHaveBeenCalled();
      } finally {
        window.removeEventListener("keydown", behind);
      }
    });

    it("closes when a press lands somewhere else", async () => {
      panel(CONNECTED);

      await openMore();
      await userEvent.click(document.body);

      expect(screen.queryByRole("button", { name: /^deafen$/i })).toBeNull();
    });

    it("stays open across a press, so the switch can be seen to have taken", async () => {
      // These are toggles rather than commands. Shutting the panel on the
      // press would hide the one piece of feedback that says it worked, and
      // would take the focus with it.
      const { onSetDeafened } = panel(CONNECTED);

      await openMore();
      await userEvent.click(screen.getByRole("button", { name: /^deafen$/i }));

      expect(onSetDeafened).toHaveBeenCalledWith(true);
      expect(screen.getByRole("button", { name: /^away$/i })).toBeVisible();
    });

    it("stands at least the 24px WCAG asks of a target", () => {
      /*
        Read off the real stylesheet, the same way the state line is. jsdom
        resolves no `var()` and lays nothing out, so a literal declaration is
        the only shape this can be held in (#107).
      */
      panel(CONNECTED);

      const style = getComputedStyle(disclosure());
      expect(parseFloat(style.width)).toBeGreaterThanOrEqual(24);
      expect(parseFloat(style.height)).toBeGreaterThanOrEqual(24);
    });

    it("gives the same 24px to each action it opens", async () => {
      // A row of text in a panel is exactly the shape that comes out at 19px
      // and fails 2.5.8, which is what #107 is open about one panel over.
      panel(CONNECTED);

      await openMore();

      for (const name of [/^deafen$/i, /^away$/i]) {
        const action = screen.getByRole("button", { name });
        expect(
          parseFloat(getComputedStyle(action).minHeight),
        ).toBeGreaterThanOrEqual(24);
      }
    });

    it("is not there at all when there is no call", () => {
      panel({ state: "disconnected" });

      expect(
        screen.queryByRole("button", { name: /more voice actions/i }),
      ).toBeNull();
    });
  });
});
