import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { asCommandError, shareSources, type ShareSource } from "../lib/api";
import { keptOnScreen } from "../lib/floating";
import "./SharePicker.css";

/** Which tab is showing. The two the issue asked for and no more. */
type Tab = "window" | "screen";

const TABS: { kind: Tab; label: string }[] = [
  { kind: "window", label: "Applications" },
  { kind: "screen", label: "Screens" },
];

/**
 * A monitor, for the Screens tab.
 *
 * Drawn rather than photographed. A thumbnail would mean capturing every
 * source to populate the picker, which is the one thing a picker must not do:
 * opening it is not consent to read anything.
 */
function ScreenGlyph() {
  return (
    <svg
      className="share-picker__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="2" y="4" width="20" height="13" rx="1.5" />
      <path d="M8 21h8M12 17v4" />
    </svg>
  );
}

/** A window, for the Applications tab. */
function WindowGlyph() {
  return (
    <svg
      className="share-picker__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="3" y="4" width="18" height="16" rx="1.5" />
      <path d="M3 8.5h18" />
      <circle cx="6" cy="6.25" r="0.6" fill="currentColor" stroke="none" />
    </svg>
  );
}

/**
 * Somewhere to choose what to share.
 *
 * Two tabs, Applications and Screens, which is what #70 asked for and what
 * X11 can actually answer: the list comes from the window manager's own
 * properties. On Wayland there is no list to read, the command refuses, and
 * this draws the reason rather than an empty card.
 *
 * The ordering is not decided here. `consort_video::screens` puts the most
 * recently raised window first and any fullscreen window ahead of that, and
 * re-sorting in the interface would be a second answer to drift from the
 * first.
 *
 * Nothing is captured by opening this. The list is property reads, the
 * thumbnails are drawings, and the first frame of anybody's screen is read
 * when a row is clicked and not before.
 *
 * It floats in the window rather than off the control that opens it, on the
 * same terms as [`PersonMenu`]: the sidebar clips what leaves it, so a card
 * anchored inside that column is cut off at its edge however it is stacked.
 */
export function SharePicker({
  at,
  onPick,
  onClose,
}: {
  /**
   * The bottom left corner to grow from, in viewport coordinates.
   *
   * Upwards, because the control is the last row of the sidebar and there is
   * nothing below it to open into.
   */
  at: { x: number; y: number };
  /** Start sharing this source id. */
  onPick: (id: string) => void;
  /** Put the picker away without sharing anything. */
  onClose: () => void;
}) {
  const [tab, setTab] = useState<Tab>("window");
  const [sources, setSources] = useState<ShareSource[] | null>(null);
  const [trouble, setTrouble] = useState<string | null>(null);
  const card = useRef<HTMLDivElement | null>(null);
  const first = useRef<HTMLButtonElement | null>(null);
  const [placement, setPlacement] = useState({ left: at.x, top: at.y });

  /*
    Read once, when the picker opens. Not polled: a list that reordered itself
    under the pointer would move the row somebody was reaching for, and for
    this picker that means sharing the wrong window.
  */
  useEffect(() => {
    let cancelled = false;
    shareSources()
      .then((listed) => {
        if (!cancelled) setSources(listed);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        // The list is left null rather than emptied, so a refusal cannot leave
        // a stale row behind to click.
        setTrouble(
          asCommandError(error)?.message ??
            "Consort could not read what is on this screen.",
        );
      });
    return () => {
      cancelled = true;
    };
  }, []);

  /* Focus lands on the tabs, so Escape and Tab both reach the card. */
  useEffect(() => {
    first.current?.focus();
  }, []);

  // Measured rather than assumed. The card's height follows what is in it, so
  // it grows when the list lands and when a tab is switched, and a constant
  // would be a guess that goes stale. Before paint, so it is never drawn low.
  useLayoutEffect(() => {
    const node = card.current;
    if (node === null) return;
    const box = node.getBoundingClientRect();
    setPlacement(keptOnScreen({ left: at.x, top: at.y - box.height }, box));
  }, [at.x, at.y, sources, tab, trouble]);

  useEffect(() => {
    function onEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      onClose();
    }

    /*
      `mousedown` rather than `click`, on the same terms as every other
      dismissable thing here: a drag that starts inside and ends outside is not
      a request to close.
    */
    function elsewhere(event: MouseEvent) {
      if (event.target instanceof Node && card.current?.contains(event.target)) {
        return;
      }
      onClose();
    }

    document.addEventListener("keydown", onEscape);
    document.addEventListener("mousedown", elsewhere);
    return () => {
      document.removeEventListener("keydown", onEscape);
      document.removeEventListener("mousedown", elsewhere);
    };
  }, [onClose]);

  const showing = (sources ?? []).filter((source) => source.kind === tab);

  return (
    <div
      className="share-picker"
      ref={card}
      role="dialog"
      aria-label="Choose what to share"
      style={{ left: placement.left, top: placement.top }}
    >
      <div className="share-picker__tabs" role="tablist" aria-label="What to share">
        {TABS.map(({ kind, label }, nth) => (
          <button
            key={kind}
            type="button"
            className="share-picker__tab"
            role="tab"
            aria-selected={tab === kind}
            ref={nth === 0 ? first : undefined}
            onClick={() => setTab(kind)}
          >
            {label}
          </button>
        ))}
      </div>

      <div className="share-picker__body" role="tabpanel">
        {trouble !== null && (
          <p className="share-picker__trouble" role="alert">
            {trouble}
          </p>
        )}
        {trouble === null && sources !== null && showing.length === 0 && (
          <p className="share-picker__empty" role="status">
            There is nothing here to share.
          </p>
        )}
        {showing.length > 0 && (
          <ul className="share-picker__list">
            {showing.map((source) => (
              <li key={source.id}>
                <button
                  type="button"
                  className="share-picker__source"
                  data-fullscreen={source.fullscreen}
                  onClick={() => onPick(source.id)}
                >
                  <span className="share-picker__preview" aria-hidden="true">
                    {source.kind === "screen" ? <ScreenGlyph /> : <WindowGlyph />}
                  </span>
                  <span className="share-picker__name">{source.title}</span>
                  {/*
                    The size, so two windows named the same thing can be told
                    apart, and so a 2560x1440 monitor is visibly the big one.
                  */}
                  <span className="share-picker__size">
                    {source.width}x{source.height}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/*
        A way out that is not Escape and not a click outside. Both of those are
        conventions somebody has to already know, and this card is the last
        moment before a screen starts going out.
      */}
      <div className="share-picker__footer">
        <button
          type="button"
          className="share-picker__cancel"
          onClick={onClose}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
