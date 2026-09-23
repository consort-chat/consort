/**
 * How far to keep a floating thing from the edge of the window.
 *
 * One number for everything that floats. Two things do: the card a name in a
 * call opens, and the call card itself, which the first of those opens from.
 * A card that stopped a different distance from the edge than the menu it
 * opens would read as a mistake rather than as two decisions.
 */
const GAP = 8;

/** Where something floating has been put, in the coordinates `fixed` takes. */
export interface Place {
  left: number;
  top: number;
}

/**
 * The nearest place to the one asked for that keeps a box of this size on
 * screen.
 *
 * The near edge wins when the two cannot both be had. A box wider than the
 * window is pinned to the left and top rather than centred or cut off at the
 * far end, because the corner somebody can still reach is the one they read
 * from and the one that carries a heading.
 *
 * `window` is read at the moment of asking rather than passed in, because
 * every caller is answering a question about right now: where the pointer went,
 * or where the window ended up after somebody dragged its corner.
 */
export function keptOnScreen(
  wanted: Place,
  box: { width: number; height: number },
): Place {
  return {
    left: Math.max(GAP, Math.min(wanted.left, window.innerWidth - box.width - GAP)),
    top: Math.max(GAP, Math.min(wanted.top, window.innerHeight - box.height - GAP)),
  };
}
