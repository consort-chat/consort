import { usePicture, type Sending } from "../lib/usePicture";
import "./SelfPicture.css";

/** What each picture is called, for somebody who cannot see it. */
const ALT: Record<Sending, string> = {
  camera: "Your camera",
  screen: "Your screen",
};

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
 * See `docs/adr/0007-draw-the-self-view-from-a-still.md`, and `0008` for the
 * second picture.
 */
export function SelfPicture({ of }: { of: Sending }) {
  const picture = usePicture(of);

  if (picture === null) return null;

  return (
    <img
      className="self-picture"
      data-of={of}
      src={picture}
      /*
        Named rather than decorative. These are the two things on the card that
        report something about the person reading it, and "is this actually
        going out" is the question they answer.
      */
      alt={ALT[of]}
      /* Drawn at a known shape, so a frame arriving does not move anything. */
      width={320}
      height={320}
      draggable={false}
    />
  );
}
