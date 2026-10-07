import type { Update } from "../lib/api";
import "./UpdateBar.css";

/**
 * A newer Consort, offered in one line across the top of the window.
 *
 * Nothing is drawn unless there is something to offer, which is what a packaged
 * build and an up-to-date one have in common. A bar saying "no updates" is a
 * bar nobody needs and a .deb user would file a bug about.
 */
export function UpdateBar({
  update,
  inACall,
  onInstall,
  onDismiss,
}: {
  /** The last thing said on the `update` channel, or null if nothing was. */
  update: Update | null;
  /** Installing restarts Consort, so a call in progress is in the way. */
  inACall: boolean;
  onInstall: () => void;
  onDismiss: () => void;
}) {
  if (update === null || update.state === "upToDate") {
    return null;
  }

  return (
    <section className="update" role="status" aria-label="Software update">
      <p className="update__line">{line(update, inACall)}</p>
      {update.state === "ready" && (
        <div className="update__actions">
          <button
            className="button button--small button--primary"
            disabled={inACall}
            onClick={onInstall}
            type="button"
          >
            Update and restart
          </button>
          <button
            className="button button--small"
            onClick={onDismiss}
            type="button"
          >
            Not now
          </button>
        </div>
      )}
    </section>
  );
}

/**
 * The sentence for whatever is happening.
 *
 * The refusal is on the line rather than only in the disabled button, because a
 * grey control with no reason beside it is the thing that gets reported as
 * broken.
 */
function line(update: Update, inACall: boolean): string {
  switch (update.state) {
    case "ready":
      return inACall
        ? `Consort ${update.version} is available. Installing it restarts Consort, which would drop you out of the call, so leave the call first.`
        : `Consort ${update.version} is available.`;
    case "downloading": {
      const done = progress(update.received, update.total);
      return done === null
        ? "Downloading the update."
        : `Downloading the update, ${done}%.`;
    }
    case "installing":
      return "Installing. Consort will restart.";
    case "failed":
      return update.reason;
    // Not reachable: an up-to-date build draws nothing. Handled so that a new
    // state added on the Rust side is a blank line rather than a crash.
    case "upToDate":
      return "";
  }
}

/**
 * How far along, as a whole percent, or null when that cannot be said.
 *
 * Null for an absent length and for a zero one, which are the same thing to a
 * reader: a server that did not say. Capped, because `Content-Length` is
 * whatever the other end claimed.
 */
export function progress(received: number, total: number | null): number | null {
  if (total === null || total <= 0) {
    return null;
  }
  return Math.min(100, Math.round((received / total) * 100));
}
