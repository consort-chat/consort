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
        */}
        <span className="call-panel__state">
          <i className="call-panel__dot" aria-hidden="true" />
          {callLabel(call)}
        </span>
        <span className="call-panel__channel" title={channelName ?? undefined}>
          {channelName ?? "Voice channel"}
        </span>
      </div>

      {/*
        Icons, so each needs a name that is not its glyph. `title` as well,
        because none of these has its purpose written beside it and one of them
        ends a conversation.

        `aria-pressed` on the two that toggle, and not on the one that does not.
        A screen reader then says "mute, pressed" rather than leaving somebody
        to work out from a label whether the thing they just did took. The
        labels stay put across the press for the same reason: a button whose
        name changes under the cursor is announced as a new button.
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

        <button
          type="button"
          className="call-panel__control"
          onClick={() => onSetDeafened(!deafened)}
          aria-pressed={deafened}
          aria-label="Deafen"
          title={deafened ? "Undeafen" : "Deafen"}
        >
          <HeadphonesIcon off={deafened} />
        </button>

        {/*
          Between deafen and hang up, which is where it belongs by severity:
          the two to its left change what this session hears and says, this one
          says where the person is, and the one to its right ends the call.
        */}
        <button
          type="button"
          className="call-panel__control"
          onClick={() => onSetAway(!away)}
          aria-pressed={away}
          aria-label="Mark yourself away"
          title={away ? "You are away" : "Mark yourself away"}
        >
          <ClockIcon on={away} />
        </button>

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
