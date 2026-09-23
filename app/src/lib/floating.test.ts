import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { keptOnScreen } from "./floating";

/**
 * jsdom has no layout, so the window is whatever a test says it is.
 *
 * Set explicitly rather than left at the 1024 by 768 jsdom starts with,
 * because a clamp tested against a size nobody wrote down is a clamp that
 * passes for the wrong reason the day that default changes.
 */
function windowIs(width: number, height: number) {
  Object.defineProperty(window, "innerWidth", { value: width, configurable: true });
  Object.defineProperty(window, "innerHeight", { value: height, configurable: true });
}

beforeEach(() => {
  windowIs(1000, 800);
});

afterEach(() => {
  windowIs(1024, 768);
});

describe("keptOnScreen", () => {
  it("leaves a place that already fits alone", () => {
    expect(keptOnScreen({ left: 200, top: 100 }, { width: 240, height: 160 }))
      .toEqual({ left: 200, top: 100 });
  });

  it("pulls a box back off the far edges", () => {
    expect(keptOnScreen({ left: 900, top: 700 }, { width: 240, height: 160 }))
      .toEqual({ left: 1000 - 240 - 8, top: 800 - 160 - 8 });
  });

  it("keeps a box off the near edges", () => {
    expect(keptOnScreen({ left: -50, top: -50 }, { width: 240, height: 160 }))
      .toEqual({ left: 8, top: 8 });
  });

  it("leaves a box bigger than the window reachable at the near corner", () => {
    // The corner somebody can still reach is the one they read from. Pinning
    // it to the far edge instead would put the heading off screen.
    expect(keptOnScreen({ left: 40, top: 40 }, { width: 1200, height: 900 }))
      .toEqual({ left: 8, top: 8 });
  });
});
