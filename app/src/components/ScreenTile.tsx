import { SelfPicture } from "./SelfPicture";
import "./ScreenTile.css";

/**
 * A monitor, under whatever picture there is.
 *
 * Always drawn, so the square is never empty: this session's own picture
 * answers nothing until its first frame, and nobody else's arrives at all yet.
 */
function ScreenGlyph() {
  return (
    <svg
      className="call-screen__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="2.5" y="4" width="19" height="13" rx="2" />
      <path d="M9 20h6" />
      <path d="M12 17v3" />
    </svg>
  );
}

interface Props {
  /**
   * What to call it: what is being shared for this session's own screen, and
   * whose it is for anybody else's.
   *
   * The whole accessible name for somebody else's, which is why it reads as a
   * phrase rather than a bare name.
   */
  label: string;
  /**
   * Whether this is the screen this session is sending.
   *
   * Only this one has a picture. Frames come from one local capture, and
   * nothing carries anybody else's into this window yet: see
   * `docs/PLAN-screen-share.md`.
   */
  mine: boolean;
  /** Whether the card is already filling the window. */
  full: boolean;
  /** Fill the window with the card, or put it back. */
  onToggle: () => void;
}

/**
 * One screen somebody is sharing, as a square on the call card.
 *
 * Its own square rather than a strip across the card, so two people presenting
 * at once are two tiles and not an argument about which one gets the strip.
 * The same square a face sits in, because a shared screen and a person are the
 * same kind of thing here: something the call is carrying.
 */
export function ScreenTile({ label, mine, full, onToggle }: Props) {
  return (
    <li className="call-screen">
      {/*
        A button, so the keyboard reaches it with focus, Enter and Space for
        free. What it does is grow the card, which is the one thing on the card
        that a picture of somebody's screen at seventy pixels across cannot do
        without.
      */}
      <button
        type="button"
        className="call-screen__button"
        title={full ? "Back to the card" : "Fill the window"}
        onClick={onToggle}
      >
        <span className="call-screen__stage">
          <ScreenGlyph />
          {mine && <SelfPicture of="screen" />}
        </span>
        <span className="call-screen__what">{label}</span>
      </button>
    </li>
  );
}
