/**
 * The control that makes a section of somebody's own, and the box it opens.
 *
 * At the foot of the list rather than in the header, which already holds the
 * space's name and the control that folds the column away.
 */
import { useState } from "react";

import { MAX_SECTION_NAME } from "../lib/sections";

export function NewSection({
  onCreate,
}: {
  /** Make a section with this name. Trimmed and bounded in Rust. */
  onCreate: (name: string) => void;
}) {
  // The name being typed, or null when only the button is drawn. Closed
  // again on submit, so making two sections is two deliberate presses.
  const [draft, setDraft] = useState<string | null>(null);

  if (draft === null) {
    return (
      <button
        type="button"
        className="channels__new"
        onClick={() => setDraft("")}
      >
        New section
      </button>
    );
  }

  return (
    <form
      className="channels__new-form"
      onSubmit={(event) => {
        event.preventDefault();
        onCreate(draft);
        setDraft(null);
      }}
    >
      <input
        className="channels__name-box"
        aria-label="Name for the new section"
        placeholder="Projects"
        value={draft}
        maxLength={MAX_SECTION_NAME}
        autoFocus
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") setDraft(null);
        }}
      />
      <button
        type="submit"
        className="channels__save"
        disabled={draft.trim() === ""}
      >
        Add
      </button>
      <button
        type="button"
        className="channels__cancel"
        onClick={() => setDraft(null)}
      >
        Cancel
      </button>
    </form>
  );
}
