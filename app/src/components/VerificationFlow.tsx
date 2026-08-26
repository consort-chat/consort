import { useState } from "react";

import {
  asCommandError,
  verificationAccept,
  verificationCancel,
  verificationConfirm,
  verificationMismatch,
  verificationStartSas,
  type CancelReason,
  type VerificationFlow,
  type VerificationFlowState,
} from "../lib/api";
import "./VerificationFlow.css";

/**
 * What to say about a flow that ended without verifying anything.
 *
 * The SDK hands over its own sentence for each of these and none of them is
 * rendered. "The SAS did not match." is written for whoever is reading a log,
 * and a person who has just pressed a button deserves a sentence about what
 * they should do next.
 *
 * `alarming` is the difference that matters. A mismatch is the answer the
 * whole comparison exists to produce, and it is the only one of these that
 * means somebody might be listening. Everything else is somebody changing
 * their mind, running out of time, or another of their own devices getting
 * there first, and dressing those in the same red is how a warning stops
 * meaning anything.
 */
function endingFor(
  reason: CancelReason,
  byUs: boolean,
): { text: string; alarming: boolean } {
  switch (reason) {
    case "mismatch":
      return {
        text: "The emoji did not match, so nothing was verified. If you are sure you compared them correctly, somebody may be intercepting the connection.",
        alarming: true,
      };
    case "declined":
      return {
        text: byUs
          ? "You declined the verification. This session is still unverified."
          : "The other session declined the verification.",
        alarming: false,
      };
    case "timedOut":
      return {
        text: "The verification expired before both sides answered. Start it again when you are ready.",
        alarming: false,
      };
    case "acceptedElsewhere":
      return {
        text: "Another of your sessions answered this request, so there is nothing to do here.",
        alarming: false,
      };
    case "other":
      return {
        text: "The verification ended before it finished. Start it again to try once more.",
        alarming: false,
      };
  }
}

/** One picture and the word for it. */
function Emoji({ symbol, description }: { symbol: string; description: string }) {
  return (
    <li className="flow__emoji">
      {/*
        An emoji is a text node with no accessible name, so a screen reader
        reads either nothing or the vendor's own description, which is not the
        word the other device is showing. The word below it is the one both
        people are comparing, so that is the name.
      */}
      <span className="flow__symbol" role="img" aria-label={description}>
        {symbol}
      </span>
      <span className="flow__word">{description}</span>
    </li>
  );
}

interface Props {
  flow: VerificationFlow;
  /** Clear a flow that is over. Local to the interface; nothing is sent. */
  onDismiss: () => void;
}

/**
 * One verification, from the request through to whatever became of it.
 *
 * Every action names the flow rather than the panel holding a handle to it.
 * That is not ceremony: a request goes to every device on the account and two
 * of them can answer, so a flow is something you address rather than something
 * anybody owns.
 */
