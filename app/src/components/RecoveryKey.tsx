import { useState, type FormEvent } from "react";

import { asCommandError, verificationRecover } from "../lib/api";
import "./RecoveryKey.css";

/**
 * The second route to a verified session: type the account's recovery key.
 *
 * Emoji needs a second session online at the same moment and a person looking
 * at two screens. This needs a string, which is what somebody installing
 * Consort on their only machine actually has, so it is the difference between
 * a client a stranger can use and one only somebody with Element already
 * running can.
 *
 * Nothing is reported upwards on success. The Rust side signs this device with
 * the keys that come back, the verification watcher notices and publishes
 * `verified`, and the banner this sits inside stops rendering. Wiring a
 * callback for that would be a second source of truth for the same fact.
 */
export function RecoveryKeyForm({ soleRoute }: { soleRoute: boolean }) {
  const [key, setKey] = useState("");
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  function submit(event: FormEvent) {
    event.preventDefault();
    setPending(true);
    setFailure(null);

    verificationRecover(key)
      // Held no longer than it takes to use. The Rust side has already handed
      // it to the SDK and dropped it, and a verified session has no further
      // use for the key that verified it.
      .then(() => setKey(""))
      .catch((raw: unknown) => {
        const error = asCommandError(raw);
        // Safe to log: the Rust side's error text names what went wrong and
        // never quotes what was typed, which is asserted on that side.
        console.error("could not verify with a recovery key", error.detail);
        setFailure(error.message);
      })
      .finally(() => setPending(false));
  }

  return (
    <form className="recovery" onSubmit={submit}>
      <label className="recovery__label" htmlFor="recovery-key">
        {soleRoute ? "Recovery key" : "Or use your recovery key"}
      </label>
      <div className="recovery__row">
        <input
          id="recovery-key"
          className="field__input recovery__input"
          /*
            Not a password field. This is 48 characters of base58 typed or
            pasted once, and hiding it means every mistake is found by the
            homeserver instead of by the person making it. Element shows it in
            the clear for the same reason.
          */
          type="text"
          value={key}
          onChange={(event) => setKey(event.target.value)}
          disabled={pending}
          autoComplete="off"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
        <button
          className="button button--primary button--small"
          type="submit"
          disabled={pending || key.trim() === ""}
        >
          {pending ? "Checking…" : "Verify"}
        </button>
      </div>
      <p className="recovery__hint">
        Forty-eight characters, usually shown in groups of four. If you set a
        passphrase instead, that works here too.
      </p>
      {failure !== null && <p className="verification__failure">{failure}</p>}
    </form>
  );
}
