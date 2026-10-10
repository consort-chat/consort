import { useEffect, useState } from "react";

import {
  asCommandError,
  dismissVerificationWarning,
  verificationOtherSessionsExist,
  verificationRecoveryExists,
  verificationVerifyThisSession,
  verificationWarningDismissed,
  type Verification,
} from "../lib/api";
import { Confirm } from "./Confirm";
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
 *
 * The same reasoning is why #59 shrinks this rather than closing it. Both
 * routes out need something somebody may not have, a second device or a key
 * they kept, so for an account with neither this was a paragraph with nothing
 * on the far side of it. Agreed to, it becomes one line and the way back,
 * which keeps the fact on screen without the wall nobody can act on.
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
  /**
   * Whether this session has already been told what staying unverified costs.
   *
   * `null` until the answer is back, and nothing below the headline is drawn
   * until then. The headline is true either way, so it goes up immediately;
   * offering a choice before knowing whether it has already been made would
   * flash a wall at somebody who settled this weeks ago.
   */
  const [dismissed, setDismissed] = useState<boolean | null>(null);
  /** Asked for again from the short form. Local, and spent on a remount. */
  const [expanded, setExpanded] = useState(false);
  /** Whether the question in front of agreeing is open. */
  const [asking, setAsking] = useState(false);

  /** Whether the routes and the cost are on screen rather than one line. */
  const long = dismissed === false || expanded;

  useEffect(() => {
    if (state !== "unverified") return;

    let cancelled = false;
    verificationWarningDismissed()
      .then((answer) => {
        if (!cancelled) setDismissed(answer);
      })
      .catch((raw: unknown) => {
        console.error(
          "could not find out whether this session has answered already",
          asCommandError(raw).detail,
        );
        // Fail safe, and the one direction here that matters. An unreadable
        // settings file must not be able to silence this.
        if (!cancelled) setDismissed(false);
      });

    return () => {
      cancelled = true;
    };
  }, [state]);

  useEffect(() => {
    if (state !== "unverified" || !long) return;

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
  }, [state, long]);

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

  /**
   * Agree to carry on unverified, and shrink to the one line that stays true.
   *
   * Written down before it is drawn, which is the same rule the privacy screen
   * follows: a banner that shrank on the press and came back at the next
   * launch would look like the choice had been forgotten rather than never
   * recorded. A save that fails leaves the warning in full and says why.
   */
  async function carryOn() {
    setAsking(false);
    setFailure(null);
    try {
      await dismissVerificationWarning();
      setDismissed(true);
      setExpanded(false);
    } catch (raw: unknown) {
      const error = asCommandError(raw);
      console.error("could not record the answer", error.detail);
      setFailure(error.message);
    }
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
      {state === "unverified" && long && (
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
        </>
      )}

      {state === "unverified" && failure !== null && (
        <p className="verification__failure">{failure}</p>
      )}

      {/*
        Held back until the answer is in. Either half drawn on a guess is the
        wrong half for half of the launches it is drawn on.
      */}
      {state === "unverified" &&
        dismissed !== null &&
        (long ? (
          <div className="verification__anchor">
            <button
              type="button"
              className="button button--ghost button--small"
              aria-haspopup="dialog"
              aria-expanded={asking}
              onClick={() => setAsking((open) => !open)}
            >
              Continue without verifying
            </button>
            {asking && (
              <Confirm
                question="Carry on without verifying this session?"
                detail="Messages that were encrypted before you signed in stay unreadable here. Encrypted voice channels will refuse this session, so nobody in one will be able to hear you. And other people's clients will go on warning about anything you send from this device. Nothing is deleted, and you can still verify later."
                go="Continue unverified"
                onConfirm={() => void carryOn()}
                onCancel={() => setAsking(false)}
              />
            )}
          </div>
        ) : (
          /*
            One line and the way back. Expanding is local rather than a second
            write: somebody looking at the routes again has not changed their
            mind about the answer they already gave, and making them give it
            twice is the nag this exists to end.
          */
          <div className="verification__actions">
            <button
              type="button"
              className="button button--ghost button--small"
              onClick={() => setExpanded(true)}
            >
              Verify
            </button>
          </div>
        ))}
    </section>
  );
}
