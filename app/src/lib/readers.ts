/**
 * Who has read how far, held where a row of faces can ask about one message.
 *
 * Module state rather than component state, on the same reasoning as
 * `lib/presence` and `lib/avatars`: one answer serves every row drawing it,
 * and a component that owned it would ask again on every remount.
 *
 * ## Why this is not a prop
 *
 * It could be. A `Readers` could be threaded down into `MessageGroups` beside
 * the messages, and it would be shorter. It was measured instead, because the
 * issue this answers asked for that, and the measurement said something the
 * obvious design does not:
 *
 * - Receipts riding on `Message`, so a receipt republishes the timeline:
 *   **6.3ms** to redraw fifty messages.
 * - Receipts on a channel of their own, handed to `MessageGroups` as a prop:
 *   **5.3ms**. React re-runs every row's render function whether or not the
 *   messages changed, so a separate channel on its own buys 16%.
 * - Receipts read by the rows that draw them, through `useSyncExternalStore`:
 *   **0.14ms**.
 *
 * A busy room produces about as many receipts as messages, so the first two
 * are a conversation that redraws itself roughly once a second while nothing
 * in it has changed. That is what this file exists to avoid, and the
 * per-message reference stability below is the whole mechanism: an unchanged
 * row is handed back the object it already had, so React leaves it alone.
 */
import type { ReadOn, Readers } from "./api";

/**
 * Which conversation a row of faces belongs to.
 *
 * A space between the two, which is unambiguous because neither a room ID nor
 * an event ID can contain one.
 */
function scopeOf(roomId: string, threadRoot: string | undefined): string {
  return threadRoot === undefined ? roomId : `${roomId} ${threadRoot}`;
}

/** The latest answer, by conversation and then by message. */
let held = new Map<string, Map<string, ReadOn>>();

/** Everyone waiting to be told, which is one per row of faces on screen. */
const listeners = new Set<() => void>();

/**
 * Who has read up to one message, or nothing if nobody this account can see.
 *
 * The same object for as long as the answer has not changed, which is what
 * lets a row bail out of re-rendering. See the module docs for why that is the
 * point rather than a nicety.
 */
export function readersOf(
  roomId: string,
  threadRoot: string | undefined,
  eventId: string,
): ReadOn | undefined {
  return held.get(scopeOf(roomId, threadRoot))?.get(eventId);
}

/** Be told when any of it changes. Returns the way to stop being told. */
export function subscribeToReaders(told: () => void): () => void {
  listeners.add(told);
  return () => {
    listeners.delete(told);
  };
}

/**
 * Take one answer from Rust.
 *
 * Rebuilt rather than merged, because the published value is the whole truth
 * about that room: a person who has gone quiet is absent from it, and merging
 * would leave their face where they used to be for ever.
 *
 * What is carried over is the objects. Every message whose readers are the
 * same as last time keeps the object it had, so a row that has not changed is
 * handed back a reference it recognises and does not re-render. A receipt in a
 * busy room moves one row and leaves the other forty-nine alone.
 */
export function publishedReaders(readers: Readers): void {
  const next = new Map<string, Map<string, ReadOn>>();
  next.set(
    scopeOf(readers.roomId, undefined),
    carriedOver(scopeOf(readers.roomId, undefined), readers.main),
  );
  if (readers.thread !== undefined) {
    const scope = scopeOf(readers.roomId, readers.thread.rootId);
    next.set(scope, carriedOver(scope, readers.thread.on));
  }

  held = next;
  for (const told of listeners) told();
}

/** One conversation's answer, keeping whatever object each row already had. */
function carriedOver(scope: string, on: ReadOn[]): Map<string, ReadOn> {
  const before = held.get(scope);
  const after = new Map<string, ReadOn>();

  for (const one of on) {
    const had = before?.get(one.eventId);
    after.set(one.eventId, had !== undefined && same(had, one) ? had : one);
  }
  return after;
}

/** Whether two answers about one message say the same thing. */
function same(one: ReadOn, two: ReadOn): boolean {
  return (
    (one.more ?? 0) === (two.more ?? 0) &&
    one.readers.length === two.readers.length &&
    one.readers.every((who, at) => who === two.readers[at])
  );
}

/**
 * Forget every answer, keeping whoever is waiting to be told.
 *
 * What a room change calls. The faces belong to the room they were published
 * about, and a session that visited forty rooms would otherwise be holding
 * forty answers nothing will ever ask for again.
 *
 * Deliberately not [`resetReaders`], which also empties the listeners. Those
 * belong to the rows currently on screen: taking them away is taking away the
 * subscription `useSyncExternalStore` set up, and a row whose subscription has
 * gone is never told anything again.
 */
export function forgetReaders(): void {
  held = new Map();
  for (const told of listeners) told();
}

/**
 * Empty the store, listeners and all.
 *
 * Test-only, for the reason `resetPresenceCache` is: module state outliving a
 * component is the point and outliving a test is not. Production code wants
 * [`forgetReaders`]; see there for why the difference matters.
 */
export function resetReaders(): void {
  held = new Map();
  listeners.clear();
}
