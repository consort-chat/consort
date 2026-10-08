/*
  The one thing about a section heading that only the stylesheet holds: it is
  now a control, so it has a target size to clear.

  A file of its own, because it is the only kind of test that wants CSS. See
  `vitest.config.ts` for why `.css.test.tsx` is the name that gets one.
*/
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import "./ChannelList.css";

/** What SC 2.5.8 (Target Size, Minimum) asks for, in CSS pixels. */
const FLOOR = 24;

/** Every rule jsdom parsed, for the lengths it resolves before anybody asks. */
function everyRule(): readonly string[] {
  return Array.from(document.styleSheets).flatMap((sheet) =>
    Array.from(sheet.cssRules).map((rule) => rule.cssText),
  );
}

/** One of the controls a section heading now holds, on its own. */
function control(name: string): HTMLElement {
  const { container } = render(<button type="button" className={name} />);
  return container.firstElementChild as HTMLElement;
}

/** The text of one rule, which is where a length can still be read as written. */
function rule(selector: string): string {
  const matching = everyRule().filter((one) =>
    one.startsWith(`${selector} {`),
  );
  expect(matching).toHaveLength(1);
  return matching.join("");
}

describe("the control that folds a section away", () => {
  it("measures itself against the text size rather than in pixels", () => {
    // So that it grows with the slider #122 added. A number in `px` passes the
    // floor below at the ordinary size and is wrong at 150%. Read off the rule
    // rather than the element, because jsdom resolves a `rem` to pixels before
    // `getComputedStyle` can be asked which it was.
    expect(rule(".channels__fold")).toMatch(/min-height:\s*[\d.]+rem/);
  });

  it("is big enough to hit at the ordinary text size", () => {
    // The heading used to be a word. Now it is something to press, and this
    // repository has been under 24 by 24 three times (#82, and #102 twice).
    // jsdom resolves the `rem` above against a 16px root, so this is pixels.
    const height = parseFloat(
      getComputedStyle(control("channels__fold")).minHeight,
    );

    expect(height).toBeGreaterThanOrEqual(FLOOR);
  });

  it("takes the list it folds out of the layout", () => {
    // The list is a grid, and an author `display` beats the browser's own
    // `[hidden]` rule, so without an override of our own a folded section
    // still draws every channel in it. Read off the rule, because jsdom
    // answers `display: none` for a hidden element either way and so cannot
    // tell the two apart.
    expect(rule(".channels__list[hidden]")).toContain("display: none");
  });
});

describe("the handle that moves a section", () => {
  it("is big enough to take hold of at the ordinary text size", () => {
    // The same floor the fold control clears, for the same reason. A grip is
    // the smallest thing in this column and the one most worth aiming at.
    const style = getComputedStyle(control("channels__grip"));

    expect(parseFloat(style.minHeight)).toBeGreaterThanOrEqual(FLOOR);
    expect(parseFloat(style.minWidth)).toBeGreaterThanOrEqual(FLOOR);
  });

  it("measures itself against the text size rather than in pixels", () => {
    expect(rule(".channels__grip")).toMatch(/min-height:\s*[\d.]+rem/);
    expect(rule(".channels__grip")).toMatch(/min-width:\s*[\d.]+rem/);
  });

  it("is visible without being hovered", () => {
    // A control revealed only under the pointer is one a keyboard finds by
    // accident and a reader of the screen never sees at all.
    expect(parseFloat(getComputedStyle(control("channels__grip")).opacity))
      .toBeGreaterThan(0);
  });
});

describe("the controls #170 put on a section somebody made", () => {
  /** Every control in this column that has to clear the target size floor. */
  const CONTROLS = [
    "channels__manage",
    "channels__new",
    "channels__choose",
    "channels__save",
    "channels__cancel",
  ];

  it.each(CONTROLS)("%s is big enough to hit", (name) => {
    // The same floor the fold control clears, for the same reason: this
    // repository has been under 24 by 24 three times (#82, and #102 twice).
    const height = parseFloat(getComputedStyle(control(name)).minHeight);

    expect(height).toBeGreaterThanOrEqual(FLOOR);
  });

  it.each(["channels__manage", "channels__new", "channels__choose"])(
    "%s measures itself against the text size",
    (name) => {
      expect(rule(`.${name}`)).toMatch(/min-height:\s*[\d.]+rem/);
    },
  );

  it("measures the Save and Cancel pair against the text size too", () => {
    // One rule for the two, so `rule` cannot be asked for either on its own.
    expect(rule(".channels__save,\n.channels__cancel")).toMatch(
      /min-height:\s*[\d.]+rem/,
    );
  });

  it("leaves the rename and delete controls visible to the keyboard", () => {
    // Faded rather than hidden, on the grip's terms. `display: none` or
    // `visibility: hidden` would take them out of the tab order as well.
    const style = getComputedStyle(control("channels__manage"));

    expect(style.display).not.toBe("none");
    expect(style.visibility).not.toBe("hidden");
  });

  it("takes a section's checklist out of the layout when it is put away", () => {
    // A grid, so an author `display` beats the browser's own `[hidden]` rule.
    // The same trap `.channels__list[hidden]` is here for.
    expect(rule(".channels__choices[hidden]")).toContain("display: none");
  });
});

describe("the control that reads a voice channel without connecting", () => {
  it("is laid out in the row rather than over it", () => {
    // #192. Positioned, it was drawn on top of the grip that drags a channel
    // onto a section, and the two glyphs overlapped.
    expect(getComputedStyle(control("channels__chat")).position).toBe("static");
  });

  it("is kept off what it sits beside by the row's own gap", () => {
    // In the flow with a gap, no pair of these can land on each other. That
    // is the fix, and a negative margin putting one back would undo it.
    expect(rule(".channels__row")).toMatch(/gap:\s*var\(--space-\d\)/);
    expect(rule(".channels__chat")).not.toMatch(/margin[^:]*:\s*-/);
  });

  it("comes after the badge and before the grip", () => {
    const order = (name: string) =>
      parseInt(getComputedStyle(control(name)).order, 10);

    expect(order("channels__mentions")).toBeLessThan(order("channels__chat"));
    expect(order("channels__chat")).toBeLessThan(order("channels__room-grip"));
  });

  it("is big enough to hit at the ordinary text size", () => {
    // #192 again: 24 by 24 cleared the floor and still had to be aimed at.
    // The same floor the rest of this column clears, with room over it.
    const style = getComputedStyle(control("channels__chat"));

    expect(parseFloat(style.minHeight)).toBeGreaterThanOrEqual(FLOOR);
    expect(parseFloat(style.minWidth)).toBeGreaterThanOrEqual(FLOOR);
  });

  it("measures itself against the text size rather than in pixels", () => {
    expect(rule(".channels__chat")).toMatch(/min-height:\s*[\d.]+rem/);
    expect(rule(".channels__chat")).toMatch(/min-width:\s*[\d.]+rem/);
  });

  it("grows the target without growing the glyph", () => {
    // The speaker on the same row is drawn from the same 24px grid, and two
    // icons at different weights beside each other read as a mistake.
    expect(rule(".channels__chat-glyph")).toMatch(/width:\s*14px/);
  });

  it("draws its focus ring around the target rather than the glyph", () => {
    // The ring is on the control, so it follows the hit area wherever that
    // goes. A pseudo-element carrying the target would leave it on the glyph.
    expect(rule(".channels__chat:focus-visible")).toContain(
      "var(--shadow-focus)",
    );
    expect(everyRule().some((one) => one.startsWith(".channels__chat::"))).toBe(
      false,
    );
  });
});
