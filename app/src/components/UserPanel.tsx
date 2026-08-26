import { asCommandError, logout, type Connection, type Profile } from "../lib/api";
import "./UserPanel.css";

/**
 * One short phrase per state, for the panel.
 *
 * A stopped loop is the only case that does not imply a message might still
 * arrive, and a session the homeserver has rejected is the only one the user
 * has to do something about, so those two do not share a label.
 */
export function connectionLabel(connection: Connection): string {
  switch (connection.state) {
    case "connecting":
      return "Connecting";
    case "live":
      return "Connected";
    case "offline":
      return "Reconnecting";
    case "stopped":
      return connection.reason === "sessionEnded"
        ? "Session ended"
        : "Disconnected";
  }
}

/** The first letter of whatever we are calling somebody, for the avatar. */
export function initialOf(name: string): string {
  return name.replace(/^@/, "").charAt(0).toUpperCase() || "?";
}

interface Props {
  profile: Profile;
  connection: Connection;
  pending: boolean;
  onSigningOut: () => void;
  onSignedOut: () => void;
}

/**
 * Who you are, whether the client is connected, and the way out.
 *
 * The strip along the bottom of the channel list, which is where Discord puts
 * it and where a hand already is. Small on purpose: it holds the three things
 * worth having permanently on screen and nothing else. Anything that needs a
 * sentence to explain belongs in the main pane, which is why the verification
 * banner is not here.
 *
 * A labelled group rather than a bare div. The account name used to be this
 * screen's `h1`, which was true when the screen was one centred card and is
 * not true of a thirty-two pixel strip in a corner. A group announces itself
 * on the way in, which is what somebody arriving here by keyboard needs, and
 * it gives the name somewhere to live that is not a heading it is not.
 */
export function UserPanel({
  profile,
  connection,
  pending,
  onSigningOut,
  onSignedOut,
}: Props) {
  const name = profile.display_name ?? profile.user_id;

  async function handleLogout() {
    onSigningOut();
    try {
      await logout();
    } catch (raw: unknown) {
      // `logout` clears the local session even when the server call fails, so
      // there is no state where staying on this screen is correct.
      console.error("logout reported an error", asCommandError(raw).detail);
    }
    onSignedOut();
  }

  return (
    <div className="user-panel" role="group" aria-label="Account">
      <div className="user-panel__avatar" aria-hidden="true">
        {initialOf(name)}
      </div>

      <div className="user-panel__who">
        <span className="user-panel__name" title={profile.user_id}>
          {name}
        </span>
        <span
          className="user-panel__status"
          data-connection={connection.state}
          aria-live="polite"
        >
          <i className="user-panel__dot" aria-hidden="true" />
          {connectionLabel(connection)}
        </span>
      </div>

      <button
        className="button button--ghost button--small"
        onClick={handleLogout}
        disabled={pending}
      >
        {pending ? "Signing out…" : "Sign out"}
      </button>
    </div>
  );
}
