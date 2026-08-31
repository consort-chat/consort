import type { CallReadiness, CallRefused } from "../lib/api";
import "./CallRefusedNotice.css";

/**
 * Why a click on a voice channel did nothing.
 *
 * Without this the gate is worse than the bug it prevents. A join that is
 * silently declined looks exactly like a click that missed, and somebody would
 * press it four more times before deciding the application was broken.
 *
 * It says what happened and points at the way out rather than repeating it.
 * The verification banner sits directly below this and already offers both
 * routes, the emoji comparison and the recovery key, phrased for whichever
 * ones this account actually has. Restating any of that here would be a second
 * copy to drift from the first.
 */
export function CallRefusedNotice({
  refusal,
  channelName,
  onDismiss,
}: {
  refusal: CallRefused;
  /** What the channel is called, or null when the room list has not got it. */
  channelName: string | null;
  onDismiss: () => void;
}) {
  const where = channelName ?? "That voice channel";

  return (
    <section
      className="refusal"
      role="alert"
      aria-label="Voice channel not joined"
    >
      <p className="refusal__headline">{where} was not joined.</p>
      <p className="refusal__detail">{reason(refusal.readiness)}</p>
      <div className="refusal__actions">
        <button
          className="button button--small"
          onClick={onDismiss}
          type="button"
        >
          Dismiss
        </button>
      </div>
    </section>
  );
}

/**
 * The sentence for each way of not being able to be heard.
 *
 * Two, because they are cleared in two different places and somebody told the
 * wrong one goes and does the wrong thing. `sessionUnverified` is fixed on this
 * device; `noIdentity` belongs to the account and can be fixed from any client
 * signed in to it.
 *
 * Both lead with the consequence rather than the cause. "Nobody would have
 * been able to hear you" is the fact somebody needs; that it is about
 * cross-signing is the explanation, and it comes second.
 */
function reason(readiness: CallReadiness): string {
  switch (readiness.state) {
    case "sessionUnverified":
      return (
        "Nobody in the call would have been able to hear you, because this " +
        "session is not verified and encrypted calls will not accept its " +
        "audio. Verify it below and try again."
      );
    case "noIdentity":
      return (
        "Nobody in the call would have been able to hear you, because this " +
        "account has no encryption identity set up yet. Set up recovery, " +
        "here or on any other client you are signed in to, and try again."
      );
    // Not reachable: a ready session is not refused. Handled anyway, because
    // the alternative is a blank alert if a future state is added and this is
    // not.
    case "ready":
      return "This session can be heard in calls, so this should not have happened.";
  }
}
