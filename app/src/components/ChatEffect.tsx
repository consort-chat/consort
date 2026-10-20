import { useEffect, useState } from "react";

import type { Effect } from "../lib/api";
import {
  driftOf,
  motionWanted,
  particlesFor,
  playsFor,
} from "../lib/chatEffects";
import "./ChatEffect.css";

/**
 * A chat effect, playing over the room.
 *
 * `effect` is what to play and `onDone` is called when the last glyph has
 * gone, so whoever holds the ask can clear it and let the next one start.
 *
 * Nothing is drawn when the desktop asked for reduced motion, and `onDone` is
 * called immediately instead: an overlay that only stopped animating would
 * leave fifty glyphs standing across the room, which is worse than either
 * playing it or not. The switch in Accessibility is checked by the caller,
 * which is where the setting already is.
 *
 * `aria-hidden`, and deliberately. The message that asked for this is in the
 * room with its words on it, so the animation is decoration for the eye; a
 * screen reader announcing fifty party poppers would be the one reading of it
 * that is worse than silence.
 */
export function ChatEffect({
  effect,
  onDone,
}: {
  effect: Effect;
  onDone: () => void;
}) {
  const [wanted] = useState(motionWanted);

  useEffect(() => {
    if (!wanted) {
      onDone();
      return;
    }
    const over = window.setTimeout(onDone, playsFor(effect));
    return () => window.clearTimeout(over);
  }, [effect, wanted, onDone]);

  if (!wanted) return null;

  return (
    <div
      className="effect"
      data-effect={effect}
      data-drift={driftOf(effect)}
      aria-hidden="true"
    >
      {particlesFor(effect).map((particle, index) => (
        <span
          key={index}
          className="effect__glyph"
          style={{
            left: `${particle.left}%`,
            animationDelay: `${particle.delay}s`,
            animationDuration: `${particle.seconds}s`,
            scale: `${particle.scale}`,
          }}
        >
          {particle.glyph}
        </span>
      ))}
    </div>
  );
}
