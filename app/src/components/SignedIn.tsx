import { useState } from "react";

import { asCommandError, logout, type Profile } from "../lib/api";
import "./SignedIn.css";

interface Props {
  profile: Profile;
  onSignedOut: () => void;
}

/**
 * What you get after signing in, for now.
 *
 * This is the placeholder the room list and voice channels replace. It is not
 * decorative: it prints the device ID, which is the value you need when
 * checking cross-signing state or matching a session in the homeserver's
 * device list, and that is exactly what the next milestone will be debugging.
 */
export function SignedIn({ profile, onSignedOut }: Props) {
  const [pending, setPending] = useState(false);

  async function handleLogout() {
    setPending(true);
    try {
      await logout();
    } catch (raw: unknown) {
      // `logout` clears the local session even when the server call fails, so
      // there is no state where staying on this screen is correct.
      console.error("logout reported an error", asCommandError(raw).detail);
    }
    onSignedOut();
  }

  const name = profile.display_name ?? profile.user_id;
  const initial = name.replace(/^@/, "").charAt(0).toUpperCase();

  return (
    <div className="signed-in">
      <header className="signed-in__bar">
        <span className="signed-in__status">
          <i className="signed-in__dot" aria-hidden="true" />
          Connected
        </span>
        <button
          className="button button--ghost button--small"
          onClick={handleLogout}
          disabled={pending}
        >
          {pending ? "Signing out…" : "Sign out"}
        </button>
      </header>

      <main className="signed-in__body">
        <div className="signed-in__avatar" aria-hidden="true">
          {initial}
        </div>
        <h1 className="signed-in__name">{name}</h1>

        <dl className="signed-in__facts">
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

        <p className="signed-in__next">
          Authentication works. Voice channels are next.
        </p>
      </main>
    </div>
  );
}
