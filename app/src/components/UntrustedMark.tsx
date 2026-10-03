import { senderTrustLabel } from "../lib/labels";
import type { SenderTrust } from "../lib/api";

/**
 * The mark beside a message whose sender could not be vouched for.
 *
 * Focusable, which `PresenceDot` is not: `title` alone never opens for a
 * keyboard, and this glyph carries something somebody has to be able to read.
 */
export function UntrustedMark({ trust }: { trust: SenderTrust }) {
  const sentence = senderTrustLabel(trust);

  return (
    <span className="timeline__untrusted">
      {/* The sentence is the name, so nothing here is a colour on its own. */}
      <span
        className="timeline__untrusted-glyph"
        data-trust={trust}
        role="img"
        aria-label={sentence}
        tabIndex={0}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true" focusable="false">
          <path
            d="M8 1.5 3 3.2v4.3c0 3 2.1 5.7 5 6.5 2.9-.8 5-3.5 5-6.5V3.2L8 1.5Z"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinejoin="round"
          />
          <path
            d="M8 5.3v3.2"
            stroke="currentColor"
            strokeWidth="1.4"
            strokeLinecap="round"
          />
          <circle cx="8" cy="10.7" r="0.8" fill="currentColor" />
        </svg>
      </span>
      {/* Hidden: the glyph above already carries these words as its name. */}
      <span className="timeline__untrusted-tip" aria-hidden="true">
        {sentence}
      </span>
    </span>
  );
}
