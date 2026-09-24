/**
 * What is in the composer, and the one rule for changing it from outside.
 */

/**
 * `draft` with `key` typed in over the selection between `start` and `end`.
 *
 * Both composers use this, which is why it is here rather than in either of
 * them. Pressing a key in the picker is typing a character: it goes in where
 * the caret is, it replaces whatever was selected, and the caret ends up after
 * it, the same as if somebody had pressed a letter.
 *
 * Offsets are the textarea's own, which is to say UTF-16 units rather than
 * characters. The returned caret is counted the same way, because putting it
 * back anywhere else would leave it inside the emoji just inserted and let the
 * next keystroke cut it in half.
 */
export function withInserted(
  draft: string,
  start: number,
  end: number,
  key: string,
): { text: string; caret: number } {
  return {
    text: draft.slice(0, start) + key + draft.slice(end),
    caret: start + key.length,
  };
}
