import { useEffect, useRef } from "react";

import "./ReactionPicker.css";

/**
 * The keys offered without typing anything.
 *
 * A short set on purpose. A complete picker means either a megabyte of emoji
 * data in the bundle or a search box over a list nobody scrolls, and neither
 * is what a reaction is for: almost every one anybody sends is agreement,
 * disagreement, or a laugh. Custom and animated emoji are their own piece of
 * work and will bring a real picker with them.
 *
 * A key that is not in here still draws correctly when somebody else sends it,
 * because nothing downstream restricts what a reaction may be.
 */
const QUICK = [
  "👍",
  "👎",
  "😄",
  "🎉",
  "😕",
  "❤️",
  "🚀",
  "👀",
  "✅",
  "🙏",
  "🔥",
  "😢",
] as const;

/**
 * Somewhere to pick a reaction from.
 *
 * Anchored to the message rather than drawn over the window, because it is
 * about that message and a panel in the middle of the screen would have to say
 * which one. That also means it scrolls away with what it belongs to, which is
 * the right answer: a picker still open over a message that has been scrolled
 * past is pointing at nothing.
 */
export function ReactionPicker({
  chosen,
  onChoose,
  onClose,
  align = "right",
}: {
  /** The keys this session has already used, drawn as pressed. */
  chosen: ReadonlySet<string>;
  /** Use `key`, or take it back when it is already one of `chosen`. */
  onChoose: (key: string) => void;
  onClose: () => void;
  /**
   * Which edge of the control it hangs from.
   *
   * The toolbar sits over the message's top right corner, so its panel is
   * pinned right and grows back across the message. The control beside the
   * pills is at the other end of the row, where the same pinning would put
   * the panel somewhere the press was not.
   */
  align?: "left" | "right";
}) {
  const panel = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    panel.current?.querySelector("button")?.focus();
  }, []);

  useEffect(() => {
    function onEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      // Stopped here, so that Escape closes this and not the thread panel
      // behind it. One press, one thing.
      event.stopPropagation();
      onClose();
    }

    /*
      `mousedown` rather than `click`, on the same terms as every other
      dismissable thing here: a drag that starts inside and ends outside is not
      somebody asking for this to close.
    */
    function away(event: MouseEvent) {
      if (event.target instanceof Node && panel.current?.contains(event.target)) {
        return;
      }
      onClose();
    }

    document.addEventListener("keydown", onEscape);
    document.addEventListener("mousedown", away);
    return () => {
      document.removeEventListener("keydown", onEscape);
      document.removeEventListener("mousedown", away);
    };
  }, [onClose]);

  return (
    <div
      className={align === "left" ? "picker picker--left" : "picker"}
      role="group"
      aria-label="React with"
      ref={panel}
    >
      {QUICK.map((key) => (
        <button
          key={key}
          type="button"
          className="picker__key"
          aria-label={`React with ${key}`}
          aria-pressed={chosen.has(key)}
          onClick={() => onChoose(key)}
        >
          {key}
        </button>
      ))}
    </div>
  );
}
