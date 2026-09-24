import { afterEach, describe, expect, it } from "vitest";

import {
  APPLICATION_SCALE,
  TEXT_SCALE,
  applyTextScale,
  inRange,
  percent,
  stepped,
  zoomIntent,
} from "./scale";

/** A keystroke, with `ctrl` held unless a case says otherwise. */
function pressed(init: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
  return new KeyboardEvent("keydown", { ctrlKey: true, ...init });
}

describe("inRange", () => {
  it("leaves a size that is already usable exactly where it is", () => {
    expect(inRange(1.2, APPLICATION_SCALE)).toBe(1.2);
    expect(inRange(1.05, TEXT_SCALE)).toBe(1.05);
  });

  it("brings a size below the range up to the smallest one", () => {
    expect(inRange(0.1, APPLICATION_SCALE)).toBe(APPLICATION_SCALE.min);
    expect(inRange(-4, TEXT_SCALE)).toBe(TEXT_SCALE.min);
  });

  it("brings a size above the range down to the largest one", () => {
    expect(inRange(40, APPLICATION_SCALE)).toBe(APPLICATION_SCALE.max);
    expect(inRange(40, TEXT_SCALE)).toBe(TEXT_SCALE.max);
  });

  it("keeps the ends of both ranges themselves", () => {
    // Otherwise a slider dragged to either end would spring back off it.
    for (const range of [APPLICATION_SCALE, TEXT_SCALE]) {
      expect(inRange(range.min, range)).toBe(range.min);
      expect(inRange(range.max, range)).toBe(range.max);
    }
  });

  it("has 100% inside both ranges", () => {
    // The size every build before this one drew at. A range that excluded it
    // would move a window nobody had asked to move.
    expect(inRange(1, APPLICATION_SCALE)).toBe(1);
    expect(inRange(1, TEXT_SCALE)).toBe(1);
  });
});

describe("stepped", () => {
  it("goes up by one step", () => {
    expect(stepped(1, APPLICATION_SCALE, "in")).toBe(1.1);
  });

  it("goes down by one step", () => {
    expect(stepped(1, APPLICATION_SCALE, "out")).toBe(0.9);
  });

  it("stops at the top rather than going past it", () => {
    expect(stepped(APPLICATION_SCALE.max, APPLICATION_SCALE, "in")).toBe(
      APPLICATION_SCALE.max,
    );
  });

  it("stops at the bottom rather than going past it", () => {
    expect(stepped(APPLICATION_SCALE.min, APPLICATION_SCALE, "out")).toBe(
      APPLICATION_SCALE.min,
    );
  });

  it("answers a number a person can read rather than one floating point made", () => {
    // 0.8 + 4 * 0.1 is 1.2000000000000002, and that is the number that would
    // go into settings.json and come back out of it forever.
    let scale = APPLICATION_SCALE.min;
    for (let press = 0; press < 12; press += 1) {
      scale = stepped(scale, APPLICATION_SCALE, "in");
      expect(scale).toBe(Number(scale.toFixed(2)));
    }
  });

  it("walks the whole range in both directions and lands on both ends", () => {
    let scale = APPLICATION_SCALE.min;
    for (let press = 0; press < 40; press += 1) {
      scale = stepped(scale, APPLICATION_SCALE, "in");
    }
    expect(scale).toBe(APPLICATION_SCALE.max);

    for (let press = 0; press < 40; press += 1) {
      scale = stepped(scale, APPLICATION_SCALE, "out");
    }
    expect(scale).toBe(APPLICATION_SCALE.min);
  });

  it("finds the ladder from a size that was never on it", () => {
    // What a hand-edited settings file hands over. Stepping by a tenth from
    // 1.04 would put 1.14 in the file and keep it off the ladder forever.
    expect(stepped(1.04, APPLICATION_SCALE, "in")).toBe(1.1);
    expect(stepped(1.04, APPLICATION_SCALE, "out")).toBe(0.9);
  });

  it("steps the text size by its own step and not the application's", () => {
    expect(stepped(1, TEXT_SCALE, "in")).toBe(1 + TEXT_SCALE.step);
  });
});

