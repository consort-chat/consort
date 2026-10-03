/*
  What the channel column may be dragged to.

  The element is `.shell__sidebar` and the grid track it sizes is
  `--shell-sidebar`, so the names here are the sidebar's rather than the list's.
*/
import { clampTo, type Bounds } from "../lib/useColumnResize";

/**
 * The channel column's width to begin with, and the width it has always been.
 *
 * `AppShell.css` carries the arithmetic behind the number and repeats it as the
 * stylesheet's own default. `AppShell.css.test.tsx` holds the two together.
 */
export const SIDEBAR_WIDE = 272;

/**
 * The narrowest the column may be dragged, in pixels.
 *
 * Narrower than the width above, which the voice strip was sized for, because
 * that strip ellipsizes now rather than being cut (#104). What this floor
 * protects is a channel name still being readable and the column still being
 * there: #138 asks for a column somebody can resize, not one they can lose.
 */
export const SIDEBAR_NARROWEST = 200;

/** The widest, whatever the window. A channel list past this is a second pane. */
const WIDEST = 480;

/** And the most of a small window it may take, whichever of the two is less. */
const MOST = 0.4;

/** Asked afresh each gesture, because the maximum follows the window. */
export function sidebarBounds(): Bounds {
  return {
    min: SIDEBAR_NARROWEST,
    max: Math.min(WIDEST, Math.round(window.innerWidth * MOST)),
  };
}

/** A width the column may actually be. */
export function clampSidebarWidth(width: number): number {
  return clampTo(sidebarBounds(), width);
}
