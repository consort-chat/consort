/**
 * Room avatars, fetched once and remembered.
 *
 * The room list carries `mxc://` URIs and no bytes, because it is re-sent in
 * full whenever anything about it changes. So the pictures are asked for one
 * at a time, and this is what stops that being one request per render.
 *
 * Module state rather than component state, because the same room is drawn in
 * the rail and again in the channel list. Not a component file, because a
 * module exporting both a component and a plain function cannot be hot
 * reloaded.
 */
import { asCommandError, roomAvatar } from "./api";

/**
 * Data URLs already fetched, keyed by room ID.
 *
 * `null` is a real entry: it means we asked and there is nothing, which is
 * worth remembering so that a room whose avatar will not load is asked about
 * once rather than once per render.
 */
const fetched = new Map<string, string | null>();

/**
 * Asks in flight, so two callers wanting the same room make one request.
 *
 * The rail and the channel list mount together, so this is the ordinary case
 * rather than a race worth ignoring.
 */
const asking = new Map<string, Promise<string | null>>();

/** What is already known about a room, without asking. */
export function cachedAvatar(roomId: string): string | null | undefined {
  return fetched.get(roomId);
}

/** One request per room, shared by everything that asked while it was open. */
export function avatarFor(roomId: string): Promise<string | null> {
  const existing = asking.get(roomId);
  if (existing) return existing;

  const request = roomAvatar(roomId)
    .catch((raw: unknown) => {
      // Cosmetic. An avatar that will not load falls back to an initial, which
      // is a working interface, and a dialog about it would be worse.
      console.error(
        "could not load a room avatar",
        roomId,
        asCommandError(raw).detail,
      );
      return null;
    })
    .then((url) => {
      fetched.set(roomId, url);
      asking.delete(roomId);
      return url;
    });

  asking.set(roomId, request);
  return request;
}

/**
 * Empty the cache.
 *
 * Test-only in practice. Module state outliving a component is the point, and
 * outliving a test is not: without this the second test in a file gets the
 * first one's answers.
 */
export function resetAvatarCache(): void {
  fetched.clear();
  asking.clear();
}
