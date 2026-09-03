import { useEffect, useRef, useState, type ReactNode } from "react";

import { asCommandError, mediaUrl, saveAttachment, type Media } from "../lib/api";
import { sizeLabel } from "../lib/labels";
import "./ImageViewer.css";

/**
 * The shared shell of the three corner controls' icons.
 *
 * One wrapper rather than three copies of the same nine attributes. What
 * differs between them is the path, so the path is the only thing written out
 * at each use.
 */
function Glyph({ children }: { children: ReactNode }) {
  return (
    <svg
      className="viewer__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

/**
 * One picture, as large as the window will allow.
 *
 * A picture in a room is capped at a size that keeps a conversation readable,
 * which for a screenshot of a spreadsheet means it cannot be read at all. This
 * is the way out of that, and it is the one every client has: press the
 * picture, get the picture.
 *
 * No zooming and no panning. The window is the size, and a picture larger than
 * it is scaled to fit; anything more is a viewer rather than a chat client, and
 * saving it opens whatever this machine has for looking at pictures properly.
 *
 * ## The dialog conventions here are `SettingsModal`'s
 *
 * Escape at the document rather than on the element, because most of what is
 * inside cannot take focus and a handler on the element stops working after
 * the first click on the picture. Focus returns to whatever opened it. The
 * backdrop closes on `mousedown` with target compared to currentTarget, so a
 * drag that starts on the picture and ends outside is not a close.
 */
export function ImageViewer({
  media,
  onClose,
}: {
  media: Media;
  onClose: () => void;
}) {
  const [telling, setTelling] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const dialog = useRef<HTMLDivElement | null>(null);
  /*
    Captured during the first render rather than in the effect, for the reason
    `SettingsModal` does it: by the time an effect runs the commit that mounted
    this has already moved focus, so there is nothing left to give it back to.
  */
  const [opener] = useState<Element | null>(() => document.activeElement);

  useEffect(() => {
    dialog.current?.querySelector("button")?.focus();

    return () => {
      if (opener instanceof HTMLElement && document.contains(opener)) {
        opener.focus();
      }
    };
  }, [opener]);

  useEffect(() => {
    function onEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      onClose();
    }

    document.addEventListener("keydown", onEscape);
    return () => document.removeEventListener("keydown", onEscape);
  }, [onClose]);

  function save() {
    setProblem(null);
    saveAttachment(media.source, media.name)
      .then((path) => {
        if (path !== null) setSaved(path);
      })
      .catch((raw: unknown) => {
        setProblem(asCommandError(raw).message);
      });
  }

  const weight = sizeLabel(media.size);
  const shape =
    media.width === undefined || media.height === undefined
      ? null
      : `${media.width} by ${media.height}`;

  return (
    <div
      className="viewer"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        className="viewer__pane"
        role="dialog"
        aria-modal="true"
        aria-label={media.name}
        ref={dialog}
        /*
          `mousedown` here as well as on the backdrop. The pane fills the
          window and the picture is centred inside it, so the space either side
          of a tall picture is this element rather than the backdrop, and
          somebody pressing what looks like the backdrop should get what the
          backdrop does.
        */
        onMouseDown={(event) => {
          if (event.target === event.currentTarget) onClose();
        }}
      >
        <button
          type="button"
          className="viewer__button viewer__close"
          aria-label="Close"
          onClick={onClose}
        >
          <Glyph>
            <path d="M18 6 6 18" />
            <path d="m6 6 12 12" />
          </Glyph>
        </button>

        <div className="viewer__tools">
          <button
            type="button"
            className="viewer__button"
            aria-label="About this picture"
            aria-pressed={telling}
            onClick={() => setTelling((showing) => !showing)}
          >
            <Glyph>
              <circle cx="12" cy="12" r="9" />
              <path d="M12 16v-4" />
              <path d="M12 8h.01" />
            </Glyph>
          </button>
          <button
            type="button"
            className="viewer__button"
            aria-label={`Save ${media.name}`}
            onClick={save}
          >
            <Glyph>
              <path d="M12 3v13" />
              <path d="m7 11 5 5 5-5" />
              <path d="M4 20h16" />
            </Glyph>
          </button>
        </div>

        <img
          className="viewer__image"
          src={mediaUrl(media.source)}
          alt={media.name}
        />

        {telling && (
          <dl className="viewer__facts">
            <div className="viewer__fact">
              <dt>Name</dt>
              <dd>{media.name}</dd>
            </div>
            {shape !== null && (
              <div className="viewer__fact">
                <dt>Size</dt>
                <dd>{shape} pixels</dd>
              </div>
            )}
            {weight !== null && (
              <div className="viewer__fact">
                <dt>Weight</dt>
                <dd>{weight}</dd>
              </div>
            )}
            {media.mime !== undefined && (
              <div className="viewer__fact">
                <dt>Type</dt>
                <dd>{media.mime}</dd>
              </div>
            )}
          </dl>
        )}

        {saved !== null && <p className="viewer__note">Saved to {saved}</p>}
        {problem !== null && (
          <p className="viewer__note viewer__note--warn" role="alert">
            {problem}
          </p>
        )}
      </div>
    </div>
  );
}
