import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  forgetReaders,
  publishedReaders,
  readersOf,
  resetReaders,
  subscribeToReaders,
} from "./readers";
import type { Readers } from "./api";

const GENERAL = "!general:example.org";
const ADA = "@ada:example.org";
const BOB = "@bob:example.org";
const FIRST = "$first:example.org";
const SECOND = "$second:example.org";

beforeEach(() => {
  resetReaders();
});

function published(readers: Partial<Readers>): Readers {
  return { roomId: GENERAL, main: [], ...readers };
}

describe("what the store answers", () => {
  it("has nothing to say before anything has arrived", () => {
    expect(readersOf(GENERAL, undefined, FIRST)).toBeUndefined();
  });

  it("answers for the message that was asked about", () => {
    publishedReaders(
      published({ main: [{ eventId: FIRST, readers: [ADA] }] }),
    );

    expect(readersOf(GENERAL, undefined, FIRST)?.readers).toEqual([ADA]);
    expect(readersOf(GENERAL, undefined, SECOND)).toBeUndefined();
  });

  it("answers about the room this reader is in, not the one before it", () => {
    // One channel serves whichever room is open, and somebody who changes room
    // twice quickly has two answers in flight.
    publishedReaders(
      published({ main: [{ eventId: FIRST, readers: [ADA] }] }),
    );

    expect(readersOf("!other:example.org", undefined, FIRST)).toBeUndefined();
  });

  it("keeps a thread's readers apart from the room's", () => {
    publishedReaders(
      published({
        main: [{ eventId: FIRST, readers: [ADA] }],
        thread: { rootId: "$root:example.org", on: [{ eventId: FIRST, readers: [BOB] }] },
      }),
    );

    expect(readersOf(GENERAL, undefined, FIRST)?.readers).toEqual([ADA]);
    expect(
      readersOf(GENERAL, "$root:example.org", FIRST)?.readers,
    ).toEqual([BOB]);
  });

  it("says nothing about a thread other than the one open", () => {
    publishedReaders(
      published({
        thread: { rootId: "$root:example.org", on: [{ eventId: FIRST, readers: [BOB] }] },
      }),
    );

    expect(readersOf(GENERAL, "$elsewhere:example.org", FIRST)).toBeUndefined();
  });
});

describe("what re-renders when a receipt arrives", () => {
  it("hands back the same object for a message nobody moved past", () => {
    /*
      The whole point of the store, and the thing a `useSyncExternalStore`
      caller depends on: an unchanged answer has to be the same reference, or
      every row of faces in the room re-renders every time one person reads
      something. Measured at 6.3ms against 0.14ms, which is the difference
      between a conversation that sits still and one that does not.
    */
    publishedReaders(
      published({
        main: [
          { eventId: FIRST, readers: [ADA] },
          { eventId: SECOND, readers: [BOB] },
        ],
      }),
    );
    const before = readersOf(GENERAL, undefined, FIRST);

    // Bob reads on. Ada has not moved.
    publishedReaders(
      published({
        main: [
          { eventId: FIRST, readers: [ADA] },
          { eventId: SECOND, readers: [BOB, "@cleo:example.org"] },
        ],
      }),
    );

    expect(readersOf(GENERAL, undefined, FIRST)).toBe(before);
  });

  it("hands back a new object for a message somebody did move past", () => {
    publishedReaders(
      published({ main: [{ eventId: SECOND, readers: [BOB] }] }),
    );
    const before = readersOf(GENERAL, undefined, SECOND);

    publishedReaders(
      published({ main: [{ eventId: SECOND, readers: [ADA, BOB] }] }),
    );

    expect(readersOf(GENERAL, undefined, SECOND)).not.toBe(before);
    expect(readersOf(GENERAL, undefined, SECOND)?.readers).toEqual([ADA, BOB]);
  });

  it("tells a subscriber when something arrives", () => {
    const told = vi.fn();
    subscribeToReaders(told);

    publishedReaders(
      published({ main: [{ eventId: FIRST, readers: [ADA] }] }),
    );

    expect(told).toHaveBeenCalled();
  });

  it("keeps telling a subscriber after a room change forgets the answers", () => {
    /*
      A room change empties the store, and the rows on screen own the
      subscriptions. Emptying those as well would leave a row that is never
      told anything again, and a message whose faces never arrive.
    */
    const told = vi.fn();
    subscribeToReaders(told);

    forgetReaders();
    publishedReaders(
      published({ main: [{ eventId: FIRST, readers: [ADA] }] }),
    );

    expect(readersOf(GENERAL, undefined, FIRST)?.readers).toEqual([ADA]);
    expect(told).toHaveBeenCalledTimes(2);
  });

  it("tells a subscriber that the answers have been forgotten", () => {
    // A room change has to reach the rows, or the last room's faces sit under
    // this room's messages until somebody reads something.
    publishedReaders(
      published({ main: [{ eventId: FIRST, readers: [ADA] }] }),
    );
    const told = vi.fn();
    subscribeToReaders(told);

    forgetReaders();

    expect(readersOf(GENERAL, undefined, FIRST)).toBeUndefined();
    expect(told).toHaveBeenCalled();
  });

  it("stops telling a subscriber that has gone", () => {
    const told = vi.fn();
    const stop = subscribeToReaders(told);
    stop();

    publishedReaders(
      published({ main: [{ eventId: FIRST, readers: [ADA] }] }),
    );

    expect(told).not.toHaveBeenCalled();
  });
});
