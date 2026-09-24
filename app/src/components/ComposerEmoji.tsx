import { useState, type RefObject } from "react";

import { withInserted } from "../lib/draft";
import { EmojiPicker } from "./EmojiPicker";
import "./ComposerEmoji.css";

/**
 * The control in a composer row that puts an emoji in the draft.
 *
 * The second of the two the picker exists for, and the whole of what makes it
 * the second one: this hands the key to the box rather than sending it. The
 * grid, the search, the remembered row and the data behind them are shared
 * with the picker that hangs off a message.
 *
 * One component rather than the same twelve lines in both composers. The room
 * and the thread panel each own their own draft, which is the seam, and
 * everything on this side of it is the same question in both.
 */
export function ComposerEmoji({
  box,
  draft,
  disabled,
  onChanged,
}: {
  /** The box to type into, so the key lands where the caret is. */
  box: RefObject<HTMLTextAreaElement | null>;
  /** What is in it, which is the state the caller owns. */
  draft: string;
  disabled?: boolean;
  /**
   * The draft, with the key in it.
   *
   * A callback rather than a setter, because the room's composer has a typing
   * notice to send as well and the thread panel has not.
   */
  onChanged: (text: string) => void;
}) {
  const [open, setOpen] = useState(false);

  /**
   * Type one key in, where the caret is.
   *
   * Focus goes back to the box afterwards. Somebody picking an emoji is
   * mid-sentence, and leaving it in a panel that has just closed would send
   * the next thing they type nowhere.
   */
  function insert(key: string) {
    const at = box.current;
    const { text, caret } = withInserted(
      draft,
      at?.selectionStart ?? draft.length,
      at?.selectionEnd ?? draft.length,
      key,
    );
    onChanged(text);
    setOpen(false);
    // After the render that puts the new text in. Setting the range against
    // the old value leaves the browser to clamp it somewhere else.
    queueMicrotask(() => {
      box.current?.focus();
      box.current?.setSelectionRange(caret, caret);
    });
  }

  return (
    <span className="composer-emoji">
      <button
        type="button"
        className="composer-emoji__open"
        aria-label="Add an emoji"
        aria-expanded={open}
        disabled={disabled === true}
        onClick={() => setOpen((was) => !was)}
      >
        <SmileyIcon />
      </button>
      {open && (
        <EmojiPicker
          action="Insert"
          align="left"
          opens="up"
          onPick={insert}
          onClose={() => setOpen(false)}
        />
      )}
    </span>
  );
}

/**
 * A plain face.
 *
 * Deliberately without the plus that `ReactIcon` carries. That control adds a
 * reaction to something somebody said and has to be told apart from the
 * reactions already drawn beside it; this one puts a character in a box, and
 * the plus would be saying something that is not true of it.
 */
function SmileyIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M9 9h.01" />
      <path d="M15 9h.01" />
      <path d="M8.5 14.5a4 4 0 0 0 7 0" />
    </svg>
  );
}
