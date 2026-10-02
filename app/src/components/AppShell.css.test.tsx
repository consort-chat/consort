/*
  Three facts about the shell's columns that only the stylesheet holds.

  The grip on the channel column's edge is placed by arithmetic over the same
  two custom properties the grid is built from, and it sits outside the box that
  clips the column. Nothing at runtime can check either: jsdom does no layout,
  and in a browser a grip half a column away still looks like a grip.
*/
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import "./AppShell.css";

import { SIDEBAR_WIDE } from "./sidebarWidth";

/** Every rule jsdom parsed, for the ones no element can be asked about. */
function everyRule(): readonly string[] {
  return Array.from(document.styleSheets).flatMap((sheet) =>
    Array.from(sheet.cssRules).map((rule) => rule.cssText),
  );
}

function ruleFor(selector: string): string {
  const found = everyRule().filter((rule) => rule.startsWith(`${selector} {`));
  expect(found).toHaveLength(1);
  return found.join("");
}

describe("the width the channel column opens at", () => {
  it("is the same number in the stylesheet as in the shell", () => {
    // Two places, because the grid needs a floor that cannot be missing and the
    // shell needs a number it can clamp. Nothing else keeps them together.
    const { container } = render(<div className="shell" />);

    expect(
      getComputedStyle(container.firstElementChild as HTMLElement)
        .getPropertyValue("--shell-sidebar")
        .trim(),
    ).toBe(`${SIDEBAR_WIDE}px`);
  });
});

describe("where the grip sits", () => {
  it("is placed from the properties the grid is built from", () => {
    // A literal here would be a grip that stays put while the rail or the
    // column moves, which is a handle floating in the middle of the pane.
    expect(ruleFor(".shell__grip")).toContain(
      "calc(var(--shell-rail) + var(--shell-sidebar))",
    );
    expect(ruleFor(".shell")).toContain("var(--shell-rail)");
    expect(ruleFor(".shell")).toContain("var(--shell-sidebar)");
  });

  it("is outside the box that clips the column", () => {
    // The clip is what keeps a folded column from spilling into the pane and
    // what the voice strip's menu opens against. The grip is placed past the
    // column's edge precisely so that neither has to give.
    expect(ruleFor(".shell__sidebar")).toContain("overflow: hidden");
  });
});
