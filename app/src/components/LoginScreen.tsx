import { useState, type FormEvent } from "react";

import { asCommandError, login, type Profile } from "../lib/api";
import "./LoginScreen.css";

interface Props {
  onSignedIn: (profile: Profile) => void;
}

/**
 * Password login against a homeserver.
 *
 * The server field takes a bare server name because that is what people know
 * about themselves ("I'm on lamp.stream"), not a homeserver URL. Resolving it
 * is the SDK's job via `.well-known`, so the form does not ask the user to
 * know the difference.
 */
export function LoginScreen({ onSignedIn }: Props) {
  const [server, setServer] = useState("lamp.stream");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const canSubmit =
    !pending && server.trim() !== "" && username.trim() !== "" && password !== "";

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSubmit) return;

    setPending(true);
    setError(null);
    try {
      const profile = await login(server, username, password);
      // Clear the password from component state on the way out. It is a small
      // window and React will drop it anyway, but leaving a credential sitting
      // in a live component while the next screen mounts is avoidable.
      setPassword("");
      onSignedIn(profile);
    } catch (raw: unknown) {
      const commandError = asCommandError(raw);
      console.error("login failed", commandError.detail);
      setError(commandError.message);
      setPending(false);
    }
  }

  return (
    <div className="login">
      <aside className="login__brand">
        <div className="login__mark" aria-hidden="true">
          <span className="login__mark-c" />
          <span className="login__bars">
            <i style={{ height: "22%" }} />
            <i style={{ height: "52%" }} />
            <i style={{ height: "30%" }} />
          </span>
        </div>
        <h1 className="login__wordmark">Consort</h1>
        <p className="login__tagline">
          A desktop client for Matrix, with voice that stays out of your way.
        </p>
        <p className="login__footnote">
          Free software under the AGPL, version&nbsp;3.
        </p>
      </aside>

      <main className="login__panel">
        <form className="login__form" onSubmit={handleSubmit} noValidate>
          <header className="login__header">
            <h2>Sign in</h2>
            <p>Use your Matrix account.</p>
          </header>

          <label className="field">
            <span className="field__label">Homeserver</span>
            <input
              className="field__input"
              type="text"
              value={server}
              onChange={(event) => setServer(event.target.value)}
              autoComplete="url"
              spellCheck={false}
              disabled={pending}
              placeholder="lamp.stream"
            />
            <span className="field__hint">
              The server your account lives on, not a web address.
            </span>
          </label>

          <label className="field">
            <span className="field__label">Username</span>
            <input
              className="field__input"
              type="text"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              autoComplete="username"
              spellCheck={false}
              disabled={pending}
              placeholder="bob"
              autoFocus
            />
          </label>

          <label className="field">
            <span className="field__label">Password</span>
            <input
              className="field__input"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete="current-password"
              disabled={pending}
            />
          </label>

          {error !== null && (
            <p className="login__error" role="alert">
              {error}
            </p>
          )}

          <button className="button button--primary" type="submit" disabled={!canSubmit}>
            {pending ? "Signing in…" : "Sign in"}
          </button>
        </form>
      </main>
    </div>
  );
}
