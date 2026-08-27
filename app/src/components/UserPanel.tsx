import type { Connection, Profile } from "../lib/api";
import { connectionLabel, initialsOf } from "../lib/labels";
import "./UserPanel.css";

interface Props {
  profile: Profile;
  connection: Connection;
  onOpenSettings: () => void;
}

/**
 * Who you are, whether the client is connected, and the way into settings.
 *
 * The strip along the bottom of the channel list, which is where Discord puts
 * it and where a hand already is. Small on purpose: it holds the three things
 * worth having permanently on screen and nothing else. Anything that needs a
 * sentence to explain belongs in the main pane, which is why the verification
 * banner is not here.
 *
 * The third thing used to be Sign out. It is a gear now, and signing out moved
 * behind it into My Account. Not a rearrangement for its own sake: this strip
 * is thirty-two pixels tall and sits directly under a list people click
 * quickly, and the only irreversible action in the application does not belong
 * one stray click from a channel.
 *
 * A labelled group rather than a bare div. The account name used to be this
 * screen's `h1`, which was true when the screen was one centred card and is
 * not true of a thirty-two pixel strip in a corner. A group announces itself
 * on the way in, which is what somebody arriving here by keyboard needs, and
 * it gives the name somewhere to live that is not a heading it is not.
 */
export function UserPanel({ profile, connection, onOpenSettings }: Props) {
  const name = profile.display_name ?? profile.user_id;

  return (
    <div
      className="user-panel"
      data-connection={connection.state}
      role="group"
      aria-label="Account"
    >
      {/*
        The connection dot rides on the avatar rather than beside the label,
        which is where every client that has this strip puts it. It is not
        decoration: in a 240px column it is the sixteen pixels that let
        "Session ended" be written out rather than truncated to "Session e".
      */}
      <div className="user-panel__avatar" aria-hidden="true">
        {initialsOf(name)}
        <i className="user-panel__dot" />
      </div>

      <div className="user-panel__who">
        <span className="user-panel__name" title={profile.user_id}>
          {name}
        </span>
        <span className="user-panel__status" aria-live="polite">
          {connectionLabel(connection)}
        </span>
      </div>

      {/*
        An icon, so it needs a name that is not its glyph: the gear is drawn in
        an `aria-hidden` span and the label lives in `aria-label`. `title` as
        well, because this is the one control on the strip whose purpose is not
        written next to it.
      */}
      <button
        className="user-panel__settings"
        onClick={onOpenSettings}
        aria-label="User settings"
        title="User settings"
      >
        <span aria-hidden="true">⚙</span>
      </button>
    </div>
  );
}
