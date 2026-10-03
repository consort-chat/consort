/*
  How the faces behind a shared screen are moved into place, read off the two
  stylesheets that decide it.

  A file of its own, because it is the only kind of test that wants CSS.
  `vitest.config.ts` runs a second project over `.css.test.tsx` with the
  stylesheets processed, and `RoomTimeline.css.test.tsx` says why that cannot
  be on for the whole run.
*/
import { describe, expect, it } from "vitest";

import "./CallCard.css";
import "../styles/tokens.css";

/** Every rule of every stylesheet this file imported, media blocks included. */
function rules(): CSSRule[] {
  return Array.from(document.styleSheets).flatMap((sheet) =>
    Array.from(sheet.cssRules),
  );
}

/** The rule a selector was written as. */
function ruleFor(selector: string): CSSStyleRule {
  const found = rules().find(
    (rule): rule is CSSStyleRule =>
      rule instanceof CSSStyleRule && rule.selectorText === selector,
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
        rule instanceof CSSStyleRule && rule.selectorText === selector,
    );
  if (found === undefined) throw new Error(`nothing reduces motion for ${selector}`);
  return found.style;
}

/*
  Read off the declarations. jsdom lays nothing out and answers no media query,
  so neither half of this is anything `getComputedStyle` can be asked: what can
  be checked is that the one rule is written in terms of the other.
*/
describe("the faces rising from behind a shared screen", () => {
  it("takes its timing from the token rather than from a number", () => {
    expect(ruleFor(".call-card__peek").style.animation).toContain(
      "var(--duration-normal)",
    );
  });

  it("which is what makes the motion stop for anybody who asked it to", () => {
    // The other half, and the reason the first is enough. Without this the row
    // would animate at full length for somebody who turned motion off.
    expect(underReducedMotion(":root").getPropertyValue("--duration-normal")).toBe(
      "1ms",
    );
  });
});
