import {
  useEffect,
  useRef,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";

/** How far one press of an arrow key moves an edge, in pixels. */
export const STEP = 16;

/** The widths a column may be dragged between, in pixels. */
export interface Bounds {
  min: number;
  max: number;
}

/**
 * A width the column may actually be.
 *
 * The minimum last, so a window too small for both still leaves a readable
 * column rather than a sliver.
 */
export function clampTo({ min, max }: Bounds, width: number): number {
  return Math.max(min, Math.min(width, max));
}

/** What goes on the grip. Spread, so the aria and the handlers cannot drift. */
export interface Grip {
  "aria-orientation": "vertical";
  "aria-valuenow": number;
  "aria-valuemin": number;
  "aria-valuemax": number;
  onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void;
}

/**
 * Drag or nudge the edge of a column, whichever side of the pane it is on.
 *
 * `widens` is the way the pointer travels to make the column wider, which is
 * the only thing that differs between the two sidebars: the thread panel sits
 * to the right of the pane and the channel list to its left.
 *
 * The move listeners go on `window` rather than through `setPointerCapture`,
 * which jsdom does not implement, so a drag would be the one thing here no
 * test could reach. Capture would also be the wrong shape: what is being
 * dragged is the edge of the column, not the few pixels the hand landed on.
 */
export function useColumnResize({
  width,
  onResize,
  widens,
  bounds,
}: {
  /** How wide the column is now. Held by whoever draws it. */
  width: number;
  /** Report a width the grip was dragged or nudged to. Already clamped. */
  onResize: (width: number) => void;
  /** Which way the pointer goes to widen the column. */
  widens: "left" | "right";
  /** Asked afresh each gesture, because the maximum follows the window. */
  bounds: () => Bounds;
}): Grip {
  const limits = bounds();
  // So a gesture in flight can be let go of from somewhere other than a
  // `pointerup`, which is the only way an unmount can clean up after itself.
  const release = useRef<(() => void) | null>(null);

  /*
    A column can go while the pointer is still down on its grip: a thread is
    shut, the panel is not drawn, and the hand is still holding the edge.
    Without this its listeners outlive it.
  */
  useEffect(() => () => release.current?.(), []);

  // Leftwards is negative in client coordinates, so a column that widens to
  // the left counts a shrinking `clientX` as growth.
  const sign = widens === "left" ? -1 : 1;

  function grab(event: ReactPointerEvent<HTMLElement>) {
    // Otherwise the drag selects whatever the grip sits over, and the column
    // resizes with a blue smear across the pane beside it.
    event.preventDefault();
    const from = event.clientX;
    const started = width;

    function move(moved: PointerEvent) {
      onResize(clampTo(bounds(), started + sign * (moved.clientX - from)));
    }

    function drop() {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", drop);
      window.removeEventListener("pointercancel", drop);
      release.current = null;
    }

    release.current = drop;
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", drop);
    /*
      A touch the system takes back, which is how a drag ends when a gesture is
      recognised over it or the window loses the pointer. It is the one ending
      that never sends a `pointerup`.
    */
    window.addEventListener("pointercancel", drop);
  }

  /** The same edge, for a keyboard. A splitter only a mouse can move is half a
   * control. */
  function nudge(event: ReactKeyboardEvent<HTMLElement>) {
    const way =
      event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : 0;
    if (way === 0) return;

    // The pane scrolls under an arrow key otherwise, which would move the
    // conversation instead of the edge and look like the grip doing nothing.
    event.preventDefault();
    onResize(clampTo(bounds(), width + sign * way * STEP));
  }

  return {
    "aria-orientation": "vertical",
    "aria-valuenow": width,
    "aria-valuemin": limits.min,
    "aria-valuemax": limits.max,
    onPointerDown: grab,
    onKeyDown: nudge,
  };
}