export function VerificationFlowPanel({ flow, onDismiss }: Props) {
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  /**
   * Run one action, and say so when it does not work.
   *
   * Rejecting is the ordinary case rather than the exceptional one: flows
   * expire after ten minutes, either side can cancel, and another session may
   * have answered between this button being drawn and being pressed. A button
   * that silently does nothing is the worst of the available outcomes.
   */
  function act(run: (userId: string, flowId: string) => Promise<void>) {
    return () => {
      setPending(true);
      setFailure(null);
      run(flow.otherUserId, flow.flowId)
        .catch((raw: unknown) => {
          const error = asCommandError(raw);
          console.error("a verification action failed", error.detail);
          setFailure(error.message);
        })
        .finally(() => setPending(false));
    };
  }

  const who = flow.isSelfVerification
    ? "Another of your sessions"
    : flow.otherUserId;
  const state = flow.state;
  const over = state.kind === "done" || state.kind === "cancelled";
  const ending =
    state.kind === "cancelled"
      ? endingFor(state.reason, state.byUs)
      : undefined;

  return (
    <section
      className="flow"
      role="status"
      aria-live="polite"
      aria-label="Session verification request"
      data-kind={state.kind}
      data-outcome={ending?.alarming ? "alarming" : undefined}
    >
      {state.kind === "requested" && (
        <>
          <p className="flow__headline">
            {who} wants to verify this one.
          </p>
          <p className="flow__detail">
            Accepting will show seven pictures on both. Compare them, and if
            they are the same, this session becomes verified.
          </p>
          <div className="flow__actions">
            <button
              className="button button--primary button--small"
              onClick={act(verificationAccept)}
              disabled={pending}
            >
              Verify
            </button>
            <button
              className="button button--ghost button--small"
              onClick={act(verificationCancel)}
              disabled={pending}
            >
              Not now
            </button>
          </div>
        </>
      )}

      {state.kind === "ready" && (
        <>
          <p className="flow__headline">Waiting for the pictures.</p>
          <p className="flow__detail">
            {flow.weStarted
              ? "Both sessions are ready. The pictures are on their way."
              : "The other session normally shows them straight away. If nothing happens, start it from here."}
          </p>
          <div className="flow__actions">
            {/*
              Only as the responder. When this session asked, the Rust side
              sends the start itself as soon as both sides are ready, so this
              button would be a second one.
            */}
            {!flow.weStarted && (
              <button
                className="button button--primary button--small"
                onClick={act(verificationStartSas)}
                disabled={pending}
              >
                Show the emoji
              </button>
            )}
            <button
              className="button button--ghost button--small"
              onClick={act(verificationCancel)}
              disabled={pending}
            >
              Cancel
            </button>
          </div>
        </>
      )}

      {state.kind === "waiting" && (
        <>
          <p className="flow__headline">
            {flow.weStarted
              ? "Waiting for your other session."
              : "Waiting for the pictures."}
          </p>
          <p className="flow__detail">
            {flow.weStarted
              ? /*
                  Where the person has to go, not just that they are waiting.
                  The request is sitting on their other device and nothing here
                  can move it along.
                */
                "Open the app on your other session and accept the request there."
              : "The two sessions are agreeing on how to compare. This takes a moment."}
          </p>
          <div className="flow__actions">
            <button
              className="button button--ghost button--small"
              onClick={act(verificationCancel)}
              disabled={pending}
            >
              Cancel
            </button>
          </div>
        </>
      )}

      {state.kind === "comparing" && (
        <>
          <p className="flow__headline">Do these match the other session?</p>
          {state.emoji.length > 0 ? (
            <ul className="flow__emoji-row">
              {state.emoji.map((pair, at) => (
                <Emoji key={`${pair.symbol}-${at}`} {...pair} />
              ))}
            </ul>
          ) : (
            /*
              `supports_emoji()` really can be false, and the decimals are the
              spec's own fallback rather than a degraded mode. Three numbers
              compare exactly as well as seven pictures.
            */
            <p className="flow__decimals">{state.decimals.join(" ")}</p>
          )}
          <div className="flow__actions">
            <button
              className="button button--primary button--small"
              onClick={act(verificationConfirm)}
              disabled={pending}
            >
              They match
            </button>
            <button
              className="button button--ghost button--small"
              onClick={act(verificationMismatch)}
              disabled={pending}
            >
              They do not match
            </button>
          </div>
        </>
      )}

      {state.kind === "confirmed" && (
        <>
          <p className="flow__headline">Waiting for the other session.</p>
          <p className="flow__detail">
            You have said they match. This finishes once the other one does
            too.
          </p>
        </>
      )}

      {state.kind === "done" && (
        <p className="flow__headline">
          This session is verified. Encrypted history will open here, and
          encrypted calls will accept it.
        </p>
      )}

      {ending && <p className="flow__headline">{ending.text}</p>}

      {failure !== null && <p className="flow__failure">{failure}</p>}

      {over && (
        <div className="flow__actions">
          <button className="button button--ghost button--small" onClick={onDismiss}>
            Dismiss
          </button>
        </div>
      )}
    </section>
  );
}

/** Re-exported so callers do not need two imports to name one thing. */
export type { VerificationFlow, VerificationFlowState };
