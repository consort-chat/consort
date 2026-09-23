import { useEffect, useRef, useState } from "react";

import { microphoneOff, type Call, type SelfAudio } from "../lib/api";
import { callLabel } from "../lib/labels";
import "./CallPanel.css";

/**
 * A handset laid back down, for the control that leaves.
 *
 * It used to be a crossed-out speaker, chosen so it could not be mistaken for
 * the speaker that joins. It could not, but it was mistaken for something
 * worse: sitting in a row that already mutes a microphone and deafens a pair
 * of headphones, a struck-through speaker reads as a third switch for audio
 * rather than as the way out. A handset is the one glyph in this vocabulary
 * that ends a call instead of silencing part of one, which is why every other
 * client uses it.
 *
 * The drawing is the handset the rest of the world draws, turned until it lies
 * flat. The rotation is what makes it a hang-up rather than a call, and the
 * scale keeps the corners of a diagonal shape inside the box once it is turned
 * across it. `strokeWidth` is that scale divided back out, so this weighs the
 * same on screen as the three icons beside it.
 */
function HangUpIcon() {
  return (
    <svg
      className="call-panel__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.56"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path
        transform="rotate(135 12 12) translate(2.64 2.64) scale(0.78)"
        d="M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z"
      />
    </svg>
  );
}

/**
 * A microphone, and the same microphone struck through.
 *
 * One component with a slash it can draw or not, rather than two icons. The
 * body has to stay in exactly the same place between the two states or the
 * button appears to jump when it is pressed, and the surest way to keep two
 * drawings identical is for there to be one drawing.
 */
function MicrophoneIcon({ off }: { off: boolean }) {
  return (
    <svg
      className="call-panel__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="9" y="2" width="6" height="11" rx="3" />
      <path d="M5 10a7 7 0 0 0 14 0" />
      <path d="M12 17v4" />
      {off && <path d="M3 3l18 18" />}
    </svg>
  );
}

/**
 * Headphones, struck through when this session has stopped listening.
 *
 * Headphones rather than a second speaker, so that the thing being switched
 * off is what this session hears rather than a speaker somewhere in the room.
 * It also keeps the strike-through meaning one thing: the two icons that carry
 * one are the two ends of this session's audio, and nothing else in the row
 * borrows it.
 */
function HeadphonesIcon({ off }: { off: boolean }) {
  return (
    <svg
      className="call-panel__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M4 15v-3a8 8 0 0 1 16 0v3" />
      <path d="M4 15h3v5H5.5A1.5 1.5 0 0 1 4 18.5z" />
      <path d="M20 15h-3v5h1.5a1.5 1.5 0 0 0 1.5-1.5z" />
      {off && <path d="M3 3l18 18" />}
    </svg>
  );
}

/**
 * A clock, filled in when this session has said nobody is here.
 *
 * A clock rather than a crossed-out anything, and that is the point of the
 * button: the other two icons say what is switched off, and this one says
 * where the person went. TeamSpeak drew it this way and it reads instantly.
 */
