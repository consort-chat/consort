import { useCallback, useEffect, useRef, useState } from "react";
import type { RefCallback } from "react";

/**
 * The most device pixels a picture is ever asked for, on the long edge.
 *
 * `theirview::MAX_BOUND` and `selfview`'s clamp are this same number, so a
 * larger ask would name a size nothing will make.
 */
export const CEILING = 1920;

/**
 * What the ask is rounded up to.
 *
 * The bound restarts the poll and tells the SFU which layer to send, so a
 * measurement to the pixel would be a constraint per frame of a window drag.
 */
export const STEP = 64;

/** A measured box, which is all of a `DOMRect` this reads. */
interface Box {
  width: number;
  height: number;
}

/**
 * How many device pixels to ask a picture for, for a box this big.
 *
 * The long edge, because that is the edge Rust samples to and the edge a cap
 * is a ceiling on. A box nothing has measured yet answers with the ceiling:
 * see `docs/adr/0019-measure-the-stage-rather-than-guess-it.md`.
 */
export function stageBound(box: Box, ratio: number): number {
  const long = Math.max(box.width, box.height);
  if (!(long > 0)) return CEILING;

  const real = long * (ratio > 0 && Number.isFinite(ratio) ? ratio : 1);

  return Math.min(Math.ceil(real / STEP) * STEP, CEILING);
}

/** What [`useStageBound`] answers with. */
interface Measured {
  /** Put this on the element the picture fills. */
  ref: RefCallback<HTMLElement>;
  /** How many device pixels to ask that picture for. */
  bound: number;
  /** Measure again, for a change the window did not cause. */
  measure: () => void;
}

/**
 * The box one picture is drawn in, in device pixels, kept up to date.
 *
 * The window's own resize is watched here, the way `useDraggable` watches it,
 * because a stage that fills the window changes size without a render. A
 * change the window did not cause is [`Measured.measure`], which is the same
 * split for the same reason.
 */
export function useStageBound(): Measured {
  const node = useRef<HTMLElement | null>(null);
  const [bound, setBound] = useState(CEILING);

  // A box of nothing for an element that is not there, so "not measured yet"
  // has one answer rather than two that can drift.
  const measure = useCallback(() => {
    const box = node.current?.getBoundingClientRect() ?? {
      width: 0,
      height: 0,
    };
    setBound(stageBound(box, window.devicePixelRatio));
  }, []);

  // Measured as it is attached, which is before the first paint, so the first
  // poll asks for the box rather than for the ceiling.
  const ref = useCallback(
    (attached: HTMLElement | null) => {
      node.current = attached;
      if (attached !== null) measure();
    },
    [measure],
  );

  useEffect(() => {
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [measure]);

  return { ref, bound, measure };
}
