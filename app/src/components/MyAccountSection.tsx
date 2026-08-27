import { useState } from "react";

import { asCommandError, logout, type Profile } from "../lib/api";
import { initialsOf } from "../lib/labels";
import "./MyAccountSection.css";

interface Props {
  profile: Profile;
  onSignedOut: () => void;
}

/**
 * Who is signed in, and the way out.
 *
 * Sign out lives here rather than on the strip at the bottom of the channel
 * list, which is where it used to be and where Discord does not put it. That
 * strip is thirty-two pixels tall, sits under a list people click quickly, and
 * this is the one control in the application that cannot be undone. Two clicks
 * away, behind a gear, on a panel that has already named the account being
 * signed out of, is the right distance.
 */
export function MyAccountSection({ profile, onSignedOut }: Props) {
  const [signingOut, setSigningOut] = useState(false);
  const name = profile.display_name ?? profile.user_id;

  async function signOut() {
    setSigningOut(true);
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
    <div className="account">
      <div className="account__card">
        <div className="account__avatar" aria-hidden="true">
          {initialsOf(name)}
        </div>
        <p className="account__name">{name}</p>

        <dl className="account__facts">
          <div>
            <dt>User ID</dt>
            <dd data-selectable>{profile.user_id}</dd>
          </div>
          <div>
            <dt>Device</dt>
            <dd data-selectable>{profile.device_id}</dd>
          </div>
          <div>
            <dt>Homeserver</dt>
            <dd data-selectable>{profile.homeserver}</dd>
          </div>
        </dl>
      </div>

      <div className="account__danger">
        <button
          className="button button--danger"
          onClick={() => void signOut()}
          disabled={signingOut}
        >
          {signingOut ? "Signing out…" : "Log Out"}
        </button>
        <p className="account__note">
          Signs out of this device only. Your other sessions stay where they
          are.
        </p>
      </div>
    </div>
  );
}
