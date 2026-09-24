import { useEffect, useRef } from "react";

import "./Confirm.css";

/**
 * The step between pressing something irreversible and it happening.
 *
 * Deleting a message was the first of these and leaving a room is the second,
 * and they are one component rather than two because the shape of the question
 * is the whole point rather than the words in it. A second panel rather than
 * the same button asking twice, which is the cheap version of this and the
 * wrong one: a double click is a thing hands do on their own, and a button
 * that means "delete" on the second press is reached by an accident somebody
 * is already having. This puts the confirming press somewhere the first press
 * was not.
 *
 * Anchored to the control that asked rather than drawn over the window, on the
 * same terms as [`ReactionPicker`]: it is about that one thing, and a panel in
 * the middle of the screen would have to say which. Whichever way round it
 * opens is the anchor's business, through `--confirm-top` and
 * `--confirm-bottom`.
 */
export function Confirm({
  question,
  detail,
  go,
  onConfirm,
  onCancel,
}: {
  /** The question, which is also what the dialog is called. */
  question: string;
  /** What actually happens, in a sentence somebody has to read first. */
  detail: string;
  /** What to write on the half that does it. */
  go: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const panel = useRef<HTMLDivElement | null>(null);
  const cancel = useRef<HTMLButtonElement | null>(null);

  /*
    Cancel takes the focus, not the other one. The same rule as the first
    press, one layer down: the key somebody hits without reading is Enter, and
    it must not land on the irreversible half of a question they have not read
    yet.
  */
  useEffect(() => {
    cancel.current?.focus();
  }, []);

  useEffect(() => {
    function onEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      // Stopped here, so one press shuts this rather than also shutting the
      // thread panel or the room's details listening on the window behind it.
      event.stopPropagation();
      onCancel();
    }

    /*
      `mousedown` rather than `click`, on the same terms as everything else
      here that can be dismissed: a drag that starts inside and ends outside is
      not somebody asking for this to close.
    */
    function away(event: MouseEvent) {
      if (event.target instanceof Node && panel.current?.contains(event.target)) {
        return;
      }
      onCancel();
    }

    document.addEventListener("keydown", onEscape);
    document.addEventListener("mousedown", away);
    return () => {
      document.removeEventListener("keydown", onEscape);
      document.removeEventListener("mousedown", away);
    };
  }, [onCancel]);

  return (
    <div className="confirm" role="dialog" aria-label={question} ref={panel}>
      <p className="confirm__asks">{question}</p>
      {/*
        What actually happens, and never hidden behind a disclosure. Somebody
        deciding needs it in front of them rather than a click away, and the
        thing it is for is the promise a one-line question would imply and the
        application could not keep.
      */}
      <p className="confirm__says">{detail}</p>
      <div className="confirm__choices">
        <button
          type="button"
          className="confirm__stop"
          ref={cancel}
          onClick={onCancel}
        >
          Cancel
        </button>
        <button type="button" className="confirm__go" onClick={onConfirm}>
          {go}
        </button>
      </div>
    </div>
  );
}
