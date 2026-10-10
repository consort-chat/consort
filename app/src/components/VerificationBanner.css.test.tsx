/*
  The shape of the two controls #59 added, measured rather than read.

  Named `.css.test.tsx` because it reads a stylesheet. See `vitest.config.ts`.
*/
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const verificationOtherSessionsExist = vi.hoisted(() => vi.fn());
const verificationRecoveryExists = vi.hoisted(() => vi.fn());
const verificationWarningDismissed = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  verificationOtherSessionsExist,
  verificationRecoveryExists,
  verificationWarningDismissed,
}));

import "../styles/tokens.css";
import "./Confirm.css";
import "./VerificationBanner.css";

import { VerificationBanner } from "./VerificationBanner";

/** What a browser calls one `rem` before anybody has changed anything. */
const REM = 16;

/** What SC 2.5.8 (Target Size, Minimum) asks for, in CSS pixels. */
const FLOOR = 24;

beforeEach(() => {
  verificationOtherSessionsExist.mockReset().mockResolvedValue(true);
  verificationRecoveryExists.mockReset().mockResolvedValue(false);
  verificationWarningDismissed.mockReset().mockResolvedValue(false);
});

/** The banner, once it has settled on which form to draw. */
async function drawn(
  named: RegExp,
): Promise<{ banner: HTMLElement; button: HTMLElement }> {
  render(
    <VerificationBanner state="unverified" canStart />,
  );
  const button = await screen.findByRole("button", { name: named });
  return {
    banner: screen.getByRole("status", { name: "Session verification" }),
    button,
  };
}

describe("the floor the banner's own controls stand on", () => {
  it("is big enough to hit at the ordinary text size", async () => {
    const { banner } = await drawn(/continue without verifying/i);

    const floor = getComputedStyle(banner)
      .getPropertyValue("--verification-target")
      .trim();

    // In `rem`, so the text size slider takes it along: a length in pixels
    // would stay put while the words inside it grew, which is how a control
    // that passes at 100% fails at 150%.
    expect(floor).toMatch(/rem$/);
    expect(parseFloat(floor) * REM).toBeGreaterThanOrEqual(FLOOR);
  });

  it("is what the control that carries on unverified is sized from", async () => {
    // Otherwise the one above is measuring a custom property nothing uses.
    const { button } = await drawn(/continue without verifying/i);

    expect(getComputedStyle(button).minHeight).toBe(
      "var(--verification-target)",
    );
  });

  it("is what the way back out of the short form is sized from too", async () => {
    verificationWarningDismissed.mockResolvedValue(true);

    const { button } = await drawn(/^verify$/i);

    expect(getComputedStyle(button).minHeight).toBe(
      "var(--verification-target)",
    );
  });
});

describe("where the question hangs", () => {
  it("opens from the left edge of the button that asked it", async () => {
    // `Confirm` lines up with its anchor's right edge by default, which is
    // right for a control at the end of a row and wrong here: this one sits at
    // the left of a left-aligned banner, so the panel would hang off the side.
    render(
      <VerificationBanner state="unverified" canStart />,
    );
    await screen.findByRole("button", { name: /continue without verifying/i });
    const anchor = document.querySelector(
      ".verification__anchor",
    ) as HTMLElement;

    const style = getComputedStyle(anchor);

    expect(style.position).toBe("relative");
    expect(style.getPropertyValue("--confirm-left").trim()).toBe("0");
    expect(style.getPropertyValue("--confirm-right").trim()).toBe("auto");
  });
});
