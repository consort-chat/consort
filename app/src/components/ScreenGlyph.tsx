import "./ScreenGlyph.css";

/**
 * A monitor, under whatever picture there is.
 *
 * Always drawn, so a square is never empty: this session's own picture answers
 * nothing until its first frame, and nobody else's arrives at all yet.
 */
export function ScreenGlyph() {
  return (
    <svg
      className="call-screen-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="2.5" y="4" width="19" height="13" rx="2" />
      <path d="M9 20h6" />
      <path d="M12 17v3" />
    </svg>
  );
}
