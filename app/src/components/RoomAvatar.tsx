import { useEffect, useState } from "react";

import {
  avatarFor,
  cachedAvatar,
  memberAvatarFor,
  memberKey,
} from "../lib/avatars";
import { initialsOf } from "../lib/labels";
import "./RoomAvatar.css";

interface Props {
  roomId: string;
  /**
   * Set to draw the avatar of this person in that room rather than the room's
   * own. A Matrix profile is per room, which is why the room is still here.
   */
  userId?: string;
  /** What to draw when there is no image. Never an ID. */
  name: string;
  /**
   * The `mxc://` URI from the room list, or null when the room has none.
   *
   * Not fetched from, and that is the whole reason it is here: it says whether
   * asking is worth it. Four rooms in ten have no avatar, and skipping those
   * is the difference between an initial appearing at once and appearing after
   * a round trip that was always going to answer nothing.
   *
   * Left out for a person, because the room list carries no such hint about
   * one, so their picture is asked for and the answer remembered either way.
   */
  avatar?: string | null;
  className?: string;
}

/**
 * A room's avatar, or somebody's in it, or an initial.
 *
 * The room list carries `mxc://` URIs and no bytes, because it is re-sent in
 * full every time anything about it changes. So the picture is asked for here,
 * once per thing drawn. See `lib/avatars` for the once.
 */
export function RoomAvatar({ roomId, userId, name, avatar, className }: Props) {
  const key = userId === undefined ? roomId : memberKey(roomId, userId);
  const known = avatar === null ? null : (cachedAvatar(key) ?? null);
  const [url, setUrl] = useState<string | null>(known);

  useEffect(() => {
    // A room the list says has no avatar. Nothing to ask about, and asking
    // anyway would be a round trip per room per launch for a known answer.
    if (avatar === null) {
      setUrl(null);
      return;
    }

    const cached = cachedAvatar(key);
    if (cached !== undefined) {
      setUrl(cached);
      return;
    }

    let cancelled = false;
    const request =
      userId === undefined ? avatarFor(roomId) : memberAvatarFor(roomId, userId);

    void request.then((loaded) => {
      if (!cancelled) setUrl(loaded);
    });

    return () => {
      cancelled = true;
    };
  }, [key, roomId, userId, avatar]);

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
