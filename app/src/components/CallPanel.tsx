import type { Call } from "../lib/api";
import { callLabel } from "../lib/labels";
import "./CallPanel.css";

/**
 * A crossed-out speaker, for the control that leaves.
 *
 * The same speaker the channel list draws, with a line through it, because the
 * one thing this button must not be mistaken for is the one that joins.
 */
function HangUpIcon() {
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
      <path d="M11 5 6.5 9H3v6h3.5L11 19z" />
      <path d="m16 9 5 6" />
      <path d="m21 9-5 6" />
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
  onDisconnect: () => void;
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
export function CallPanel({ call, channelName, onDisconnect }: Props) {
  if (call.state === "disconnected" || call.state === "failed") return null;

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
        An icon, so it needs a name that is not its glyph. `title` as well,
        because this is the one control here whose purpose is not written
        beside it, and it is the one that ends a conversation.
      */}
      <button
        type="button"
        className="call-panel__leave"
        onClick={onDisconnect}
        aria-label="Disconnect from voice"
        title="Disconnect"
      >
        <HangUpIcon />
      </button>

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
    </div>
  );
}