function ClockIcon({ on }: { on: boolean }) {
  return (
    <svg
      className="call-panel__glyph"
      viewBox="0 0 24 24"
      fill={on ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" stroke={on ? "var(--surface)" : "currentColor"} />
    </svg>
  );
}

/**
 * A chevron, pointing where the panel comes out.
 *
 * Right while the quieter actions are put away, which is the arrow the issue
 * asked for, and up once they are out, because up is where they go: this strip
 * is the bottom of the sidebar and there is nowhere below it. An arrow still
 * aiming right with the panel open would be pointing at the edge of the
 * window.
 */
function ChevronIcon() {
  return (
    <svg
      className="call-panel__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M9 6l6 6-6 6" />
    </svg>
  );
}

/**
 * Deafen and away, behind one control.
 *
 * Four buttons and a label do not fit a 240px column, and the label is what
 * lost: "Voice connected" was arriving as a word and a half. These two are the
 * pair that goes, because they are the two nobody reaches for in a hurry.
 * Muting and hanging up stay where a hand finds them without reading anything.
 *
 * Above the control rather than below it, and floating rather than a second
 * row. A row that appeared inside the strip would push the channel list up
 * every time somebody looked at it, and there is no room under the strip to
 * push into.
 *
 * Its own component, and not because it is reused. `CallPanel` returns null
 * before it does anything when there is no call, so state belonging to the row
 * cannot be held above that line without the hooks moving with it.
 */
function MoreActions({
  deafened,
  away,
  onSetDeafened,
  onSetAway,
}: {
  deafened: boolean;
  away: boolean;
  onSetDeafened: (deafened: boolean) => void;
  onSetAway: (away: boolean) => void;
}) {
  const [shown, setShown] = useState(false);
  const wrap = useRef<HTMLSpanElement | null>(null);
  const toggle = useRef<HTMLButtonElement | null>(null);
  const first = useRef<HTMLButtonElement | null>(null);

  /*
    Focus follows the panel out. Without it the actions are on screen and the
    next Tab is somewhere else entirely, which for a keyboard is the same as
    the control having done nothing.
  */
  useEffect(() => {
    if (shown) first.current?.focus();
  }, [shown]);

  useEffect(() => {
    if (!shown) return;

    function hide() {
      /*
        The focus goes back to the control that opened this, but only when the
        panel still has it. A press somewhere else has already chosen where
        focus is going, and pulling it back here would take it from them.
      */
      if (wrap.current?.contains(document.activeElement) === true) {
        toggle.current?.focus();
      }
      setShown(false);
    }

    function onEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      // Stopped here, so one press shuts this rather than also reaching
      // whatever else on the window is listening for it.
      event.stopPropagation();
      hide();
    }

    /*
      `mousedown` rather than `click`, on the same terms as every other
      dismissable thing here: a drag that starts inside and ends outside is not
      somebody asking for this to close. Measured against the wrapper rather
      than the panel, so that a press on the control itself closes it once
      instead of closing it and letting the click open it again.
    */
    function elsewhere(event: MouseEvent) {
      if (event.target instanceof Node && wrap.current?.contains(event.target)) {
        return;
      }
      hide();
    }

    document.addEventListener("keydown", onEscape);
    document.addEventListener("mousedown", elsewhere);
    return () => {
      document.removeEventListener("keydown", onEscape);
      document.removeEventListener("mousedown", elsewhere);
    };
  }, [shown]);

  return (
    <span className="call-panel__more" ref={wrap}>
      {/*
        `aria-expanded` and a name that stays put, the way the state line above
        does it and for the same reason: a button renamed under the cursor is
        announced as a different button each press. The tooltip is where the
        wording is allowed to follow the state.
      */}
      <button
        type="button"
        className="call-panel__control call-panel__more-toggle"
        ref={toggle}
        aria-expanded={shown}
        aria-label="More voice actions"
        title={shown ? "Hide the other actions" : "More voice actions"}
        onClick={() => setShown(!shown)}
      >
        <ChevronIcon />
      </button>
      {shown && (
        /*
          Written out rather than drawn, which is the one thing this panel has
          that the row did not have room for. The words are the accessible name
          as well, so they hold still across a press while `aria-pressed`
          carries which way the switch is set.

          Nothing closes on a press. These are toggles, not commands, and
          shutting the panel would hide the one piece of feedback saying the
          press took, as well as taking the focus with it.
        */
        <div className="call-panel__more-actions">
          <button
            type="button"
            className="call-panel__more-action"
            ref={first}
            onClick={() => onSetDeafened(!deafened)}
            aria-pressed={deafened}
            title={deafened ? "Undeafen" : "Deafen"}
          >
            <HeadphonesIcon off={deafened} />
            Deafen
          </button>
          <button
            type="button"
            className="call-panel__more-action"
            onClick={() => onSetAway(!away)}
            aria-pressed={away}
            title={away ? "You are away" : "Mark yourself away"}
          >
            <ClockIcon on={away} />
            Away
          </button>
        </div>
      )}
    </span>
  );
}

interface Props {
  call: Call;
  /**
   * What the channel being called is named, or null when nothing local knows.
   *
   * Resolved by the caller rather than looked up here, because the room list
   * is the shell's and a panel that went and found its own would be a second
   * answer to drift from the first.
   */
  channelName: string | null;
  /**
   * Whether this session has muted or deafened itself.
   *
   * Carried in rather than held here, because it survives this panel: it is
   * true of the session rather than of the call, and a component that unmounts
   * when a call ends is the wrong place to keep something that does not.
   */
  selfAudio: SelfAudio;
  /**
   * Why this session cannot play the call, if it cannot.
   *
   * A separate sentence from `call.trouble`, which is about whether the audio
   * decrypts. This is about whether there is anywhere to put it once it has,
   * and the two fail independently: a call can be perfectly healthy and still
   * be coming out of a device that another application is holding.
   *
   * Worth its own line rather than a log entry, because speakers that will not
   * open look exactly like a call nobody is speaking in. Without it somebody
   * spends an evening blaming their microphone.
   */
  audioProblem?: string | null;
  /**
   * Whether the floating call card is on screen.
   *
   * Only ever read, never set here. The state line below is what turns it on
   * and off, and it is here rather than on the card because a card that has
   * drawn nothing has no control left to press.
   */
  cardShown: boolean;
  /** Show the call card, or put it away. One control, both directions. */
  onToggleCard: () => void;
  onDisconnect: () => void;
  onSetMuted: (muted: boolean) => void;
  onSetDeafened: (deafened: boolean) => void;
  onSetAway: (away: boolean) => void;
}

