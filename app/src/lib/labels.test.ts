import { describe, expect, it } from "vitest";

import { elapsedLabel, presenceLabel, standingLabel } from "./labels";

/** An arbitrary fixed "now", so nothing here depends on the clock. */
const NOW = 1_700_000_000_000;

function ago(seconds: number): number {
  return NOW - seconds * 1000;
}

describe("elapsedLabel", () => {
  it("says just now for the first minute", () => {
    expect(elapsedLabel(ago(0), NOW)).toBe("Just now");
    expect(elapsedLabel(ago(59), NOW)).toBe("Just now");
  });

  it("says just now for a join stamped slightly in the future", () => {
    // A clock a few seconds ahead of the server. Without the guard this reads
    // as an hour short of a day and counts backwards.
    expect(elapsedLabel(NOW + 4_000, NOW)).toBe("Just now");
  });

  it("counts whole minutes, then whole hours, then whole days", () => {
    expect(elapsedLabel(ago(60), NOW)).toBe("1 minute");
    expect(elapsedLabel(ago(59 * 60), NOW)).toBe("59 minutes");
    expect(elapsedLabel(ago(60 * 60), NOW)).toBe("1 hour");
    expect(elapsedLabel(ago(23 * 3600), NOW)).toBe("23 hours");
    expect(elapsedLabel(ago(24 * 3600), NOW)).toBe("1 day");
    expect(elapsedLabel(ago(50 * 3600), NOW)).toBe("2 days");
  });

  it("rounds down rather than to the nearest", () => {
    // Somebody who has been here 119 seconds has been here a minute, not two.
    // Rounding up would claim time that has not passed.
    expect(elapsedLabel(ago(119), NOW)).toBe("1 minute");
  });
});

describe("presenceLabel", () => {
  it("has a phrase for every state, including not knowing", () => {
    expect(presenceLabel("online")).toBe("Online");
    expect(presenceLabel("idle")).toBe("Idle");
    expect(presenceLabel("offline")).toBe("Offline");
    expect(presenceLabel("unknown")).toBe("Status unknown");
  });
});

describe("standingLabel", () => {
  it("names the two standings that change what somebody can do", () => {
    expect(standingLabel("admin")).toBe("Admin");
    expect(standingLabel("moderator")).toBe("Moderator");
  });

  it("says nothing about an ordinary member", () => {
    // A badge on everybody is a badge that says nothing.
    expect(standingLabel("member")).toBeNull();
  });
});
