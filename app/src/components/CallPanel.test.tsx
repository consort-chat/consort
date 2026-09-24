import { act, fireEvent, render, screen } from "@testing-library/react";
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

/**
 * Long enough for a hover to have been meant, twice over.
 *
 * The component waits before it acts on a pointer, so a test that asserts
 * nothing opened has to outlast that wait or it is only asserting that it has
 * not opened yet.
 */
const PAST_THE_OPEN = 400;

/**
 * Short of the linger, and not by a little.
 *
 * The close is the slower of the two waits by an order of magnitude, and this
 * is the mark a test sits on to say the panel is still there. Well clear of
 * the open wait, so a panel still on screen here is one being held rather than
 * one that has not got round to closing.
 */
const INSIDE_THE_LINGER = 1000;

/**
 * Past the linger, with room for a machine under load.
 *
 * These waits are real rather than faked, because what is under test is a
 * duration and a fake clock only ever plays back the number the source already
 * says. Bracketing it with two real waits is what makes these able to disagree
 * with the source: a close that came back to the wait that opens fails at
 * `INSIDE_THE_LINGER`, and one that never comes fails here.
 *
 * Half a second of slack over the 1500ms the close was measured at, because
 * the cost of the slack is a slower suite and the cost of cutting it fine is a
 * test that fails on a busy machine for no reason anybody can reproduce.
 */
const PAST_THE_LINGER = 2000;

/**
 * Sit out one of the waits above.
 *
 * Inside `act`, because the wait is the whole point: when the panel does close
 * on a pointer leaving, it closes from a timer rather than from anything the
 * test did, and React has no other way to be told that update was expected.
 */
async function sit(ms: number) {
  await act(async () => {
    await new Promise((resolve) => {
      window.setTimeout(resolve, ms);
    });
  });
}

