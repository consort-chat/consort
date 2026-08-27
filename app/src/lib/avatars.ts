/**
 * Avatars, fetched once and remembered.
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
import { asCommandError, memberAvatar, roomAvatar } from "./api";

/**
 * Data URLs already fetched, keyed by whatever identifies the picture.
 *
 * `null` is a real entry: it means we asked and there is nothing, which is
 * worth remembering so that an avatar that will not load is asked about once
 * rather than once per render.
 */
const fetched = new Map<string, string | null>();

/**
 * Asks in flight, so two callers wanting the same picture make one request.
 *
 * The rail and the channel list mount together, so this is the ordinary case
 * rather than a race worth ignoring.
 */
const asking = new Map<string, Promise<string | null>>();

/**
 * What a room and a person in it are cached under.
 *
 * A slash cannot appear in a room id, so a member key can never collide with
 * the plain room key that sits beside it in the same map.
 */
export function memberKey(roomId: string, userId: string): string {
  return `${roomId}/${userId}`;
}

/** What is already known under a key, without asking. */
export function cachedAvatar(key: string): string | null | undefined {
  return fetched.get(key);
}

/** One request per key, shared by everything that asked while it was open. */
function once(
  key: string,
  ask: () => Promise<string | null>,
): Promise<string | null> {
  const existing = asking.get(key);
  if (existing) return existing;

  const request = ask()
    .catch((raw: unknown) => {
      // Cosmetic. An avatar that will not load falls back to an initial, which
      // is a working interface, and a dialog about it would be worse.
      console.error("could not load an avatar", key, asCommandError(raw).detail);
      return null;
    })
    .then((url) => {
      fetched.set(key, url);
      asking.delete(key);
      return url;
    });

  asking.set(key, request);
  return request;
}

/** One room's avatar. */
export function avatarFor(roomId: string): Promise<string | null> {
  return once(roomId, () => roomAvatar(roomId));
}

/**
 * One person's avatar in one room.
 *
 * The room is part of the question because a Matrix profile is per room:
 * somebody can carry a different picture in every room they are in.
 */
export function memberAvatarFor(
  roomId: string,
  userId: string,
): Promise<string | null> {
  return once(memberKey(roomId, userId), () => memberAvatar(roomId, userId));
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
