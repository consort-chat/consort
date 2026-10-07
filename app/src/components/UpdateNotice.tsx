import { useCallback, useEffect, useState } from "react";

import {
  asCommandError,
  onCall,
  onUpdate,
  resendState,
  updateInstall,
  updatesItself,
  type Update,
} from "../lib/api";
import { UpdateBar } from "./UpdateBar";

/**
 * Everything the update bar needs, and nothing above it has to know about.
 *
 * Mounted beside the three views rather than inside the signed-in shell,
 * because a newer Consort is worth offering on the sign-in screen too. It asks
 * first whether this build updates itself at all: a .deb and an Arch package do
 * not, and in those it subscribes to nothing and draws nothing.
 */
export function UpdateNotice() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [inACall, setInACall] = useState(false);
  const [dismissed, setDismissed] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const stops: (() => void)[] = [];

    void (async () => {
      try {
        if (!(await updatesItself()) || cancelled) return;

        stops.push(
          await onUpdate((next) =>
            setUpdate((current) =>
              // A six-hourly look that could not reach GitHub says nothing about
              // whether a release exists. It still does, so the offer stands and
              // the next look is the retry.
              next.state === "failed" && current?.state === "ready"
                ? current
                : next,
            ),
          ),
        );
        stops.push(await onCall((call) => setInACall(call.state !== "disconnected")));
        // The poll starts with the process, so its first answer was published
        // before this page ran a line. See CLAUDE.md on state channels.
        await resendState();
      } catch (error: unknown) {
        // Nothing to put in front of anybody: the bar simply stays undrawn,
        // which is what a build with no updater looks like too.
        console.error("the updater could not be reached", asCommandError(error));
      }
    })();

    return () => {
      cancelled = true;
      for (const stop of stops) stop();
    };
  }, []);

  const install = useCallback(() => {
    updateInstall().catch((error: unknown) => {
      // Rust checks the call again once the bytes are down, so this is where a
      // refusal lands even when the button looked pressable. Render what it
      // said rather than a sentence of our own.
      setUpdate({ state: "failed", reason: asCommandError(error).message });
    });
  }, []);

  const dismiss = useCallback(() => {
    if (update?.state === "ready") setDismissed(update.version);
  }, [update]);

  const showing =
    update?.state === "ready" && update.version === dismissed ? null : update;

  return (
    <UpdateBar
      update={showing}
      inACall={inACall}
      onInstall={install}
      onDismiss={dismiss}
    />
  );
}
