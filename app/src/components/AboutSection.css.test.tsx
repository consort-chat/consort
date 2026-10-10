/*
  The one thing about the About pane that only the stylesheet holds: how big
  the control is. A button takes its height from the words inside it, which is
  the shape this repository has been under SC 2.5.8 with three times (#82,
  #102), and reading the rule rather than measuring it is what failed each
  time.

  A file of its own, because it is the only kind of test that wants CSS. See
  `vitest.config.ts` for why `.css.test.tsx` is the name that gets one.
*/
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import "./AboutSection.css";

const appVersion = vi.hoisted(() => vi.fn());
const updatesItself = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  appVersion,
  updatesItself,
  updateCheck: vi.fn(),
}));

import { AboutSection } from "./AboutSection";

/** What SC 2.5.8 (Target Size, Minimum) asks for, in CSS pixels. */
const FLOOR = 24;

/** Every rule jsdom parsed. The authored units are reachable no other way. */
function everyRule(): readonly string[] {
  return Array.from(document.styleSheets).flatMap((sheet) =>
    Array.from(sheet.cssRules).map((rule) => rule.cssText),
  );
}

async function theCheck(): Promise<HTMLElement> {
  appVersion.mockResolvedValue("0.12.0");
  updatesItself.mockResolvedValue(true);
  render(<AboutSection />);
  return screen.findByRole("button", { name: /check for updates/i });
}

describe("the check for updates button", () => {
  it("is measured against the text size rather than in pixels", async () => {
    // A number in `px` here holds at 100% and shrinks against the words
    // beside it at the 150% #122 allows. The authored unit is what says so:
    // jsdom resolves `rem` to pixels before a computed style is read.
    await theCheck();
    const rule = everyRule().filter((one) => one.startsWith(".about__check {"));

    expect(rule).toHaveLength(1);
    expect(rule.join("")).toMatch(/min-height: [\d.]+rem/);
    expect(rule.join("")).toMatch(/min-width: [\d.]+rem/);
  });

  it("is big enough to hit at the ordinary text size", async () => {
    const check = await theCheck();

    expect(parseFloat(getComputedStyle(check).minHeight)).toBeGreaterThanOrEqual(FLOOR);
    expect(parseFloat(getComputedStyle(check).minWidth)).toBeGreaterThanOrEqual(FLOOR);
  });
});
