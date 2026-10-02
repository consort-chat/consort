import { useEffect, useState } from "react";

import { asCommandError, selfView } from "./api";

/**
 * How long to wait between frames, in milliseconds.
 *
 * Twelve and a half a second. Smooth enough to read as video and well under
 * what a camera produces, so the cost is a frame converted per draw rather than
 * per capture.
 */
const EVERY = 80;

/**
 * The newest frame from this session's camera, while `active`.
 *
 * Null when the camera is off, before its first frame, and after a poll fails.
 * A caller draws whatever it draws without a camera, so there is no error state
 * to hold: the camera's own is on the self-video channel, with the sentence that
 * says why.
 *
 * Chained rather than on an interval, so an answer slower than `EVERY` delays
 * the next ask instead of queueing one behind it.
 */
export function useSelfView(active: boolean): string | null {
  const [picture, setPicture] = useState<string | null>(null);

  useEffect(() => {
    // Cleared rather than left, so the card goes back to faces at the click
    // rather than whenever a poll would next have said so.
    if (!active) {
      setPicture(null);
      return;
    }

    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    async function draw() {
      try {
        const next = await selfView();
        if (stopped) return;
        setPicture(next);
      } catch (raw) {
        // Once, and then stop. Twelve of these a second is not a report.
        console.error("could not read the self view", asCommandError(raw).detail);
        return;
      }
      timer = setTimeout(draw, EVERY);
    }

    void draw();

    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [active]);

  return picture;
}
