import { useEffect, useState } from "react";

import { presenceLabel } from "../lib/labels";
import { cachedPresence, presenceFor } from "../lib/presence";
import type { Presence } from "../lib/api";
import "./PresenceDot.css";

/**
 * Where somebody is, as a dot on the corner of their picture.
 *
 * Nothing is drawn until the homeserver has answered, and nothing at all is
 * drawn when the answer is that nobody would say. Both are deliberate. A grey
 * dot on somebody sitting right there is a claim this build cannot support,
 * and presence is switched off on most homeservers of any size, so "no dot" is
 * the ordinary case rather than a failure.
 *
 * The word lives on the pointer. A colour means nothing on its own, and a
 * label beside every avatar would be a second name down the length of a room.
 */
export function PresenceDot({ userId }: { userId: string }) {
  const [presence, setPresence] = useState<Presence | null>(
    () => cachedPresence(userId) ?? null,
  );

  useEffect(() => {
    let cancelled = false;
    void presenceFor(userId).then((answer) => {
      if (!cancelled) setPresence(answer);
    });

    return () => {
      cancelled = true;
    };
  }, [userId]);

  if (presence === null || presence === "unknown") return null;

  const label = presenceLabel(presence);
  return (
    <span
      className="presence"
      data-presence={presence}
      role="img"
      aria-label={label}
      title={label}
    />
  );
}