describe("zoomIntent", () => {
  it("reads Ctrl and the equals key as zooming in", () => {
    // The plain one, on most layouts, and the reason this is not a match on
    // the plus character alone.
    expect(zoomIntent(pressed({ key: "=", code: "Equal" }))).toBe("in");
  });

  it("reads Ctrl and a typed plus as zooming in", () => {
    // Shift+= on a US layout, which is what somebody who means "plus" does.
    expect(
      zoomIntent(pressed({ key: "+", code: "Equal", shiftKey: true })),
    ).toBe("in");
  });

  it("reads Ctrl and the numpad plus as zooming in", () => {
    expect(zoomIntent(pressed({ key: "+", code: "NumpadAdd" }))).toBe("in");
  });

  it("reads Ctrl and the minus key as zooming out", () => {
    expect(zoomIntent(pressed({ key: "-", code: "Minus" }))).toBe("out");
  });

  it("reads Ctrl and the numpad minus as zooming out", () => {
    expect(zoomIntent(pressed({ key: "-", code: "NumpadSubtract" }))).toBe(
      "out",
    );
  });

  it("reads Ctrl and zero as going back to the size it started at", () => {
    expect(zoomIntent(pressed({ key: "0", code: "Digit0" }))).toBe("reset");
    expect(zoomIntent(pressed({ key: "0", code: "Numpad0" }))).toBe("reset");
  });

  it("ignores the same keys without Ctrl", () => {
    // Otherwise typing a minus sign into the composer would shrink the room.
    for (const key of ["=", "+", "-", "0"]) {
      expect(zoomIntent(new KeyboardEvent("keydown", { key }))).toBeNull();
    }
  });

  it("ignores Ctrl with something else held as well", () => {
    // Ctrl+Alt+plus and Super+Ctrl+plus are other people's bindings, and a
    // window that zoomed on them would be answering a keystroke meant for the
    // desktop.
    expect(zoomIntent(pressed({ key: "+", altKey: true }))).toBeNull();
    expect(zoomIntent(pressed({ key: "+", metaKey: true }))).toBeNull();
  });

  it("ignores every other key with Ctrl held", () => {
    for (const key of ["q", "v", "1", "9", "_", "Enter", "ArrowUp"]) {
      expect(zoomIntent(pressed({ key }))).toBeNull();
    }
  });
});

describe("percent", () => {
  it("reads a multiplier out as whole percent", () => {
    expect(percent(1)).toBe("100%");
    expect(percent(0.8)).toBe("80%");
    expect(percent(1.05)).toBe("105%");
  });

  it("does not show the tail floating point left on a stepped size", () => {
    expect(percent(1.2000000000000002)).toBe("120%");
  });
});

describe("applyTextScale", () => {
  afterEach(() => {
    document.documentElement.style.removeProperty("font-size");
  });

  it("sets the root font size as a percentage of the one the page would use", () => {
    // Percent rather than pixels on purpose: it multiplies whatever default
    // the webview is using rather than replacing it, so somebody whose
    // platform already enlarges text keeps that and gets this on top.
    applyTextScale(1.25);

    expect(document.documentElement.style.fontSize).toBe("125%");
  });

  it("puts 100% back rather than leaving the last size in place", () => {
    applyTextScale(1.5);

    applyTextScale(1);

    expect(document.documentElement.style.fontSize).toBe("100%");
  });

  it("refuses a size outside the range rather than applying it", () => {
    // The same guard Rust has on the file. This one is for the moment before
    // a write: the slider applies as it is dragged, so a nonsense value would
    // be on screen whether or not it was ever saved.
    applyTextScale(40);

    expect(document.documentElement.style.fontSize).toBe(
      `${TEXT_SCALE.max * 100}%`,
    );
  });
});
