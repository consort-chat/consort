import type { ReactNode, Ref } from "react";

import { ScreenGlyph } from "./ScreenGlyph";
import "./ScreenStage.css";

interface Props {
  /**
   * What to call it: what is being shared for this session's own screen, and
   * whose it is for anybody else's.
   */
  label: string;
  /**
   * The screen itself, drawn over the glyph that stands in for it.
   *
   * A node rather than a URL, so whatever polls for frames is a component of
   * its own and a frame redraws that alone: #142's trap. Which screen this is
   * and where its frames come from are the card's business, not the stage's.
   */
  picture?: ReactNode;
  /**
   * The box the picture fills, for whatever is measuring it.
   *
   * Handed in rather than measured here: the stage owns its own layout, and
   * which pixels the picture is asked for is the card's business.
   */
  boxRef?: Ref<HTMLSpanElement>;
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
export function ScreenStage({ label, picture, boxRef, full, onToggle }: Props) {
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
      <span className="call-stage__picture" ref={boxRef}>
        <ScreenGlyph />
        {picture}
      </span>
      <span className="call-stage__what">{label}</span>
    </button>
  );
}
