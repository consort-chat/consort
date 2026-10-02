/*
  How the away clock is drawn, read off the stylesheet that draws it.

  A file of its own, because it is the only kind of test that wants CSS.
  `vitest.config.ts` runs a second project over `.css.test.tsx` with the
  stylesheets processed, and `RoomTimeline.css.test.tsx` says why that cannot
  be on for the whole run.
*/
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const memberAvatar = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberAvatar,
}));

import { CallFace } from "./CallFace";
import { resetAvatarCache } from "../lib/avatars";

const LOUNGE = "!lounge:example.org";

beforeEach(() => {
  resetAvatarCache();
  memberAvatar.mockResolvedValue(null);
});

/** Somebody away, which on this row always means muted as well. */
function drawSomebodyAway() {
  render(
    <ul aria-label="In Lounge">
      <CallFace
        person={{
          id: "@ada:example.org",
          name: "Ada",
          muted: true,
          away: true,
        }}
        roomId={LOUNGE}
        speaking={false}
        live
        onOpen={vi.fn()}
      />
    </ul>,
  );

  return {
    clock: screen.getByLabelText("Ada is away"),
    microphone: screen.getByLabelText("Ada is muted"),
  };
}

/*
  jsdom resolves the cascade but not `var()`, so a colour reads back as the
  token it was written as. That is the half worth pinning here: #144 asked for
  brighter, and the way this went wrong was a rule two files away deciding the
  colour rather than a number being too low. What the token resolves to is
  checked in a browser, where there is one.
*/
describe("the clock beside somebody who is away", () => {
  it("takes the colour away has everywhere else, not the muted grey", () => {
    const { clock } = drawSomebodyAway();

    expect(getComputedStyle(clock).color).toBe("var(--amber)");
  });

  it("is not dimmed by the mute that being away brings with it", () => {
    // The rule that hid it. Away mutes, the mute dims every flag in the row,
    // and the one glyph meant to be noticed was dimmed by its own side effect.
    const { clock, microphone } = drawSomebodyAway();

    expect(Number(getComputedStyle(clock).opacity || 1)).toBe(1);
    expect(Number(getComputedStyle(microphone).opacity)).toBeLessThan(1);
  });

  it("fades slowly enough to read as a pulse rather than a blink", () => {
    // Read off the shorthand, which jsdom keeps whole rather than splitting
    // into longhands. Two seconds is the floor: under it this starts blinking.
    const { clock } = drawSomebodyAway();
    const animation = getComputedStyle(clock).animation;

    expect(animation).toContain("call-face-away-pulse");
    expect(animation).toContain("infinite");
    expect(Number(/([\d.]+)s/.exec(animation)?.[1])).toBeGreaterThanOrEqual(2);
  });
});
