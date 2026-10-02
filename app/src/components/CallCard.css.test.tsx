/*
  Where a glyph sits inside the controls along the card's title bar.

  A file of its own, because it is the only kind of test that wants CSS.
  `vitest.config.ts` runs a second project over `.css.test.tsx` with the
  stylesheets processed, and `RoomTimeline.css.test.tsx` says why that cannot
  be on for the whole run.
*/
import { describe, expect, it } from "vitest";

import "./CallCard.css";

/** The rule a selector was written as, out of the processed stylesheet. */
function ruleFor(selector: string): CSSStyleRule {
  const rules = Array.from(document.styleSheets).flatMap((sheet) =>
    Array.from(sheet.cssRules),
  );
  const found = rules.find(
    (rule): rule is CSSStyleRule =>
      rule instanceof CSSStyleRule && rule.selectorText === selector,
  );
  if (found === undefined) throw new Error(`no rule for ${selector}`);
  return found;
}

/*
  Read off the declaration rather than through `getComputedStyle`, which cannot
  answer this one. The padding at issue is the browser's own: WebKitGTK 2.52.6
  gives a button `2px 6px 3px 6px` and jsdom gives it nothing, so the computed
  value is "0" here whether or not the rule resets it.
*/
describe("the controls on the call card's title bar", () => {
  it("leaves a glyph the whole control to be centred in", () => {
    const control = ruleFor(".call-card__control");

    expect(control.style.padding).toBe("0px");
  });
});
