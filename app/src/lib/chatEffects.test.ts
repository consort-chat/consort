import { beforeEach, describe, expect, it, vi } from "vitest";

const appearanceSettings = vi.hoisted(() => vi.fn());
vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  appearanceSettings,
}));

import {
  PARTICLES,
  arriving,
  driftOf,
  motionWanted,
  onEffect,
  particlesFor,
  playEffect,
  playsFor,
  type Watched,
} from "./chatEffects";
import type { Effect, Message } from "./api";

const ADA = "@ada:example.org";
const BOB = "@bob:example.org";
const GENERAL = "!general:example.org";

const EVERY: Effect[] = [
  "confetti",
  "fireworks",
  "rainfall",
  "snowfall",
  "spaceInvaders",
  "hearts",
];

/** One message, from Bob unless a test says otherwise. */
function said(id: string, at: number, extra: Partial<Message> = {}): Message {
  return {
    id,
    sender: BOB,
    at,
    body: "hello",
    kind: "text",
    ...extra,
  };
}

/** Say what `prefers-reduced-motion` answers. */
function reducedMotion(reduce: boolean) {
  window.matchMedia = vi.fn().mockImplementation((media: string) => ({
    matches: reduce && media.includes("prefers-reduced-motion"),
    media,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}

beforeEach(() => {
  appearanceSettings.mockReset().mockResolvedValue({
    applicationScale: 1,
    textScale: 1,
    chatEffects: true,
  });
  reducedMotion(false);
});

describe("what an effect looks like", () => {
  it("throws the same glyphs in the same places every time", () => {
    // Spread by arithmetic rather than at random, which is what lets a test
    // say anything about it at all.
    expect(particlesFor("confetti")).toEqual(particlesFor("confetti"));
  });

  it.each(EVERY)("gives %s a full set of glyphs to throw", (effect) => {
    const particles = particlesFor(effect);

    expect(particles).toHaveLength(PARTICLES);
    expect(particles.every((one) => one.glyph !== "")).toBe(true);
  });

  it("keeps every glyph inside the room", () => {
    expect(
      particlesFor("snowfall").every((one) => one.left >= 0 && one.left < 100),
    ).toBe(true);
  });

  it("starts them at different moments, so a burst is not one wall", () => {
    const delays = new Set(particlesFor("hearts").map((one) => one.delay));

    expect(delays.size).toBeGreaterThan(1);
  });

  it("sends the ones that fall down and the ones that rise up", () => {
    expect(driftOf("confetti")).toBe("fall");
    expect(driftOf("rainfall")).toBe("fall");
    expect(driftOf("snowfall")).toBe("fall");
    expect(driftOf("spaceInvaders")).toBe("fall");
    expect(driftOf("hearts")).toBe("rise");
    expect(driftOf("fireworks")).toBe("rise");
  });

  it.each(EVERY)("waits for the last of %s before it is over", (effect) => {
    const longest = Math.max(
      ...particlesFor(effect).map((one) => one.delay + one.seconds),
    );

    expect(playsFor(effect)).toBeGreaterThanOrEqual(longest * 1000);
  });
});

describe("a desktop that asked for less motion", () => {
  it("is asked, and says so", () => {
    reducedMotion(true);

    expect(motionWanted()).toBe(false);
  });

  it("wants motion when it has asked for nothing", () => {
    expect(motionWanted()).toBe(true);
  });
});

describe("what counts as an arrival", () => {
  it("plays nothing for the backlog a room opens with", () => {
    // Opening a room should not set off whatever happened in it last week.
    const opened = arriving(
      null,
      GENERAL,
      [said("$1", 1_000, { effect: "confetti" })],
      ADA,
    );

    expect(opened.effect).toBeNull();
    expect(opened.watched).toEqual({ roomId: GENERAL, at: 1_000 });
  });

  it("plays nothing for the first timeline of a different room", () => {
    const held: Watched = { roomId: "!other:example.org", at: 500 };

    expect(
      arriving(held, GENERAL, [said("$1", 1_000, { effect: "hearts" })], ADA)
        .effect,
    ).toBeNull();
  });

  it("plays what somebody else has just sent", () => {
    const held: Watched = { roomId: GENERAL, at: 1_000 };

    const next = arriving(
      held,
      GENERAL,
      [said("$1", 1_000), said("$2", 2_000, { effect: "snowfall" })],
      ADA,
    );

    expect(next.effect).toBe("snowfall");
    expect(next.watched).toEqual({ roomId: GENERAL, at: 2_000 });
  });

  it("plays the newest when two arrive at once", () => {
    const held: Watched = { roomId: GENERAL, at: 1_000 };

    expect(
      arriving(
        held,
        GENERAL,
        [
          said("$2", 2_000, { effect: "hearts" }),
          said("$3", 3_000, { effect: "fireworks" }),
        ],
        ADA,
      ).effect,
    ).toBe("fireworks");
  });

  it("plays nothing for a page of older history", () => {
    // What makes this the timestamp rather than the IDs seen: paging back
    // adds messages that are new to this side and old to the room.
    const held: Watched = { roomId: GENERAL, at: 5_000 };

    const paged = arriving(
      held,
      GENERAL,
      [said("$old", 1_000, { effect: "confetti" }), said("$now", 5_000)],
      ADA,
    );

    expect(paged.effect).toBeNull();
    expect(paged.watched.at).toBe(5_000);
  });

  it("plays nothing for this session's own message", () => {
    // Sending one already played it, at the keypress rather than a round trip
    // later, so playing it again on the way back would be twice.
    const held: Watched = { roomId: GENERAL, at: 1_000 };

    expect(
      arriving(
        held,
        GENERAL,
        [said("$2", 2_000, { sender: ADA, effect: "confetti" })],
        ADA,
      ).effect,
    ).toBeNull();
  });

  it("plays nothing for an arrival that asks for nothing", () => {
    const held: Watched = { roomId: GENERAL, at: 1_000 };

    expect(arriving(held, GENERAL, [said("$2", 2_000)], ADA).effect).toBeNull();
  });

  it("holds the room it was last told about", () => {
    expect(arriving(null, GENERAL, [], ADA).watched).toEqual({
      roomId: GENERAL,
      at: 0,
    });
  });
});

describe("announcing an effect", () => {
  it("hands it to whoever is drawing", async () => {
    const drawn = vi.fn();
    const stop = onEffect(drawn);

    await playEffect("confetti");

    expect(drawn).toHaveBeenCalledWith("confetti");
    stop();
  });

  it("announces nothing for an ordinary message", async () => {
    const drawn = vi.fn();
    const stop = onEffect(drawn);

    await playEffect(null);

    expect(drawn).not.toHaveBeenCalled();
    expect(appearanceSettings).not.toHaveBeenCalled();
    stop();
  });

  it("announces nothing once the switch is off", async () => {
    appearanceSettings.mockResolvedValue({
      applicationScale: 1,
      textScale: 1,
      chatEffects: false,
    });
    const drawn = vi.fn();
    const stop = onEffect(drawn);

    await playEffect("hearts");

    expect(drawn).not.toHaveBeenCalled();
    stop();
  });

  it("reads the switch each time, so turning it off takes effect at once", async () => {
    const stop = onEffect(vi.fn());

    await playEffect("hearts");
    await playEffect("hearts");

    expect(appearanceSettings).toHaveBeenCalledTimes(2);
    stop();
  });

  it("says so rather than silently playing nothing when the switch cannot be read", async () => {
    appearanceSettings.mockRejectedValue({ message: "no settings" });
    const drawn = vi.fn();
    const stop = onEffect(drawn);

    await expect(playEffect("confetti")).rejects.toEqual({
      message: "no settings",
    });

    expect(drawn).not.toHaveBeenCalled();
    stop();
  });

  it("stops drawing once the watcher has gone", async () => {
    const drawn = vi.fn();
    onEffect(drawn)();

    await playEffect("confetti");

    expect(drawn).not.toHaveBeenCalled();
  });
});