/** The first of the two actions, named by what the panel writes out. */
function deafen() {
  return screen.queryByRole("button", { name: /^deafen$/i });
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

    it("moves focus into what a key opened", async () => {
      // Otherwise the panel is out and the next Tab is somewhere else
      // entirely, which for a keyboard is the same as it never having opened.
      panel(CONNECTED);

      disclosure().focus();
      await userEvent.keyboard("{Enter}");

      expect(screen.getByRole("button", { name: /^deafen$/i })).toHaveFocus();
    });

    it("leaves focus on the chevron when a hand opened it", async () => {
      /*
        The mouse half of the rule above, and not a smaller version of it. A
        pointer needs no carrying: the panel is the next thing after the
        chevron either way, so Tab reaches it from where the focus already is.

        Carrying it would cost the close. Focus inside the panel is what tells
        the pointer-leave close to hold for somebody using the keyboard, so a
        press that put it there would pin every mouse-opened panel against the
        pointer leaving, which is the defect #109 came back about.
      */
      panel(CONNECTED);

      await openMore();

      expect(screen.getByRole("button", { name: /^deafen$/i })).toBeVisible();
      expect(disclosure()).toHaveFocus();
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

    describe("opening on a hover, which is a second way in and not the only one", () => {
      it("opens when the pointer settles on it", async () => {
        // The ask from #109: the two actions a pointer away rather than a
        // press away.
        panel(CONNECTED);

        await userEvent.hover(disclosure());

        expect(
          await screen.findByRole("button", { name: /^deafen$/i }),
        ).toBeVisible();
      });

      it("leaves the focus alone, unlike a press", async () => {
        /*
          The half that makes a hover safe to add beside the press. A pointer
          crossing a chevron is not a request to move the caret, and somebody
          typing in the room beside this would lose what they were mid-way
          through.
        */
        panel(CONNECTED);
        const before = document.activeElement;

        await userEvent.hover(disclosure());
        await screen.findByRole("button", { name: /^deafen$/i });

        expect(deafen()).not.toHaveFocus();
        expect(document.activeElement).toBe(before);
      });

      it("puts it away again when the pointer leaves", async () => {
        // What opened it is what closes it. A hover that had to be pressed to
        // undo would leave the panel sitting over the channel list.
        panel(CONNECTED);

        await userEvent.hover(disclosure());
        await screen.findByRole("button", { name: /^deafen$/i });
        await userEvent.unhover(disclosure());

        await sit(PAST_THE_LINGER);
        expect(deafen()).toBeNull();
      });

      it("ignores a pointer that only crossed it", async () => {
        /*
          The chevron sits between the microphone and the way out, so every
          pointer going to either passes over it. Opening on the crossing
          would throw the panel over the channel list several times a minute.
        */
        panel(CONNECTED);

        await userEvent.hover(disclosure());
        await userEvent.unhover(disclosure());

        await sit(PAST_THE_OPEN);
        expect(deafen()).toBeNull();
      });

      it("puts a panel that was pressed open away too", async () => {
        /*
          This used to assert the opposite, and the opposite is what #109 came
          back about. The reasoning was that a press is an ask and stays
          answered until it is taken back, which is true of a keyboard and not
          of a hand: the pointer leaving *is* how a hand takes it back, and
          without this the only way to shut a panel opened by pressing the
          chevron was to press somewhere else entirely.
        */
        panel(CONNECTED);

        await openMore();
        await userEvent.unhover(disclosure());

        await sit(PAST_THE_LINGER);
        expect(deafen()).toBeNull();
      });

      it("puts it away after a hand has used what it opened", async () => {
        /*
          The same stuck panel one control further in, and the one a rule
          written around where the focus sits gets wrong. Pressing Deafen with
          a mouse focuses Deafen, exactly as Tab into it would, so a close that
          held for any focus inside the panel would hold for the press the
          panel exists to receive. Every visit would end with it still open.
        */
        panel(CONNECTED);

        await userEvent.hover(disclosure());
        await userEvent.click(
          await screen.findByRole("button", { name: /^deafen$/i }),
        );
        await userEvent.unhover(disclosure());

        await sit(PAST_THE_LINGER);
        expect(deafen()).toBeNull();
      });

      it("keeps it while the linger is still running", async () => {
        /*
          The other half of the measurement, and the half that would notice the
          close coming back to the wait that opens. A second is a long way past
          150ms and a long way short of the 1500ms asked for on #109, so a
          panel still on screen here is one being held on purpose.
        */
        panel(CONNECTED);

        await userEvent.hover(disclosure());
        await screen.findByRole("button", { name: /^deafen$/i });
        await userEvent.unhover(disclosure());

        await sit(INSIDE_THE_LINGER);
        expect(deafen()).toBeVisible();
      });

      it("takes the pending close off a press that arrives inside it", async () => {
        /*
          The case a long linger creates and a short one hid. The pointer has
          gone and the close is counting down, and in that second and a half
          somebody reaches the control another way: a key, or a finger. Without
          the press cancelling what the pointer armed, the panel they have just
          asked for puts itself away a moment later, and every part of that is
          invisible to them.
        */
        panel(CONNECTED);

        await userEvent.hover(disclosure());
        await screen.findByRole("button", { name: /^deafen$/i });
        await userEvent.unhover(disclosure());

        disclosure().focus();
        await userEvent.keyboard("{Enter}");

        await sit(PAST_THE_LINGER);
        expect(deafen()).toBeVisible();
      });

      it("holds still once the keyboard has caught up with the pointer", async () => {
        // Hovered open, then Tabbed into. Closing because the pointer moved on
        // would take the focus out of the panel somebody is using.
        panel(CONNECTED);

        await userEvent.hover(disclosure());
        const action = await screen.findByRole("button", { name: /^deafen$/i });
        action.focus();
        await userEvent.unhover(disclosure());

        await sit(PAST_THE_LINGER);
        expect(deafen()).toBeVisible();
        expect(deafen()).toHaveFocus();
      });

      it("holds still when a key follows a hand into the panel", async () => {
        /*
          Pressed open with a mouse and then Tabbed into, which is the order
          somebody switching from one to the other does it in. The press marks
          the focus as the hand's, and the Tab has to hand it back, or the
          pointer wandering off closes the panel around a keyboard that is by
          then the thing using it.
        */
        panel(CONNECTED);

        await openMore();
        await userEvent.tab();
        expect(deafen()).toHaveFocus();

        await userEvent.unhover(disclosure());

        await sit(PAST_THE_LINGER);
        expect(deafen()).toBeVisible();
        expect(deafen()).toHaveFocus();
      });

      it("closes even while the chevron itself holds the focus", async () => {
        /*
          The limit of the clause above, and what keeps it from becoming the
          latch it replaced. The keyboard being *in the panel* is what earns a
          panel the right to stay; the keyboard resting on the control that
          opens it is not the same thing, and counting it would leave every
          hover-opened panel stuck for anyone who had tabbed this far.
        */
        panel(CONNECTED);

        disclosure().focus();
        await userEvent.hover(disclosure());
        await screen.findByRole("button", { name: /^deafen$/i });
        await userEvent.unhover(disclosure());

        await sit(PAST_THE_LINGER);
        expect(deafen()).toBeNull();
      });

      it("does not take a finger arriving for a hover", async () => {
        /*
          A tap fires a pointer event before it fires a click, so a finger
          arrives over the control the same way a cursor does. Taken as a
          hover it would open the panel on the way to the press and the press
          would then shut what the finger had just opened, which on the second
          tap is a panel a touchscreen cannot put away.

          Fired rather than tapped: `userEvent` leaves the pointer over the
          control after a tap, so a second tap in jsdom never arrives again and
          the difference this guard makes would not show.
        */
        panel(CONNECTED);

        fireEvent.pointerOver(disclosure(), { pointerType: "touch" });

        await sit(PAST_THE_OPEN);
        expect(deafen()).toBeNull();
      });

      it("takes a cursor arriving, which is the same event from a mouse", async () => {
        // The control for the test above: the guard is on what kind of
        // pointer it is, not on the event never being heard.
        panel(CONNECTED);

        fireEvent.pointerOver(disclosure(), { pointerType: "mouse" });

        expect(
          await screen.findByRole("button", { name: /^deafen$/i }),
        ).toBeVisible();
      });

      it("still opens on a tap, which is the press it falls back to", async () => {
        // Touch keeps the way in it always had.
        panel(CONNECTED);

        await userEvent.pointer({ target: disclosure(), keys: "[TouchA]" });

        expect(deafen()).toBeVisible();
      });

      it("opens on one press after a press elsewhere put it away", async () => {
        /*
          Thomas's second report on #109, walked exactly: press the chevron,
          press outside to shut it, come back to the chevron, press it again.
          The second press has to open it.

          What made it take two was a single flag written by both ways in. The
          pointer coming back opened the panel on its own, the press read that
          as open and shut it, and only the press after that appeared to work.
          On screen it looks like the control flipping to hidden and back for
          no reason, which is how he described it.
        */
        panel(CONNECTED);

        await openMore();
        expect(deafen()).toBeVisible();

        await userEvent.click(document.body);
        expect(deafen()).toBeNull();

        // Back to the chevron, which opens it on the way, and then the press.
        await userEvent.hover(disclosure());
        await sit(PAST_THE_OPEN);
        await userEvent.click(disclosure());

        expect(deafen()).toBeVisible();
      });

      it("keeps it open when a press lands on a panel a hover opened", async () => {
        /*
          The same fault one step smaller, and the one that would come back
          first if the press ever went back to inverting what it found. No
          press elsewhere, no coming back: just a pointer resting long enough
          to open the panel and then pressing the control under it.
        */
        panel(CONNECTED);

        await userEvent.hover(disclosure());
        await screen.findByRole("button", { name: /^deafen$/i });

        await userEvent.click(disclosure());

        expect(deafen()).toBeVisible();
        expect(disclosure()).toHaveAttribute("aria-expanded", "true");
      });

      it("still shuts on the press after that", async () => {
        /*
          The control for the two above. A first press that always opens would
          be a chevron a pointer can never shut, so the second press has to
          take the hover with it rather than leaving it to reopen what was just
          put away.
        */
        panel(CONNECTED);

        await userEvent.hover(disclosure());
        await screen.findByRole("button", { name: /^deafen$/i });

        await userEvent.click(disclosure());
        await userEvent.click(disclosure());

        expect(deafen()).toBeNull();
      });

      it("still opens from the keyboard, which hovers nothing", async () => {
        // Why the hover is added beside the press rather than put in its
        // place: this is the way in that has no pointer to offer.
        panel(CONNECTED);

        disclosure().focus();
        await userEvent.keyboard("{Enter}");

        expect(deafen()).toBeVisible();
        expect(deafen()).toHaveFocus();
      });
    });

    it("is not there at all when there is no call", () => {
      panel({ state: "disconnected" });

      expect(
        screen.queryByRole("button", { name: /more voice actions/i }),
      ).toBeNull();
    });
  });
});
