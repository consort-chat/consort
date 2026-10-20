import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CEILING, STEP, stageBound, useStageBound } from "./stageBound";

/** A box of CSS pixels, the way `getBoundingClientRect` reports one. */
function box(width: number, height: number) {
  return { width, height };
}

describe("stageBound", () => {
  it("asks for the long edge of the box", () => {
    // The whole point. Rust samples the frame to a long edge and the SFU picks
    // a layer by height, so the number that leaves here is the box the picture
    // is drawn in and not a guess at it: issue #165.
    expect(stageBound(box(1280, 720), 1)).toBe(1280);
  });

  it("asks for the taller edge of a box taller than it is wide", () => {
    expect(stageBound(box(720, 1280), 1)).toBe(1280);
  });

  it("asks in device pixels and not in CSS pixels", () => {
    // A 960 CSS pixel box on a 2x display is 1920 real pixels, and a picture
    // made for 960 of them is drawn at half the detail the screen can show.
    expect(stageBound(box(640, 360), 2)).toBe(1280);
  });

  it("rounds up, so the picture is never smaller than the box", () => {
    expect(stageBound(box(1281, 720), 1)).toBe(1280 + STEP);
  });

  it("answers in steps, so dragging a window edge is not an ask per pixel", () => {
    // The bound is what restarts the poll and what tells the SFU which layer
    // to send. Measured to the pixel, a resize would be a constraint per
    // frame of the drag.
    expect(stageBound(box(1201, 676), 1)).toBe(stageBound(box(1200, 675), 1));
  });

  it("stops at the ceiling the picture is made under", () => {
    // `theirview::MAX_BOUND`. Asking past it is asking for a picture nothing
    // will make, so the SFU would be told a size the window never draws.
    expect(stageBound(box(3840, 2160), 1)).toBe(CEILING);
    expect(stageBound(box(2560, 1440), 2)).toBe(CEILING);
  });

  it("asks for the ceiling for a box nothing has measured yet", () => {
    // One poll before layout has given the stage a size. The alternative is a
    // deliberately blurred first frame on the one view where the detail is the
    // whole point.
    expect(stageBound(box(0, 0), 1)).toBe(CEILING);
  });

  it("treats a ratio it cannot believe as one device pixel per CSS pixel", () => {
    // `devicePixelRatio` is read off the window, so a zero or a NaN is
    // reachable and would ask for a picture with no pixels in it.
    expect(stageBound(box(960, 540), 0)).toBe(960);
    expect(stageBound(box(960, 540), Number.NaN)).toBe(960);
  });
});

describe("useStageBound", () => {
  afterEach(() => vi.restoreAllMocks());

  /** Report `width` by `height` from `node`, the way a laid-out one does. */
  function measures(node: HTMLElement, width: number, height: number) {
    node.getBoundingClientRect = () =>
      ({
        width,
        height,
        left: 0,
        top: 0,
        right: width,
        bottom: height,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      }) as DOMRect;
  }

  /** An element that measures `width` by `height`. */
  function sized(width: number, height: number): HTMLElement {
    const node = document.createElement("span");
    measures(node, width, height);
    return node;
  }

  it("answers with the ceiling while there is nothing to measure", () => {
    // Mounted with no stage under it, which is every render before the one
    // that puts a screen on the stage.
    const { result } = renderHook(() => useStageBound());

    expect(result.current.bound).toBe(CEILING);
  });

  it("measures the element as it is attached", () => {
    // Before the first paint, so the first poll asks for the box and not for
    // the ceiling it starts at.
    const { result } = renderHook(() => useStageBound());

    act(() => result.current.ref(sized(1024, 576)));

    expect(result.current.bound).toBe(1024);
  });

  it("measures again when it is asked to", () => {
    // The card growing is the stage growing, and the window did not move. The
    // card calls this from the layout effect that follows its own size.
    const { result } = renderHook(() => useStageBound());
    const node = sized(1024, 576);
    act(() => result.current.ref(node));

    measures(node, 640, 360);
    act(() => result.current.measure());

    expect(result.current.bound).toBe(640);
  });

  it("measures again when the window changes size", () => {
    // Filling the window means the stage is the window, so the window
    // resizing is the one thing that changes this without a render.
    const { result } = renderHook(() => useStageBound());
    const node = sized(1024, 576);
    act(() => result.current.ref(node));

    measures(node, 640, 360);
    act(() => window.dispatchEvent(new Event("resize")));

    expect(result.current.bound).toBe(640);
  });

  it("goes back to the ceiling once the element is gone", () => {
    // A stage unmounts whenever the last share stops, and the card asks for a
    // bound before it next asks for a picture.
    const { result } = renderHook(() => useStageBound());
    act(() => result.current.ref(sized(640, 360)));

    act(() => {
      result.current.ref(null);
      result.current.measure();
    });

    expect(result.current.bound).toBe(CEILING);
  });

  it("stops measuring once it is unmounted", () => {
    const { result, unmount } = renderHook(() => useStageBound());
    const node = sized(1024, 576);
    act(() => result.current.ref(node));
    const measured = vi.spyOn(node, "getBoundingClientRect");

    unmount();
    window.dispatchEvent(new Event("resize"));

    expect(measured).not.toHaveBeenCalled();
  });
});
