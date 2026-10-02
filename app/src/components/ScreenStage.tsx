import { ScreenGlyph } from "./ScreenGlyph";
import { SelfPicture } from "./SelfPicture";
import "./ScreenStage.css";

interface Props {
  /**
   * What to call it: what is being shared for this session's own screen, and
   * whose it is for anybody else's.
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
 * The screen the call is watching, across the width of the card.
 *
 * One stage rather than a row of equals: a desktop at seventy pixels across
 * says which window layout is going out and nothing about what it says, and a
 * call with a screen in it is usually a call about that screen.
 */
export function ScreenStage({ label, mine, full, onToggle }: Props) {
  return (
    /*
      One name whatever size the card is, with the state on `aria-pressed`, the
      way the card's own controls do it: a button whose name changes under the
      cursor is announced as a new button.
    */
    <button
      type="button"
      className="call-stage"
      aria-label={`${label}, fill the window`}
      aria-pressed={full}
      title={full ? "Back to the card" : "Fill the window"}
      onClick={onToggle}
    >
      <span className="call-stage__picture">
        <ScreenGlyph />
        {mine && <SelfPicture of="screen" />}
      </span>
      <span className="call-stage__what">{label}</span>
    </button>
  );
}
