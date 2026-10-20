/*
  The switch that turns chat effects off, measured rather than read.

  A file of its own because it is the kind of test that wants CSS, which
  `vitest.config.ts` runs as a second project. What it is for is the target
  size: WCAG 2.5.8 asks 24 by 24 CSS pixels of anything there is to press, and
  this repository has been under it before by taking a control's height from
  the words inside it.
*/
import { describe, expect, it } from "vitest";

import "./AccessibilitySection.css";
import "../styles/tokens.css";

/** Every rule of every stylesheet this file imported, media blocks included. */
function rules(): CSSRule[] {
  return Array.from(document.styleSheets).flatMap((sheet) =>
    Array.from(sheet.cssRules),
  );
}

/**
 * One selector with its line breaks taken out.
 *
 * A rule listing two selectors is written over two lines, and jsdom hands the
 * newline back inside `selectorText`.
 */
function oneLine(selector: string): string {
  return selector.replace(/\s+/g, " ").trim();
}

/** The rule a selector was written as. */
function ruleFor(selector: string): CSSStyleRule {
  const found = rules().find(
    (rule): rule is CSSStyleRule =>
      rule instanceof CSSStyleRule && oneLine(rule.selectorText) === selector,
  );
  if (found === undefined) throw new Error(`no rule for ${selector}`);
  return found;
}

/** What a reduced-motion block says about a selector, if anything does. */
function underReducedMotion(selector: string): CSSStyleDeclaration {
  const found = rules()
    .filter((rule): rule is CSSMediaRule => rule instanceof CSSMediaRule)
    .filter((rule) => rule.conditionText.includes("prefers-reduced-motion"))
    .flatMap((rule) => Array.from(rule.cssRules))
    .find(
      (rule): rule is CSSStyleRule =>
        rule instanceof CSSStyleRule && oneLine(rule.selectorText) === selector,
    );
  if (found === undefined) {
    throw new Error(`nothing reduces motion for ${selector}`);
  }
  return found.style;
}

describe("the switch that turns them off", () => {
  it("clears 24 by 24 CSS pixels, which is what WCAG 2.5.8 asks", () => {
    // In `rem`, so it grows with the text size the slider above it sets. At
    // the 16px root this page is drawn at, 1.5rem is 24px.
    const box = ruleFor(".a11y__switch").style;

    expect(box.height).toBe("1.5rem");
    expect(box.width).toBe("2.5rem");
  });

  it("stops sliding for anybody who asked for less motion", () => {
    expect(underReducedMotion(".a11y__switch, .a11y__switch::after").transition).toBe(
      "none",
    );
  });
});
