import "./SidebarToggle.css";

/**
 * The control that folds the channel list away, and the one that brings it
 * back.
 *
 * One component for both, because they are one control that happens to be
 * drawn in two places: folding hides the header it lives in, so something
 * still on screen has to be able to undo it. The chevron points the way the
 * list will move.
 *
 * The unfolding half is drawn twice, in the room header and in the empty pane,
 * because the empty pane has no header. Without the second, folding the list
 * before picking a channel would be a one-way door.
 */
export function SidebarToggle({
  folded,
  onToggle,
}: {
  /** Whether the list is hidden right now, which decides which way it points. */
  folded: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      className="sidebar-toggle"
      aria-label={folded ? "Show the channel list" : "Hide the channel list"}
      onClick={onToggle}
    >
      <svg
        className="sidebar-toggle__glyph"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d={folded ? "M9 6l6 6-6 6" : "M15 6l-6 6 6 6"} />
      </svg>
    </button>
  );
}
