import { useEffect, useState } from "react";

import { asCommandError, screenView, selfView, theirView } from "./api";

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

  useEffect(() => poll(() => ASK[of](), setPicture, of), [of]);

  return picture;
}

/**
 * The newest frame of what somebody else in the call is sending, for as long as
 * this is mounted.
 *
 * `usePicture`'s twin, null on the same terms and polled the same way. The one
 * difference is `bound`: the long edge in pixels of the box this is being drawn
 * into, which Rust samples the frame to. Changing it restarts the poll, so a
 * card that grew asks for a bigger picture on its next frame rather than on its
 * next call.
 *
 * That argument is the seam issue #167 is built on. A cap somebody chooses goes
 * over the top of it, and the same number is what tells the SFU which layer to
 * send: `docs/adr/0013-ask-for-a-picture-in-pixels.md`.
 */
export function useTheirPicture(
  userId: string,
  of: Sending,
  bound: number,
): string | null {
  const [picture, setPicture] = useState<string | null>(null);

  useEffect(
    () =>
      poll(
        () => theirView(userId, of, bound),
        setPicture,
        `${userId} ${of}`,
      ),
    [userId, of, bound],
  );

  return picture;
}

/**
 * Keep asking `ask` and handing what comes back to `draw`, until the function
 * this answers with is called.
 *
 * Shared by both hooks above so there is one answer to how often a picture is
 * asked for and what happens when an ask fails.
 *
 * Chained rather than on an interval, so an answer slower than `EVERY` delays
 * the next ask instead of queueing one behind it.
 */
function poll(
  ask: () => Promise<string | null>,
  draw: (picture: string | null) => void,
  what: string,
): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function next() {
    try {
      const picture = await ask();
      if (stopped) return;
      draw(picture);
    } catch (raw) {
      // Once, and then stop. Twelve of these a second is not a report.
      console.error(
        `could not read the ${what} picture`,
        asCommandError(raw).detail,
      );
      return;
    }
    timer = setTimeout(next, EVERY);
  }

  void next();

  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}
