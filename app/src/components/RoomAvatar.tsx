import { useEffect, useState } from "react";

import { avatarFor, cachedAvatar } from "../lib/avatars";
import { initialsOf } from "../lib/labels";
import "./RoomAvatar.css";

interface Props {
  roomId: string;
  /** What to draw when there is no image. Never a room ID. */
  name: string;
  /**
   * The `mxc://` URI from the room list, or null.
   *
   * Not fetched from, and that is the whole reason it is here: it says whether
   * asking is worth it. Four rooms in ten have no avatar, and skipping those
   * is the difference between an initial appearing at once and appearing after
   * a round trip that was always going to answer nothing.
   */
  avatar: string | null;
  className?: string;
}

/**
 * A room's avatar, or its initial.
 *
 * The room list carries the `mxc://` URI and no bytes, because it is re-sent
 * in full every time anything about it changes. So the picture is asked for
 * here, per room, once. See `lib/avatars` for the once.
 */
export function RoomAvatar({ roomId, name, avatar, className }: Props) {
  const [url, setUrl] = useState<string | null>(() =>
    avatar === null ? null : (cachedAvatar(roomId) ?? null),
  );

  useEffect(() => {
    if (avatar === null) {
      setUrl(null);
      return;
    }

    const known = cachedAvatar(roomId);
    if (known !== undefined) {
      setUrl(known);
      return;
    }

    let cancelled = false;
    void avatarFor(roomId).then((loaded) => {
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
