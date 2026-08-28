import { useEffect, useState } from "react";

import {
  asCommandError,
  callConnect,
  callDisconnect,
  callSetDeafened,
  callSetMuted,
  HEARING,
  onCall,
  onAudio,
  onSelfAudio,
  onSpeaking,
  onConnection,
  onKeyBackup,
  onRooms,
  onVerification,
  onVerificationFlow,
  resendState,
  tokenStorage,
  type Call,
  type Connection,
  type KeyBackup,
  type Profile,
  type Rooms,
  type SelfAudio,
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
  // Empty rather than absent, which is what an account in no rooms looks like
  // too. There is no third state to render: the shell draws whatever it has,
  // and what it has before the first report is nothing.
  const [rooms, setRooms] = useState<Rooms>({ spaces: [] });
  // Keyed by flow id rather than a single slot. A request goes to every device
  // on the account and two can arrive, and one slot would silently drop the
  // second, leaving somebody waiting on a device that will never answer.
  const [flows, setFlows] = useState<Record<string, VerificationFlow>>({});
  // Not in a channel, which is what a fresh process is. A webview that
  // reloaded mid-call is corrected by `resendState` below, which is why this
  // channel is one of the ones the Rust side keeps.
  const [call, setCall] = useState<Call>({ state: "disconnected" });
  // Starts where Rust starts. The two only ever meet through `onSelfAudio`,
  // which says nothing until something changes, so agreeing on the opening
  // value is what makes silence mean "neither" rather than "unknown".
  const [selfAudio, setSelfAudio] = useState<SelfAudio>(HEARING);
  // Who is talking, by user id. A `Set` rather than an array because the only
  // question ever asked of it is whether one name is in it, once per person
  // drawn, several times a second.
  const [speaking, setSpeaking] = useState<ReadonlySet<string>>(
    () => new Set(),
  );
  // Why the call cannot be played, when it cannot. Held here rather than in
  // the call panel because the panel unmounts between calls and this arrives
  // as the call is starting, which is exactly when it would be missed.
  const [audioProblem, setAudioProblem] = useState<string | null>(null);

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
      onRooms((tree) => {
        // Assigned, never merged. The whole tree arrives every time any part
        // of it changes, which is what stops this copy drifting away from the
        // account it is meant to describe.
        if (!cancelled) setRooms(tree);
      }).then(keep),
      onCall((state) => {
        if (!cancelled) setCall(state);
      }).then(keep),
      onSelfAudio((audio) => {
        if (!cancelled) setSelfAudio(audio);
      }).then(keep),
      onSpeaking((userIds) => {
        if (!cancelled) setSpeaking(new Set(userIds));
      }).then(keep),
      // Only the call's own output, out of a channel that also carries the
      // settings screen's microphone test and its level readings. A failed
      // chime is the settings screen's business and is already drawn there.
      onAudio((activity) => {
        if (cancelled) return;
        if (activity.state === "callAudioFailed") {
          setAudioProblem(
            `Consort cannot play this call: ${activity.error}. Nobody in it will be audible until an output device is available.`,
          );
        } else if (
          activity.state === "callAudioStarted" ||
          activity.state === "callAudioStopped"
        ) {
          setAudioProblem(null);
        }
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

  /**
   * Join a voice channel.
   *
   * Nothing is set here. Every state this screen shows comes back through
   * `onCall`, including the failure, so that what is on screen is what the
   * call thread actually did rather than what was asked of it. The one thing
   * that rejects is asking while signed out, which cannot happen from this
   * screen and is logged rather than drawn.
   */
  function joinVoice(roomId: string) {
    callConnect(roomId).catch((raw: unknown) => {
      console.error("could not ask to join the call", asCommandError(raw).detail);
    });
  }

  function leaveVoice() {
    callDisconnect().catch((raw: unknown) => {
      console.error("could not ask to leave the call", asCommandError(raw).detail);
    });
  }

  /**
   * Mute, unmute, deafen or undeafen.
   *
   * Nothing is set here either, for the reason `joinVoice` sets nothing: the
   * button reflects what the call thread did. A press that never reached it
   * leaves the button where it was, which is the truth.
   */
  function setMuted(muted: boolean) {
    callSetMuted(muted).catch((raw: unknown) => {
      console.error("could not ask to mute", asCommandError(raw).detail);
    });
  }

  function setDeafened(deafened: boolean) {
    callSetDeafened(deafened).catch((raw: unknown) => {
      console.error("could not ask to deafen", asCommandError(raw).detail);
    });
  }

  const running = Object.values(flows);

  return (
    <AppShell
      profile={profile}
      rooms={rooms}
      connection={connection}
      call={call}
      verification={verification}
      keyBackup={keyBackup}
      storage={storage}
      flows={running}
      canStartVerification={!running.some(isRunning)}
      onDismissFlow={dismiss}
      selfAudio={selfAudio}
      speaking={speaking}
      audioProblem={audioProblem}
      onJoinVoice={joinVoice}
      onLeaveVoice={leaveVoice}
      onSetMuted={setMuted}
      onSetDeafened={setDeafened}
      onSignedOut={onSignedOut}
    />
  );
}
