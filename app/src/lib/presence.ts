/**
 * Where people are, asked for once each and remembered.
 *
 * The timeline draws an avatar per group and a busy room has six groups from
 * the same person, so without this a burst of messages would be a burst of
 * requests for one answer.
 *
 * Module state rather than component state, on the same reasoning as
 * `lib/avatars`: the same person is drawn in several places at once, and a
 * component that owned the answer would ask again on every remount.
 *
 * ## Asked once, and not again
 *
 * Presence is a snapshot here, not a subscription. Somebody who goes offline
 * while their messages are on screen keeps their dot until the room is
 * reopened. That is a smaller lie than the alternatives: the homeserver only
 * pushes presence to clients that ask it to, most of them are configured not
 * to answer at all, and a dot that changed under a message somebody was
 * reading would be motion in the corner of the eye for no gain.
 */
import { asCommandError, memberProfile, type Presence } from "./api";
import { memberKey } from "./avatars";

/** What is known about each person, keyed by [`memberKey`]. */
const known = new Map<string, Presence>();

/** Asks in flight, so three dots about one person make one request. */
const asking = new Map<string, Promise<Presence>>();

/** What is already known about somebody, without asking. */
export function cachedPresence(key: string): Presence | undefined {
  return known.get(key);
}

/**
 * Where one person is, in one room.
 *
 * Never rejects. "Unknown" is a real state with its own drawing, so a
 * homeserver that refuses to answer and a homeserver that answers "nobody
 * would say" reach the interface as the same thing.
 */
export function presenceFor(roomId: string, userId: string): Promise<Presence> {
  const key = memberKey(roomId, userId);
  const existing = asking.get(key);
  if (existing) return existing;

  const request = memberProfile(roomId, userId)
    .then((profile) => profile.presence)
    .catch((raw: unknown) => {
      console.error("could not read a presence", key, asCommandError(raw).detail);
      return "unknown" as const;
    })
    .then((presence) => {
      known.set(key, presence);
      asking.delete(key);
      return presence;
    });

  asking.set(key, request);
  return request;
}

/**
 * Empty the cache.
 *
 * Test-only in practice, for the reason `resetAvatarCache` is: module state
 * outliving a component is the point and outliving a test is not.
 */
export function resetPresenceCache(): void {
  known.clear();
  asking.clear();
}
