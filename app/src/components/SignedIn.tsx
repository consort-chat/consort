import { useEffect, useState } from "react";

import {
  asCommandError,
  logout,
  onConnection,
  onVerification,
  onVerificationFlow,
  resendState,
  tokenStorage,
  verificationOtherSessionsExist,
  verificationRecoveryExists,
  verificationVerifyThisSession,
  type Connection,
  type Profile,
  type TokenStorage,
  type Verification,
  type VerificationFlow,
} from "../lib/api";
import { RecoveryKeyForm } from "./RecoveryKey";
import { VerificationFlowPanel } from "./VerificationFlow";
import "./SignedIn.css";

/**
 * One short phrase per state, for the header.
 *
 * A stopped loop is the only case that does not imply a message might still
 * arrive, and a session the homeserver has rejected is the only one the user
 * has to do something about, so those two do not share a label.
 */
function connectionLabel(connection: Connection): string {
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

/**
 * What to say about this session's verification, and why it matters.
 *
 * `unknown` gets its own sentence rather than borrowing either of the others.
 * It is a real state, it is the one every launch starts in, and rendering it
 * as "verified" would tell somebody their messages are safe before anything
 * had checked.
 */
function VerificationBanner({
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

  const headline =
    state === "verified"
      ? "This session is verified."
      : state === "unverified"
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

/** Whether a flow is still going, and so worth not starting a second one beside. */
function isRunning(flow: VerificationFlow): boolean {
  return flow.state.kind !== "done" && flow.state.kind !== "cancelled";
}

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
  // Not `live`. Claiming a connection before the sync loop has said anything
  // is the lie this replaced: the old header was the literal string
  // "Connected", written into the markup and true only by coincidence.
  const [connection, setConnection] = useState<Connection>({
    state: "connecting",
  });
  // Same reasoning, and it matters more here. Starting at `verified` would be
  // a claim about somebody's messages made before anything had looked.
  const [verification, setVerification] = useState<Verification>({
    state: "unknown",
  });
  // Keyed by flow id rather than a single slot. A request goes to every device
  // on the account and two can arrive, and one slot would silently drop the
  // second, leaving somebody waiting on a device that will never answer.
  const [flows, setFlows] = useState<Record<string, VerificationFlow>>({});

  function dismiss(flowId: string) {
    setFlows((current) => {
      const { [flowId]: _gone, ...rest } = current;
      return rest;
    });
  }

  useEffect(() => {
    let cancelled = false;
    const stops: Array<() => void> = [];

    // Subscribing is asynchronous, so a listener can be handed over after the
    // screen is gone. Stopping it straight away is the difference between one
    // that ends with the component and one that lives as long as the process,
    // handling events into unmounted state and duplicating every event once
    // per sign-in.
    function keep(stop: () => void) {
      if (cancelled) stop();
      else stops.push(stop);
    }

    const listening = Promise.all([
      onConnection((state) => {
        if (!cancelled) setConnection(state);
      }).then(keep),
      onVerification((state) => {
        if (!cancelled) setVerification(state);
      }).then(keep),
      onVerificationFlow((flow) => {
        if (!cancelled) {
          setFlows((current) => ({ ...current, [flow.flowId]: flow }));
        }
      }).then(keep),
    ]);

    listening
      // Only once both listeners are attached, and only if this screen is
      // still here. Asking earlier would be answered into the void, which is
      // the exact race it exists to close.
      .then(() => (cancelled ? undefined : resendState()))
      .catch((raw: unknown) => {
        // Handled at subscribe time rather than in the cleanup below, because
        // a rejection nobody has attached to yet is an unhandled rejection for
        // as long as this screen is open. Cosmetic in effect: the banners stay
        // on their initial states, neither of which claims anything works.
        console.error(
          "could not follow the session state",
          asCommandError(raw).detail,
        );
      });

    return () => {
      cancelled = true;
      for (const stop of stops) stop();
    };
  }, []);

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
        <span
          className="signed-in__status"
          data-connection={connection.state}
          aria-live="polite"
        >
          <i className="signed-in__dot" aria-hidden="true" />
          {connectionLabel(connection)}
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
          <p
            className="signed-in__notice"
            role="status"
            aria-label="Where your sign-in is stored"
          >
            {storage.description}
          </p>
        )}

        {Object.values(flows).map((flow) => (
          <VerificationFlowPanel
            key={flow.flowId}
            flow={flow}
            onDismiss={() => dismiss(flow.flowId)}
          />
        ))}

        <VerificationBanner
          state={verification.state}
          canStart={!Object.values(flows).some(isRunning)}
        />
      </main>
    </div>
  );
}
