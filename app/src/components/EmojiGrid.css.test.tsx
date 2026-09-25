/*
  What the emoji grid measures, read off the stylesheet that draws it.

  Three things, and none is visible from the component on its own.

  The first is the column count. The arrow keys move down by `ACROSS`, and the
  stylesheet draws that many columns off a custom property. Nothing at runtime
  can check they agree: jsdom does no layout, and in a browser the number is
  settled by the time anything could ask. If they drift, the down arrow sends
  the cursor somewhere the eye is not, and every keyboard test still passes,
  because they all use the constant.

  The second is that the grid is sized in `rem` rather than `px`. #122 put the
  text size on a slider of its own, so the root font size is something somebody
  can set to 150%, and a grid measured in pixels would keep its old size while
  the words beside it grew.

  The third is where the category strip's scrollbar sits. WebKit draws it
  inside the box, so without room reserved below the tabs it lands across their
  feet and swallows a press aimed there (#125).

  A file of its own, because it is the only kind of test that wants CSS. See
  `vitest.config.ts` for why `.css.test.tsx` is the name that gets one.
*/
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import "./EmojiGrid.css";

import { ACROSS, EmojiGrid } from "./EmojiGrid";
import type { EmojiSet } from "../lib/emoji";

const SMALL: EmojiSet = {
  groups: [
    {
      name: "Smileys & Emotion",
      slug: "smileys-emotion",
      emoji: [
        { key: "😀", name: "grinning face", terms: ["grinning"], skins: [] },
      ],
    },
  ],
  tones: [{ name: "light skin tone", swatch: "🏻" }],
};

/** What a browser calls one `rem` before anybody has changed anything. */
const REM = 16;

/** What SC 2.5.8 (Target Size, Minimum) asks for, in CSS pixels. */
const FLOOR = 24;

function drawTheGrid() {
  const { container } = render(
    <EmojiGrid
      set={SMALL}
      action="React with"
      recent={[]}
      tone={0}
      onTone={vi.fn()}
      onPick={vi.fn()}
    />,
  );
  return container.querySelector(".emoji") as HTMLElement;
}

describe("the shape of the grid", () => {
  it("draws as many columns as the arrow keys step by", () => {
    const grid = drawTheGrid();

    expect(
      getComputedStyle(grid).getPropertyValue("--emoji-across").trim(),
    ).toBe(String(ACROSS));
  });

  it("measures a key against the text size rather than in pixels", () => {
    // So that it grows with the slider #122 added. A number in `px` here
    // passes every other test in the suite and is wrong at 150%.
    const grid = drawTheGrid();

    expect(
      getComputedStyle(grid).getPropertyValue("--emoji-key").trim(),
    ).toMatch(/rem$/);
  });

  it("leaves a key big enough to hit at the ordinary text size", () => {
    const grid = drawTheGrid();
    const key = parseFloat(
      getComputedStyle(grid).getPropertyValue("--emoji-key"),
    );

    expect(key * REM).toBeGreaterThanOrEqual(FLOOR);
  });

  it("caps the grid in rows rather than at a fixed height", () => {
    // A length here would keep its old size at 150% text while the keys inside
    // it grew, so the bottom row would be cut in half rather than scrolled to.
    drawTheGrid();
    const rows = screen.getByRole("group", { name: "Smileys & Emotion" });

    expect(getComputedStyle(rows).maxHeight).toContain("var(--emoji-key)");
  });

  it("sizes the key it actually draws from that number", () => {
    // Otherwise the two above are measuring a custom property nothing uses.
    drawTheGrid();
    const key = screen.getByRole("button", { name: "React with grinning face" });

    expect(getComputedStyle(key).minHeight).toBe("var(--emoji-key)");
    expect(getComputedStyle(key).minWidth).toBe("var(--emoji-key)");
  });
});

/** The strip of categories, which is the one thing here that scrolls sideways. */
const strip = () => screen.getByRole("group", { name: /categor/i });

/**
 * What the bar takes across the foot of the strip, in CSS pixels.
 *
 * The one pictured on #125 measured about seven, and a GTK overlay bar widens
 * to roughly twice that when the pointer comes near it.
 */
const BAR = 14;

/** Every rule jsdom parsed. A pseudo-element is reachable no other way. */
function everyRule(): readonly string[] {
  return Array.from(document.styleSheets).flatMap((sheet) =>
    Array.from(sheet.cssRules).map((rule) => rule.cssText),
  );
}

describe("the category strip", () => {
  it("draws a scrollbar, which is the only way across the categories", () => {
    drawTheGrid();

    expect(
      getComputedStyle(strip()).getPropertyValue("scrollbar-width").trim(),
    ).toBe("thin");
  });

  it("does not hide the one WebKit draws either", () => {
    // The pair of rules that took the bar away. Asking for `thin` above while
    // a pseudo-element rule still says `display: none` would leave it gone.
    drawTheGrid();

    const hidden = everyRule().filter(
      (rule) =>
        rule.startsWith(".emoji__tabs::-webkit-scrollbar") &&
        rule.includes("display: none"),
    );

    expect(hidden).toEqual([]);
  });

  it("still scrolls, because there are more categories than fit", () => {
    // A strip that had lost its overflow would have a usable bar and nothing
    // for it to move.
    drawTheGrid();

    expect(getComputedStyle(strip()).getPropertyValue("overflow-x")).toBe(
      "auto",
    );
  });

  it("reserves room below the tabs for the bar to sit in", () => {
    // The actual fix for #125. jsdom does no layout, so what is checkable is
    // that the clearance exists, is measured against the text size, and is
    // deeper than the bar it is holding off.
    const grid = drawTheGrid();
    const clearance = getComputedStyle(grid)
      .getPropertyValue("--emoji-bar")
      .trim();

    expect(clearance).toMatch(/rem$/);
    expect(parseFloat(clearance) * REM).toBeGreaterThanOrEqual(BAR);
    expect(getComputedStyle(strip()).paddingBottom).toBe("var(--emoji-bar)");
  });

  it("leaves a tab big enough to hit all of", () => {
    // The other half of it. The clearance keeps the bar off the tabs; this is
    // the tabs being worth the room in the first place.
    const grid = drawTheGrid();
    const height = getComputedStyle(grid).getPropertyValue("--emoji-tab").trim();

    expect(height).toMatch(/rem$/);
    expect(parseFloat(height) * REM).toBeGreaterThanOrEqual(FLOOR);
  });

  it("sizes the tab it actually draws from that number", () => {
    // Otherwise the one above is measuring a custom property nothing uses.
    drawTheGrid();
    const tab = within(strip()).getByRole("button", {
      name: "Smileys & Emotion",
    });

    expect(getComputedStyle(tab).minHeight).toBe("var(--emoji-tab)");
  });
});
