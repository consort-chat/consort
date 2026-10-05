import type { ReactNode } from "react";

import { ScreenGlyph } from "./ScreenGlyph";
import "./ScreenTile.css";

interface Props {
  /**
   * What to call it: what is being shared for this session's own screen, and
   * whose it is for anybody else's.
   */
  label: string;
  /** The screen itself, over the glyph. See [`ScreenStage`]'s own. */
  picture?: ReactNode;
  /** Put this one on the stage, and whatever was there back in the strip. */
  onPick: () => void;
}

/**
 * One shared screen waiting its turn, as a square under the stage.
 *
 * The same square a face sits in, because a shared screen and a person are the
 * same kind of thing down here: something the call is carrying that is not
 * what the call is currently about.
 */
export function ScreenTile({ label, picture, onPick }: Props) {
  return (
    <li className="call-screen">
      {/*
        What it does rather than only whose it is. The label is drawn under the
        square as well, so speech input has a name to press, but a control that
        rearranges the card should say so to somebody who cannot see it move.
      */}
      <button
        type="button"
        className="call-screen__button"
        aria-label={`Put ${label} on the stage`}
        title="Put it on the stage"
        onClick={onPick}
      >
        <span className="call-screen__stage">
          <ScreenGlyph />
          {picture}
        </span>
        <span className="call-screen__what">{label}</span>
      </button>
    </li>
  );
}
