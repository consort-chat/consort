import "./ComposerAttach.css";

/**
 * The control in a composer row that opens the desktop's file picker.
 *
 * One component rather than the same button twice, which is the seam
 * `ComposerEmoji` draws: the room and the thread panel each own what becomes of
 * the file, and the control itself asks the same question in both.
 */
export function ComposerAttach({
  disabled,
  onClick,
}: {
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="composer-attach"
      aria-label="Attach a file"
      disabled={disabled === true}
      onClick={onClick}
    >
      <PaperclipIcon />
    </button>
  );
}

/**
 * A paperclip, for the control above and for the line that says what is
 * waiting to be sent.
 */
export function PaperclipIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      /*
        Shifted, not square with the origin. The path's ink spans x 4.33..21.00
        and y 4.03..22.07, so it sits centred on (12.66, 13.05) and drew half a
        pixel right and most of one low. Moving the window rather than the
        coordinates leaves the path as it came, and a scale is not an option
        here: `stroke-width` scales with it, which would make this the one thin
        icon in the application.
      */
      viewBox="0.66 1.05 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 12.5 12.9 20.6a5 5 0 0 1-7.1-7.1l8.5-8.5a3.3 3.3 0 0 1 4.7 4.7l-8.5 8.5a1.7 1.7 0 0 1-2.4-2.4l7.8-7.8" />
    </svg>
  );
}
