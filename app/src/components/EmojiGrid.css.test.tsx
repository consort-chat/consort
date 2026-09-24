/*
  What the emoji grid measures, read off the stylesheet that draws it.

  Two things, and neither is visible from the component on its own.

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

  A file of its own, because it is the only kind of test that wants CSS. See
  `vitest.config.ts` for why `.css.test.tsx` is the name that gets one.
*/
import { render, screen } from "@testing-library/react";
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

  it("sizes the key it actually draws from that number", () => {
    // Otherwise the two above are measuring a custom property nothing uses.
    drawTheGrid();
    const key = screen.getByRole("button", { name: "React with grinning face" });

    expect(getComputedStyle(key).minHeight).toBe("var(--emoji-key)");
    expect(getComputedStyle(key).minWidth).toBe("var(--emoji-key)");
  });
});
