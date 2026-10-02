import { useEffect, useState } from "react";

import { asCommandError, screenView, selfView } from "./api";

/**
 * How long to wait between frames, in milliseconds.
 *
 * Twelve and a half a second. Smooth enough to read as video and well under
 * what a camera produces, so the cost is a frame converted per draw rather than
 * per capture.
 */
const EVERY = 80;

/** Which of the things this session can be sending to draw. */
export type Sending = "camera" | "screen";

/** Where each picture comes from. Both are one slot in Rust, newest wins. */
const ASK: Record<Sending, () => Promise<string | null>> = {
  camera: selfView,
  screen: screenView,
};

/**
 * The newest frame of what this session is sending, for as long as this is
 * mounted.
 *
 * Null before the first frame and after a poll fails. A caller draws whatever
 * it draws without one, so there is no error state to hold: what is actually
 * running is reported on the self-video and self-screen channels, with the
 * sentence that says why.
 *
 * Being mounted is the switch, rather than a flag to pass. Nothing draws a
 * square for a camera that is off, so there is nowhere for this to live and be
 * idle, and unmounting forgets the last frame for free.
 *
 * `of` names the source rather than being a function to call, so the one
 * argument is a value and a caller cannot restart the poll every render by
 * passing a fresh closure.
 *
 * Chained rather than on an interval, so an answer slower than `EVERY` delays
 * the next ask instead of queueing one behind it.
 */
export function usePicture(of: Sending): string | null {
  const [picture, setPicture] = useState<string | null>(null);

  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    async function draw() {
      try {
        const next = await ASK[of]();
        if (stopped) return;
        setPicture(next);
      } catch (raw) {
        // Once, and then stop. Twelve of these a second is not a report.
        console.error(
          `could not read the ${of} picture`,
          asCommandError(raw).detail,
        );
        return;
      }
      timer = setTimeout(draw, EVERY);
    }

    void draw();

    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [of]);

  return picture;
}
