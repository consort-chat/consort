import { useEffect, useRef } from "react";

import "./ConfirmDelete.css";

/**
 * The step between pressing Delete and a message being deleted.
 *
 * An edit can be edited again. A redaction cannot be taken back: the
 * homeserver empties the event and serves the emptied version from then on.
 * The control sits one button away from Edit in a row that appears on hover,
 * so the first press must not be the one that does it.
 *
 * A second panel rather than the same button asking twice, which is the cheap
 * version of this and the wrong one: a double click is a thing hands do on
 * their own, and a button that means "delete" on the second press is reached
 * by an accident somebody is already having. This puts the confirming press
 * somewhere the first press was not.
 *
 * Anchored to the message rather than drawn over the window, on the same terms
 * as [`ReactionPicker`]: it is about that message, and a panel in the middle
 * of the screen would have to say which one.
 */
export function ConfirmDelete({
  onConfirm,
  onCancel,
}: {
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const panel = useRef<HTMLDivElement | null>(null);
  const cancel = useRef<HTMLButtonElement | null>(null);

  /*
    Cancel takes the focus, not Delete. The same rule as the first press, one
    layer down: the key somebody hits without reading is Enter, and it must
    not land on the irreversible half of a question they have not read yet.
  */
  useEffect(() => {
    cancel.current?.focus();
  }, []);

  useEffect(() => {
    function onEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      // Stopped here, so one press shuts this rather than also shutting the
      // thread panel listening on the window behind it.
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
    <div
      className="confirm"
      role="dialog"
      aria-label="Delete this message?"
      ref={panel}
    >
      <p className="confirm__asks">Delete this message?</p>
      {/*
        What actually happens, because redacting is not erasing and a sentence
        promising otherwise would be a promise Consort cannot keep. The
        homeserver empties the event and serves the emptied version from then
        on; a server that already replicated the room keeps whatever it has,
        and no client can reach across federation to change that.
      */}
      <p className="confirm__says">
        The words are removed from the room for everyone. Servers and clients
        that already have a copy may keep it.
      </p>
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
          Delete
        </button>
      </div>
    </div>
  );
}
