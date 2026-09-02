import { useEffect, useState } from "react";

import {
  asCommandError,
  verificationOtherSessionsExist,
  verificationRecoveryExists,
  verificationVerifyThisSession,
  type Verification,
} from "../lib/api";
import { RecoveryKeyForm } from "./RecoveryKey";
import "./VerificationBanner.css";

/**
 * What to say about this session's verification, and why it matters.
 *
 * `unknown` gets its own sentence rather than borrowing either of the others.
 * It is a real state, it is the one every launch starts in, and rendering it
 * as "verified" would tell somebody their messages are safe before anything
 * had checked.
 *
 * This lives in the main pane rather than in the user panel at the bottom of
 * the channel list, and that is deliberate. The panel is sixty pixels tall,
 * and this is the one piece of the interface that tells somebody their
 * messages cannot be decrypted. It does not get folded into a corner because
 * the layout has one.
 *
 * A verified session draws nothing. This is a warning, and a warning that is
 * permanently on screen saying everything is fine is a strip of the window
 * somebody learns to skip, which is the strip the real warning has to appear
 * in. `unknown` still speaks, because it is not the same claim: it is the
 * launch state, it says only that nothing has looked yet, and going quiet for
 * it would render "not known" as "fine".
 */
export function VerificationBanner({
  state,
  canStart,
}: {
  state: Verification["state"];
  canStart: boolean;
}) {
  /**
   * Whether the account has another session to compare emoji with, and whether
   * it has a recovery key to type. Two questions, two routes, and a session
   * with neither is a dead end that has to be said out loud.
   *
   * `null` while nobody has asked or the answer has not come back. Rendering
   * either concrete answer during that gap would flicker between two different
   * pieces of advice, so nothing is offered until both have landed.
   */
  const [others, setOthers] = useState<boolean | null>(null);
  const [recovery, setRecovery] = useState<boolean | null>(null);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    if (state !== "unverified") return;

    let cancelled = false;
    verificationOtherSessionsExist()
      .then((exists) => {
        if (!cancelled) setOthers(exists);
      })
      .catch((raw: unknown) => {
        console.error(
          "could not count the account's other sessions",
          asCommandError(raw).detail,
        );
        // Fail open. Being wrong this way costs one request nobody answers.
        // Being wrong the other way tells somebody with a phone signed in that
        // their only route is a recovery key, which they may never have kept.
        if (!cancelled) setOthers(true);
      });

    verificationRecoveryExists()
      .then((exists) => {
        if (!cancelled) setRecovery(exists);
      })
      .catch((raw: unknown) => {
        console.error(
          "could not find out whether the account has a recovery key",
          asCommandError(raw).detail,
        );
        // Open here too, and for the same shape of reason. A box offered to
        // somebody with no key costs them one attempt and a clear answer;
        // hiding it from somebody who has one leaves a lone session with no
        // way out at all.
        if (!cancelled) setRecovery(true);
      });

    return () => {
      cancelled = true;
    };
  }, [state]);

  function start() {
    setPending(true);
    setFailure(null);
    verificationVerifyThisSession()
      .catch((raw: unknown) => {
        const error = asCommandError(raw);
        console.error("could not start a verification", error.detail);
        setFailure(error.message);
      })
      .finally(() => setPending(false));
  }

  if (state === "verified") return null;

  const headline =
    state === "unverified"
      ? "This session is not verified."
      : "Checking whether this session is verified.";

  return (
    <section
      className="verification"
      data-verification={state}
      role="status"
      aria-live="polite"
      aria-label="Session verification"
    >
      <p className="verification__headline">{headline}</p>
      {state === "unverified" && (
        <>
          <p className="verification__detail">
            Messages encrypted before you signed in will not open here, and
            encrypted calls will not accept this device.
          </p>
          {others !== null && recovery !== null && (
            <>
              {others === false && recovery === false ? (
                /*
                  The honest dead end, and it is a real one. A lone session has
                  nobody to compare pictures with, and an account with no
                  secret storage has no key to type instead. Saying so beats a
                  button that can only spend ten minutes arriving at the same
                  answer.
                */
                <p className="verification__detail">
                  No other session is signed in and this account has no
                  recovery key, so there is nothing to verify against yet. Sign
                  in on another device, or set a recovery key up from a client
                  that has one.
                </p>
              ) : (
                <>
                  {others && (
                    <>
                      {canStart && (
                        <div className="verification__actions">
                          <button
                            className="button button--primary button--small"
                            onClick={start}
                            disabled={pending}
                          >
                            Verify this session
                          </button>
                        </div>
                      )}
                      {/*
                        Kept even while a flow is running. Asking from the
                        other end works just as well, and somebody whose
                        request is sitting unanswered on a device they cannot
                        reach should know the other direction exists.
                      */}
                      <p className="verification__detail">
                        You can also start one from a client you are already
                        signed in to, and the request will appear above.
                      </p>
                    </>
                  )}
                  {recovery && <RecoveryKeyForm soleRoute={!others} />}
                </>
              )}
            </>
          )}
          {failure !== null && (
            <p className="verification__failure">{failure}</p>
          )}
        </>
      )}
    </section>
  );
}
