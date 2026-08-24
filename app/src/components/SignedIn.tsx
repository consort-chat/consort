import { useEffect, useState } from "react";

import {
  asCommandError,
  logout,
  tokenStorage,
  type Profile,
  type TokenStorage,
} from "../lib/api";
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
  const [storage, setStorage] = useState<TokenStorage | null>(null);

  useEffect(() => {
    let cancelled = false;

    tokenStorage()
      .then((value) => {
        if (!cancelled) setStorage(value);
      })
      .catch((raw: unknown) => {
        // Cosmetic. Not knowing where the token is kept is no reason to
        // interrupt someone who is already signed in.
        console.error("token_storage failed", asCommandError(raw).detail);
      });

    return () => {
      cancelled = true;
    };
  }, []);

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

        {/*
          Shown only when we had to fall back. Storing the token in the system
          keyring is the expected case and does not need announcing; storing it
          in a file is a real, if small, reduction in protection and the person
          it affects should be the one who knows about it.
        */}
        {storage !== null && !storage.isPreferred && (
          <p className="signed-in__notice" role="status">
            {storage.description}
          </p>
        )}

        <p className="signed-in__next">
          Authentication works. Session verification is next.
        </p>
      </main>
    </div>
  );
}
