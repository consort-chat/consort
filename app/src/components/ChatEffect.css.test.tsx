/*
  What a chat effect animates, and what stops it, read off the stylesheet that
  decides both.

  A file of its own because it is the kind of test that wants CSS, which
  `vitest.config.ts` runs as a second project. Two things are worth reading
  back here. The glyphs move on `transform` and `opacity` alone, which is what
  keeps fifty of them at once on the compositor rather than in layout; and the
  overlay is gone entirely for anybody who asked for less motion, which is the
  CSS half of a rule `ChatEffect` also enforces in JavaScript.
*/
import { describe, expect, it } from "vitest";

import "./ChatEffect.css";
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
  if (found === undefined) {
    throw new Error(`nothing reduces motion for ${selector}`);
  }
  return found.style;
}

/** Every property one set of keyframes animates. */
function animated(name: string): Set<string> {
  const found = rules().find(
    (rule): rule is CSSKeyframesRule =>
      rule instanceof CSSKeyframesRule && rule.name === name,
  );
  if (found === undefined) throw new Error(`no keyframes called ${name}`);

  const moved = new Set<string>();
  for (const frame of Array.from(found.cssRules)) {
    if (!(frame instanceof CSSKeyframeRule)) continue;
    for (const property of Array.from(frame.style)) moved.add(property);
  }
  return moved;
}

describe("the glyphs crossing the room", () => {
  it.each(["effect-fall", "effect-rise"])(
    "moves %s on nothing but transform and opacity",
    (name) => {
      // Anything else is a layout per frame, fifty times over.
      expect([...animated(name)].sort()).toEqual(["opacity", "transform"]);
    },
  );

  it("is reachable by nothing, being drawn over the messages", () => {
    expect(ruleFor(".effect").style.pointerEvents).toBe("none");
  });

  it("is not drawn at all for anybody who asked for less motion", () => {
    // The whole overlay, rather than the animation off it: fifty glyphs
    // standing still across the room is worse than either answer.
    expect(underReducedMotion(".effect").display).toBe("none");
    expect(underReducedMotion(".effect__glyph").animationName).toBe("none");
  });
});
