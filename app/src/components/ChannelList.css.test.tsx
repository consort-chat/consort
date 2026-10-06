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

/** The control a section heading now holds, on its own. */
function foldControl(): HTMLElement {
  const { container } = render(
    <button type="button" className="channels__fold" />,
  );
  return container.firstElementChild as HTMLElement;
}

describe("the control that folds a section away", () => {
  it("measures itself against the text size rather than in pixels", () => {
    // So that it grows with the slider #122 added. A number in `px` passes the
    // floor below at the ordinary size and is wrong at 150%. Read off the rule
    // rather than the element, because jsdom resolves a `rem` to pixels before
    // `getComputedStyle` can be asked which it was.
    const rule = everyRule().filter((one) =>
      one.startsWith(".channels__fold {"),
    );

    expect(rule).toHaveLength(1);
    expect(rule.join("")).toMatch(/min-height:\s*[\d.]+rem/);
  });

  it("is big enough to hit at the ordinary text size", () => {
    // The heading used to be a word. Now it is something to press, and this
    // repository has been under 24 by 24 three times (#82, and #102 twice).
    // jsdom resolves the `rem` above against a 16px root, so this is pixels.
    const height = parseFloat(getComputedStyle(foldControl()).minHeight);

    expect(height).toBeGreaterThanOrEqual(FLOOR);
  });

  it("takes the list it folds out of the layout", () => {
    // The list is a grid, and an author `display` beats the browser's own
    // `[hidden]` rule, so without an override of our own a folded section
    // still draws every channel in it. Read off the rule, because jsdom
    // answers `display: none` for a hidden element either way and so cannot
    // tell the two apart.
    const rule = everyRule().filter((one) =>
      one.startsWith(".channels__list[hidden] {"),
    );

    expect(rule).toHaveLength(1);
    expect(rule.join("")).toContain("display: none");
  });
});
