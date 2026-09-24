/**
 * How big the application is drawn, and the arithmetic of changing it.
 *
 * Two knobs, and keeping them apart is the whole point of this module. The
 * application scale is the webview's own zoom, which Rust applies because only
 * Rust can; it moves everything a page has, pictures and avatars and borders
 * included, which is why it is what Ctrl and plus are bound to. The text scale
 * is a root font size, which only the page can set; it moves the type scale and
 * the spacing measured against it, and leaves a picture somebody sent at the
 * size they sent it.
 *
 * Nothing here talks to Rust. The ranges and the stepping are plain arithmetic
 * so they can be checked without a window, which is the only way any of it can
 * be checked: `set_zoom` needs a running application.
 */

/** A size range, and how far one press of a key moves inside it. */
export interface Range {
  min: number;
  max: number;
  step: number;
}

/**
 * The application scale, as multipliers of the size Consort has always drawn.
 *
 * The same numbers as `MIN_APPLICATION_SCALE` and `MAX_APPLICATION_SCALE` in
 * `app/src-tauri/src/settings.rs`, which is the authority: it clamps what
 * reaches the file and what comes back out of it, so a disagreement here shows
 * up as a slider that snaps rather than as a window nobody can read.
 *
 * A tenth per press, which is what a browser's zoom ladder does around 100%,
 * and 80% to 200% rather than the 20% to 1000% Tauri's own hotkeys offer.
 */
export const APPLICATION_SCALE: Range = { min: 0.8, max: 2, step: 0.1 };

/**
 * The text scale, on top of whatever the application scale already is.
 *
 * Narrower than the range above, and `settings.rs` says why: this multiplier
 * moves the words while the pictures and the fixed column widths stay put, so
 * its far end is a layout in tension rather than a larger Consort. A twentieth
 * per press, because the useful range is small enough that a tenth would be
 * four stops from end to end.
 */
export const TEXT_SCALE: Range = { min: 0.9, max: 1.5, step: 0.05 };

/** Which way a keystroke moves the application scale. */
export type Zoom = "in" | "out" | "reset";

/**
 * `value`, brought inside `range`.
 *
 * Here as well as in Rust rather than instead of it. Rust's copy guards the
 * file, which somebody can edit by hand. This one guards the moment before a
 * write: both sliders apply as they are dragged, so a size out of range would
 * be on screen whether or not it ever reached disk.
 */
export function inRange(value: number, range: Range): number {
  return Math.min(Math.max(value, range.min), range.max);
}

/**
 * The next size up or down from `value`, on `range`'s ladder.
 *
 * Snapped to the ladder rather than added to blindly. A hand-edited file can
 * hold 1.04, and stepping by a tenth from there would put 1.14 in the file and
 * leave every later press off the ladder too.
 *
 * Rounded to two places because that is the arithmetic being asked for and not
 * what floating point does with it: 0.8 plus four tenths is 1.2000000000000002,
 * and without this that is the number written to settings.json and read back
 * out of it forever.
 */
export function stepped(
  value: number,
  range: Range,
  direction: "in" | "out",
): number {
  const rungs = Math.round((inRange(value, range) - range.min) / range.step);
  const moved = range.min + (rungs + (direction === "in" ? 1 : -1)) * range.step;
  return Number(inRange(moved, range).toFixed(2));
}

/**
 * What a keystroke is asking of the application scale, or null for anything
 * else.
 *
 * Ctrl and plus is not one key. It is Ctrl+= on most layouts, Ctrl+Shift+= from
 * somebody who actually types a plus, and Ctrl and the numpad's own plus; every
 * browser takes all three. Matching on `key` rather than `code` takes all three
 * at once, because the first produces "=" and the other two both produce "+",
 * and it keeps working on a layout that puts those characters somewhere else.
 *
 * Alt and Meta are refused rather than ignored. Ctrl+Alt+plus and
 * Super+Ctrl+plus belong to other things on a Linux desktop, and a window that
 * answered them would be taking a keystroke meant for the window manager.
 *
 * Deliberately not filtered by what has focus, on the same grounds as the
 * Ctrl+Q handler beside it in `App.tsx`: a size is nobody's in particular, and
 * a shortcut that silently did nothing depending on where the caret sat would
 * be worse than not having one.
 */
export function zoomIntent(event: KeyboardEvent): Zoom | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;

  switch (event.key) {
    case "=":
    case "+":
      return "in";
    case "-":
      return "out";
    case "0":
      return "reset";
    default:
      return null;
  }
}

/**
 * Everybody waiting to hear that a keystroke changed the application scale.
 *
 * Here because two things move that scale and only one of them is on screen:
 * the slider in Settings, and Ctrl and plus from anywhere. Without this,
 * pressing the key with the Accessibility section open leaves the slider
 * showing a size the window no longer is, and the next drag of it would snap
 * the window back to that.
 *
 * No payload. What a listener wants is to go and read the size again, from the
 * file that is the authority on it, and the announcement is made after the
 * write so that reading finds the new one.
 *
 * The same shape as the listeners in `api.ts`: subscribing answers with the
 * function that stops it, to be called from an effect's cleanup.
 */
const watching = new Set<() => void>();

/** Hear about it. The answer stops listening. */
export function onZoomed(handler: () => void): () => void {
  watching.add(handler);
  return () => {
    watching.delete(handler);
  };
}

/** Say so. */
export function zoomed(): void {
  for (const handler of watching) handler();
}

/** A multiplier, as whole percent, for a readout beside a slider. */
export function percent(scale: number): string {
  return `${Math.round(scale * 100)}%`;
}

/**
 * Draw the page's words at `scale`.
 *
 * A percentage rather than a pixel size, because a percentage on the root
 * multiplies whatever the webview would otherwise have used. Somebody whose
 * platform already enlarges text keeps that and gets this on top of it; a pixel
 * size would quietly throw their setting away.
 *
 * The other half of the size is not here. That is the webview's zoom, which is
 * Rust's to apply, and this deliberately does not reach for it: a function that
 * moved both would be the two knobs becoming one.
 */
export function applyTextScale(scale: number): void {
  document.documentElement.style.fontSize = percent(inRange(scale, TEXT_SCALE));
}
