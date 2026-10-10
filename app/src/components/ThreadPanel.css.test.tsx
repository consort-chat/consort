/*
  What the four controls in the thread's composer measure, read off the
  stylesheet that draws them. #173: the box and Send came out shorter than the
  emoji control beside them. #134 added the paperclip, which is the same square.

  A file of its own, because it is the only kind of test that wants CSS.
  `RoomTimeline.css.test.tsx` says why that cannot be on for the whole run.
*/
import { describe, expect, it } from "vitest";

import "./ThreadPanel.css";
import "./ComposerAttach.css";
import "./ComposerEmoji.css";

/** The rule a selector was written as. */
function ruleFor(selector: string): CSSStyleRule {
  const found = Array.from(document.styleSheets)
    .flatMap((sheet) => Array.from(sheet.cssRules))
    .find(
      (rule): rule is CSSStyleRule =>
        rule instanceof CSSStyleRule && rule.selectorText === selector,
    );
  if (found === undefined) throw new Error(`no rule for ${selector}`);
  return found;
}

/*
  Read off the declarations. jsdom lays nothing out and cannot resolve `lh`, so
  what can be checked is that every control is written in terms of the one
  number, and that the number resolves the same way for each of them.
*/
describe("the thread composer's four controls", () => {
  it("leaves a column for the paperclip, the emoji control, the box and Send", () => {
    expect(
      ruleFor(".thread__composer").style.gridTemplateColumns,
    ).toBe("auto auto minmax(0, 1fr) auto");
  });

  it("works its height out from the line, the padding and the border", () => {
    expect(ruleFor(".thread__composer").style.getPropertyValue(
      "--composer-control",
    )).toBe("calc(1lh + 2*(var(--space-3) + 1px))");
  });

  it("sets the small type on the row, so `1lh` is one number for them all", () => {
    // Without this the emoji control resolves `1lh` against the inherited
    // base size while the box and Send resolve it against `--text-sm`.
    expect(ruleFor(".thread__composer").style.fontSize).toBe("var(--text-sm)");
  });

  it("pins the box to that height rather than leaving it to arithmetic", () => {
    expect(ruleFor(".thread__draft").style.minHeight).toBe(
      "var(--composer-control)",
    );
  });

  it("pads the box by what that height was worked out from", () => {
    expect(ruleFor(".thread__draft").style.padding).toBe("var(--space-3)");
  });

  it("pins Send to the same height", () => {
    expect(ruleFor(".thread__send").style.minHeight).toBe(
      "var(--composer-control)",
    );
  });

  it("spends a border on Send, so it comes to the height of the bordered two", () => {
    expect(ruleFor(".thread__send").style.border).toBe("1px solid transparent");
  });

  it("sizes the emoji control from the same number", () => {
    const emoji = ruleFor(".composer-emoji__open").style;
    expect(emoji.width).toBe("var(--composer-control)");
    expect(emoji.height).toBe("var(--composer-control)");
  });

  it("sizes the paperclip from it too, so the row is one strip", () => {
    const attach = ruleFor(".composer-attach").style;
    expect(attach.width).toBe("var(--composer-control)");
    expect(attach.height).toBe("var(--composer-control)");
  });
});
