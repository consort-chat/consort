/**
 * What a chat effect looks like, and when one plays.
 *
 * Which msgtype means which effect is Rust's: see `consort_matrix::slash`.
 * What is here is the drawing and the one rule about arrivals, both of which
 * are worth testing without a browser.
 */
import { appearanceSettings, type Effect, type Message } from "./api";

/** How many glyphs an effect throws across the room. */
export const PARTICLES = 48;

/** How long the last of them waits before it starts, in seconds. */
const SPREAD = 0.9;

/** One effect, as something to draw. */
interface Look {
  /** The glyphs it throws, handed out in turn so a burst is not uniform. */
  glyphs: readonly [string, ...string[]];
  /** Which way they travel. */
  drift: "fall" | "rise";
  /** How long one glyph takes to cross, in seconds. */
  seconds: number;
}

const LOOKS: Record<Effect, Look> = {
  confetti: { glyphs: ["🎉", "🎊", "✨"], drift: "fall", seconds: 2.6 },
  fireworks: { glyphs: ["🎆", "🎇", "✨"], drift: "rise", seconds: 2.4 },
  rainfall: { glyphs: ["💧", "🌧️"], drift: "fall", seconds: 1.6 },
  snowfall: { glyphs: ["❄️", "🌨️"], drift: "fall", seconds: 3.4 },
  spaceInvaders: { glyphs: ["👾", "🛸"], drift: "fall", seconds: 3 },
  hearts: { glyphs: ["💝", "💖", "💗"], drift: "rise", seconds: 2.8 },
};

/** One glyph on its way across the room. */
export interface Particle {
  glyph: string;
  /** How far across, in percent of the room's width. */
  left: number;
  /** How long before it starts, in seconds. */
  delay: number;
  /** How long it takes, in seconds. */
  seconds: number;
  /** Its size, as a multiplier on the base one. */
  scale: number;
}

/** Which way `effect` travels, for the keyframes that move it. */
export function driftOf(effect: Effect): "fall" | "rise" {
  return LOOKS[effect].drift;
}

/**
 * The glyphs `effect` throws, in the order they are drawn.
 *
 * Spread by arithmetic rather than at random, so what is on screen is the same
 * every time and a test can say what it should be. The three multipliers are
 * coprime with the count, which is what keeps the positions from falling into
 * stripes.
 */
export function particlesFor(effect: Effect): Particle[] {
  const look = LOOKS[effect];
  return Array.from({ length: PARTICLES }, (_unused, index) => ({
    // The modulo cannot be out of range; the type system does not follow that.
    glyph: look.glyphs[index % look.glyphs.length] ?? look.glyphs[0],
    left: (index * 37) % 100,
    delay: (((index * 17) % 100) / 100) * SPREAD,
    seconds: look.seconds * (0.8 + ((index * 23) % 40) / 100),
    scale: 0.7 + ((index * 29) % 70) / 100,
  }));
}

/** How long `effect` needs before the last glyph is gone, in milliseconds. */
export function playsFor(effect: Effect): number {
  const longest = particlesFor(effect).reduce(
    (most, particle) => Math.max(most, particle.delay + particle.seconds),
    0,
  );
  return Math.ceil(longest * 1000);
}

/**
 * Whether the desktop asked for less motion.
 *
 * Read here rather than in CSS because the honest answer for an effect is not
 * to play it. A `@media` block that only removed the animation would leave
 * fifty glyphs sitting still across the room, which is worse than either.
 */
export function motionWanted(): boolean {
  return !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** How far a room has been watched, for deciding what counts as an arrival. */
export interface Watched {
  roomId: string;
  /** The newest `at` already on screen, in milliseconds. */
  at: number;
}

/**
 * The effect a message that has just arrived asks for, and what to hold.
 *
 * Three things this has to get right, and each is a way of playing nothing:
 *
 * - A room's first timeline plays nothing. A backlog with a 🎉 somewhere in it
 *   is a conversation, not an arrival, and opening a room should not set off
 *   whatever happened in it last week.
 * - A page of older history plays nothing either, which is why this watches
 *   the newest timestamp rather than which IDs have been seen: paging back
 *   adds messages that are new to this side and old to the room.
 * - This session's own messages play nothing here. Sending one already played
 *   it, at the keypress, rather than a homeserver round trip later.
 */
export function arriving(
  held: Watched | null,
  roomId: string,
  messages: readonly Message[],
  selfId: string,
): { watched: Watched; effect: Effect | null } {
  const newest = messages.reduce((most, message) => Math.max(most, message.at), 0);

  if (held === null || held.roomId !== roomId) {
    return { watched: { roomId, at: newest }, effect: null };
  }

  const arrived = messages.filter(
    (message) => message.at > held.at && message.sender !== selfId,
  );
  const asking = arrived.filter((message) => message.effect !== undefined).at(-1);

  return {
    watched: { roomId, at: Math.max(held.at, newest) },
    effect: asking?.effect ?? null,
  };
}

/*
  Who is drawing effects, which is the room pane. An announcer rather than a
  prop, on the same terms as `onZoomed` in `lib/scale.ts`: the thread panel is
  a sibling of the room pane rather than a child, so a command typed into a
  thread has no prop to hand its effect back along.
*/
const watching = new Set<(effect: Effect) => void>();

/** Draw whatever is announced. Returns the unwatch, for an effect cleanup. */
export function onEffect(handler: (effect: Effect) => void): () => void {
  watching.add(handler);
  return () => {
    watching.delete(handler);
  };
}

/**
 * Play `effect`, if anything should. Null plays nothing, which is what an
 * ordinary message resolves to.
 *
 * The switch is read here, each time, rather than held: somebody who turns
 * effects off in settings has a room already open behind the modal, and a
 * remembered answer would go on throwing confetti at them until they changed
 * rooms.
 *
 * Rejects when the settings cannot be read, which the caller already has
 * somewhere to say. Swallowing it here would make an effect that silently
 * never plays indistinguishable from one that is switched off.
 */
export async function playEffect(effect: Effect | null): Promise<void> {
  if (effect === null) return;
  const settings = await appearanceSettings();
  if (!settings.chatEffects) return;
  for (const handler of watching) handler(effect);
}