/**
 * Where you are, in voice, and the way out of it.
 *
 * Sits directly above the account strip, which is where Discord puts it and
 * where the eye already goes for "what is this client doing". Absent entirely
 * when there is no call: a permanent row saying "not in a voice channel" is a
 * row that is wrong-looking most of the time and teaches people to stop
 * reading it.
 *
 * Absent for a failure too, which is the one judgement call here. A failed
 * join leaves the channel list unchanged and nothing to disconnect from, so
 * what is left to say is why, and that belongs beside the channel that was
 * clicked rather than in a panel about a call that does not exist. The state
 * is still carried in so this can change its mind without the shell changing
 * shape.
 */
export function CallPanel({
  call,
  channelName,
  selfAudio,
  audioProblem = null,
  cardShown,
  onToggleCard,
  onDisconnect,
  onSetMuted,
  onSetDeafened,
  onSetAway,
}: Props) {
  if (call.state === "disconnected" || call.state === "failed") return null;

  // Deafening mutes, and so does being away, so the microphone button reads as
  // off in all three cases. It stays pressable: unmuting while deafened or away
  // is a reasonable thing to ask for and the Rust side takes it, it simply does
  // not take effect until the stronger state is cleared. What it must not do is
  // claim the microphone is live when it is not.
  const { muted, deafened } = selfAudio;
  const away = selfAudio.away === true;
  const off = microphoneOff(selfAudio);

  return (
    <div
      className="call-panel"
      data-state={call.state}
      role="group"
      aria-label="Voice connection"
    >
      <div className="call-panel__where">
        {/*
          The state is written out, not only coloured. Mint against amber is
          the reinforcement, never the message.

          A button, because it is also the way the call card comes back. The
          card can be put away from its own corner and there was nothing that
          brought it back, which left somebody in a call with no card and
          nowhere to ask for one. This line is the answer: it is already the
          one thing on screen that is about the call and nothing else, it is
          in the part of the sidebar that never scrolls, and it costs no room
          in a strip that has none to give.

          `aria-expanded` rather than a label that changes, the way the room
          header does it: the name stays put across the press, which is what
          stops a screen reader announcing it as a different button each time.

          Both directions on the one control. Pressing it while the card is up
          puts it away, which is what "toggle" has to mean for the press to be
          worth making twice.
        */}
        <button
          type="button"
          className="call-panel__state"
          aria-expanded={cardShown}
          title={cardShown ? "Hide the call card" : "Show the call card"}
          onClick={onToggleCard}
        >
          <i className="call-panel__dot" aria-hidden="true" />
          {/*
            The words in a span of their own, so the ellipsis has a block to
            happen in. `text-overflow` on the flex container above it never
            reached the bare text node, so a label too long for the column was
            cut off mid-letter rather than trailed off.
          */}
          <span className="call-panel__label">{callLabel(call)}</span>
        </button>
        <span className="call-panel__channel" title={channelName ?? undefined}>
          {channelName ?? "Voice channel"}
        </span>
      </div>

      {/*
        Icons, so each needs a name that is not its glyph. `title` as well,
        because none of these has its purpose written beside it and one of them
        ends a conversation.

        `aria-pressed` on the one that toggles, and not on the two that do not.
        A screen reader then says "mute, pressed" rather than leaving somebody
        to work out from a label whether the thing they just did took. The
        labels stay put across the press for the same reason: a button whose
        name changes under the cursor is announced as a new button.

        Three of them, still in severity order: what this session says, then
        the quieter pair behind the chevron, then the way out.
      */}
      <div className="call-panel__controls">
        <button
          type="button"
          className="call-panel__control"
          onClick={() => onSetMuted(!muted)}
          aria-pressed={off}
          aria-label="Mute microphone"
          title={off ? "Unmute" : "Mute"}
        >
          <MicrophoneIcon off={off} />
        </button>

        <MoreActions
          deafened={deafened}
          away={away}
          onSetDeafened={onSetDeafened}
          onSetAway={onSetAway}
        />

        <button
          type="button"
          className="call-panel__leave"
          onClick={onDisconnect}
          aria-label="Disconnect from voice"
          title="Disconnect"
        >
          <HangUpIcon />
        </button>
      </div>

      {/*
        An alert, and the loudest thing on this strip when it is here. A call
        whose audio will not decrypt looks exactly like a working one from
        every other angle: the membership published, the roster is right, and
        the packets are arriving. Nothing else on this screen would say so.

        Spanning both columns rather than sitting in the text column, because
        it is a sentence rather than a label and a 240px column with a button
        beside it would break it over four lines.
      */}
      {call.state === "connected" && call.trouble !== null && (
        <p className="call-panel__problem" role="alert">
          {call.trouble}
        </p>
      )}
      {/*
        Below the other one and never instead of it. They are different
        failures with different fixes, and a call can have both: nothing
        decrypts *and* there is nowhere to play it.
      */}
      {audioProblem !== null && (
        <p className="call-panel__problem" role="alert">
          {audioProblem}
        </p>
      )}
    </div>
  );
}
