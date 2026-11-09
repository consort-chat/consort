import { usePicture, type Sending } from "../lib/usePicture";
import "./CallPicture.css";

/** What each picture is called, for somebody who cannot see it. */
const ALT: Record<Sending, string> = {
  camera: "Your camera",
  screen: "Your screen",
};

interface Props {
  /** This session's camera, or the screen it is sharing. */
  of: Sending;
  /**
   * How many pixels to ask for on the long edge.
   *
   * The size of the box this is drawn into, which the call site knows and this
   * does not. Our own share goes on the stage as well as in a square, and
   * issue #194 was this being fixed where the frame was sampled.
   */
  bound: number;
}

/**
 * A still of what this session is sending, replaced on a timer.
 *
 * Its own component because the picture changes twelve times a second and
 * nothing else on the card does: a frame arriving has to redraw this and not
 * the faces beside it. #142 found that out by holding the picture one level up
 * and re-rendering every face in the call twelve and a half times a second.
 *
 * A still on a timer rather than a `<video>`: there is no stream for one to
 * play, because the device is open in Rust and V4L2 gives it to one process.
 * See `docs/adr/0007-draw-the-self-view-from-a-still.md`, `0008` for the second
 * picture, and `0014` for why the size travels with the ask. Everybody else's
 * is [`TheirPicture`], which is the same still from a different source.
 */
export function SelfPicture({ of, bound }: Props) {
  const picture = usePicture(of, bound);

  if (picture === null) return null;

  return (
    <img
      className="call-picture"
      data-of={of}
      data-mine="true"
      src={picture}
      /*
        Named rather than decorative. These are the two things on the card that
        report something about the person reading it, and "is this actually
        going out" is the question they answer.
      */
      alt={ALT[of]}
      /* Drawn at a known shape, so a frame arriving does not move anything. */
      width={bound}
      height={bound}
      draggable={false}
    />
  );
}
