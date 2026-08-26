import { useEffect, useState } from "react";

import {
  asCommandError,
  onConnection,
  onKeyBackup,
  onVerification,
  onVerificationFlow,
  resendState,
  tokenStorage,
  type Connection,
  type KeyBackup,
  type Profile,
  type TokenStorage,
  type Verification,
  type VerificationFlow,
} from "../lib/api";
import { AppShell } from "./AppShell";

/** Whether a flow is still going, and so worth not starting a second one beside. */
function isRunning(flow: VerificationFlow): boolean {
  return flow.state.kind !== "done" && flow.state.kind !== "cancelled";
}

interface Props {
  profile: Profile;
  onSignedOut: () => void;
}

/**
 * Everything the signed-in screen knows, and nothing it draws.
 *
 * The split is deliberate. This owns the subscriptions, which are the part
 * with a lifetime to get wrong: a listener that outlives its component keeps
 * handling events into unmounted state, and after a sign out and a sign in
 * every event arrives once per leaked listener. `AppShell` owns the layout,
 * which is the part that changes every time the design does. Keeping them
 * apart means a layout change cannot quietly break a listener.
 */
export function SignedIn({ profile, onSignedOut }: Props) {
  const [signingOut, setSigningOut] = useState(false);
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
  // Starts at `unknown` for the same reason as the two above. Every other
  // value is a claim about whether somebody's messages survive this machine.
  const [keyBackup, setKeyBackup] = useState<KeyBackup>({ state: "unknown" });
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
      onKeyBackup((state) => {
        if (!cancelled) setKeyBackup(state);
      }).then(keep),
    ]);

    listening
      // Only once every listener is attached, and only if this screen is still
      // here. Asking earlier would be answered into the void, which is the
      // exact race it exists to close.
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

  const running = Object.values(flows);

  return (
    <AppShell
      profile={profile}
      connection={connection}
      verification={verification}
      keyBackup={keyBackup}
      storage={storage}
      flows={running}
      canStartVerification={!running.some(isRunning)}
      signingOut={signingOut}
      onDismissFlow={dismiss}
      onSigningOut={() => setSigningOut(true)}
      onSignedOut={onSignedOut}
    />
  );
}
