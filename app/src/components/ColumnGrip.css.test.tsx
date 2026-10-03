/*
  What the grip on a column's edge looks like, read off the stylesheet.

  #138 is the whole of this file: the thread panel has been resizable since
  #113 and nothing said so, because the grip was transparent until the pointer
  was already on it. A control nobody can see is a control nobody finds, so the
  lines being drawn at rest is the behaviour, not decoration.

  A file of its own, because it is the only kind of test that wants CSS. See
  `vitest.config.ts` for why `.css.test.tsx` is the name that gets one.
*/
import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import "../styles/tokens.css";
import "./ColumnGrip.css";

import { ColumnGrip } from "./ColumnGrip";

/** The narrowest target a hand can reliably land on, in CSS pixels. */
const HITTABLE = 8;

function drawTheGrip() {
  const { container } = render(
    <ColumnGrip
      className="grip--test"
      label="Resize the thing"
      width={300}
      bounds={() => ({ min: 100, max: 600 })}
      widens="left"
      onResize={vi.fn()}
    />,
  );
  const grip = container.querySelector(".grip") as HTMLElement;
  return { grip, lines: grip.querySelector(".grip__lines") as HTMLElement };
}

/** Every rule jsdom parsed. A pseudo-element is reachable no other way. */
function everyRule(): readonly string[] {
  return Array.from(document.styleSheets).flatMap((sheet) =>
    Array.from(sheet.cssRules).map((rule) => rule.cssText),
  );
}

describe("the lines that say an edge can be dragged", () => {
  it("draws two of them, as hairlines either side of a gap", () => {
    const { lines } = drawTheGrip();
    const style = getComputedStyle(lines);

    expect(style.borderLeftWidth).toBe("1px");
    expect(style.borderRightWidth).toBe("1px");
    expect(style.borderLeftStyle).toBe("solid");
    expect(style.borderRightStyle).toBe("solid");
  });

  it("draws them at rest rather than waiting to be hovered", () => {
    // The whole of #138. A grip that is transparent until the pointer is on it
    // tells somebody it is draggable only once they have guessed that it is.
    const { lines } = drawTheGrip();

    expect(getComputedStyle(lines).opacity).toBe("1");
    expect(getComputedStyle(lines).visibility).toBe("visible");
  });

  it("takes its colour from a token rather than a literal", () => {
    // `currentcolor` on the lines, so the one `color` below is the only place
    // the grip's colour is written down.
    const { grip } = drawTheGrip();

    expect(getComputedStyle(grip).color).toBe("var(--line-strong)");
  });

  it("names a token that tokens.css actually defines", () => {
    // Otherwise the assertion above passes against a variable nothing sets and
    // the lines come out the inherited text colour.
    drawTheGrip();

    expect(
      getComputedStyle(document.documentElement)
        .getPropertyValue("--line-strong")
        .trim(),
    ).not.toBe("");
  });

  it("writes no colour of its own anywhere in the file", () => {
    drawTheGrip();

    const literals = everyRule().filter(
      (rule) => rule.startsWith(".grip") && /#[0-9a-f]{3}|rgb\(|oklch\(/i.test(rule),
    );

    expect(literals).toEqual([]);
  });
});

describe("the grip as something to take hold of", () => {
  it("is wide enough for a hand to land on", () => {
    // A one pixel target is one nobody can hit, which is why the grip is wider
    // than the rule it sits on.
    const { grip } = drawTheGrip();

    expect(parseFloat(getComputedStyle(grip).width)).toBeGreaterThanOrEqual(
      HITTABLE,
    );
  });

  it("shows the cursor that says which way it moves", () => {
    const { grip } = drawTheGrip();

    expect(getComputedStyle(grip).cursor).toBe("col-resize");
  });

  it("brightens under the pointer and under focus, from a token each time", () => {
    drawTheGrip();

    const lit = everyRule().filter(
      (rule) =>
        rule.startsWith(".grip:hover") || rule.startsWith(".grip:focus-visible"),
    );

    expect(lit.length).toBeGreaterThan(0);
    for (const rule of lit) expect(rule).toContain("var(--");
  });
});
