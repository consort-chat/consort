import { useEffect, useState } from "react";

import { asCommandError, roomAvatar } from "../lib/api";
import "./RoomAvatar.css";

/**
 * Data URLs already fetched, keyed by room ID.
 *
 * Module-level rather than component state, because the same room is drawn in
 * the rail and again in the channel list, and every remount would otherwise be
 * another IPC round trip. `null` is a real entry: it means we asked and there
 * is nothing, which is worth remembering so that a room with no avatar is
 * asked about once rather than once per render.
 *
 * The Rust side caches the bytes on disk, so this is not about the homeserver.
 * It is about the round trip, which is per render and would show as a flicker.
 */
const fetched = new Map<string, string | null>();

/**
 * Asks in flight, so two components wanting the same room make one request.
 *
 * The rail and the channel list mount together, so this is the ordinary case
 * rather than a race worth ignoring.
 */
const asking = new Map<string, Promise<string | null>>();

/**
 * Empty the cache.
 *
 * Test-only in practice. A module-level cache outlives a component, which is
 * the point, but it also outlives a test, which is not: without this the
 * second test in a file gets the first one's answers.
 */
export function resetRoomAvatarCache(): void {
  fetched.clear();
  asking.clear();
}

/** One request per room, shared by everything that asked while it was open. */
function ask(roomId: string): Promise<string | null> {
  const existing = asking.get(roomId);
  if (existing) return existing;

  const request = roomAvatar(roomId)
    .catch((raw: unknown) => {
      // Cosmetic. An avatar that will not load falls back to initials, which
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

/** The letter to draw when there is no picture. */
export function initialsOf(name: string): string {
  const first = [...name.replace(/^[!#@]/, "").trim()][0];
  return first ? first.toUpperCase() : "?";
}

interface Props {
  roomId: string;
  /** What to draw when there is no image. Never a room ID. */
  name: string;
  /**
   * The `mxc://` URI from the room list, or null.
   *
   * Not fetched from, and that is the whole reason it is here: it says whether
   * asking is worth it. Four rooms in ten have no avatar, and skipping those
   * is the difference between initials appearing at once and appearing after a
   * round trip that was always going to answer nothing.
   */
  avatar: string | null;
  className?: string;
}

/**
 * A room's avatar, or its initial.
 *
 * The room list carries the `mxc://` URI and no bytes, because it is re-sent
 * in full every time anything about it changes. So the picture is asked for
 * here, per room, once.
 */
export function RoomAvatar({ roomId, name, avatar, className }: Props) {
  const [url, setUrl] = useState<string | null>(() =>
    avatar === null ? null : (fetched.get(roomId) ?? null),
  );

  useEffect(() => {
    if (avatar === null) {
      setUrl(null);
      return;
    }

    if (fetched.has(roomId)) {
      setUrl(fetched.get(roomId) ?? null);
      return;
    }

    let cancelled = false;
    void ask(roomId).then((loaded) => {
      if (!cancelled) setUrl(loaded);
    });

    return () => {
      cancelled = true;
    };
  }, [roomId, avatar]);

  return (
    <span className={className ? `avatar ${className}` : "avatar"}>
      {url === null ? (
        <span className="avatar__initial" aria-hidden="true">
          {initialsOf(name)}
        </span>
      ) : (
        /*
          Decorative. Everything drawing one of these gives the surrounding
          control its own accessible name, so an alt here would say the room's
          name a second time.
        */
        <img className="avatar__image" src={url} alt="" />
      )}
    </span>
  );
}
