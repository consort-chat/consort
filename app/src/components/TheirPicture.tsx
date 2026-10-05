import { useTheirPicture, type Sending } from "../lib/usePicture";
import "./CallPicture.css";

/** What each picture is called, for somebody who cannot see it. */
const ALT: Record<Sending, (who: string) => string> = {
  camera: (who) => `${who}'s camera`,
  screen: (who) => `${who}'s screen`,
};

interface Props {
  /** Whose picture this is. The key Rust answers by. */
  userId: string;
  /** What to call them, for the one person who cannot see the picture. */
  name: string;
  /** Their camera, or the screen they are sharing. */
  of: Sending;
  /**
   * How many pixels to ask for on the long edge.
   *
   * The size of the box this is drawn into, which the call site knows and this
   * does not. A square asks for little and the stage asks for a lot, off one
   * frame either way.
   */
  bound: number;
}

/**
 * A still of what somebody else in the call is sending, replaced on a timer.
 *
 * [`SelfPicture`]'s twin and its own component for the same reason: a frame
 * arriving twelve and a half times a second has to redraw this and not the
 * faces beside it, which is the trap #142 found.
 *
 * A still rather than a `<video>`, because there is no stream for one to play:
 * the frames are decoded in Rust and the webview cannot reach the SFU. See
 * `docs/adr/0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md`.
 */
export function TheirPicture({ userId, name, of, bound }: Props) {
  const picture = useTheirPicture(userId, of, bound);

  if (picture === null) return null;

  return (
    <img
      className="call-picture"
      data-of={of}
      src={picture}
      /* Named rather than decorative: whose face or whose desktop this is is
         the whole content of the square. */
      alt={ALT[of](name)}
      /* Drawn at a known shape, so a frame arriving does not move anything. */
      width={bound}
      height={bound}
      draggable={false}
    />
  );
}
