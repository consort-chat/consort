import { useEffect, useRef, useState } from "react";

import {
  asCommandError,
  emojiSettings,
  emojiUsed,
  setEmojiTone,
  type EmojiSettings,
} from "../lib/api";
import { loadEmoji, type EmojiSet } from "../lib/emoji";
import { EmojiGrid } from "./EmojiGrid";
import "./EmojiPicker.css";

/** Nothing remembered yet, and nothing that failed to be read. */
const NOTHING_YET: EmojiSettings = { recent: [], tone: 0 };

/**
 * Somewhere to pick an emoji from.
 *
 * Two controls open one of these and they differ in one thing: what `onPick`
 * does with the key. The one hanging off a message sends it as a reaction, the
 * one in the composer types it into the draft. Everything about finding an
 * emoji is the same question in both places, so the grid, the search and the
 * data are [`EmojiGrid`] and this is the panel around them.
 *
 * Anchored to whatever opened it rather than drawn over the window, because it
 * is about that message or that box and a panel in the middle of the screen
 * would have to say which. That also means it scrolls away with what it
 * belongs to, which is the right answer: a picker still open over a message
 * that has been scrolled past is pointing at nothing.
 */
export function EmojiPicker({
  action,
  chosen,
  onPick,
  onClose,
  align = "right",
}: {
  /** How a key's label reads, as a verb: "React with" or "Insert". */
  action: string;
  /** The keys this session has already used, drawn as pressed. Reactions only. */
  chosen?: ReadonlySet<string> | undefined;
  /** Take this key. Sent, or typed, depending on which control opened this. */
  onPick: (key: string) => void;
  onClose: () => void;
  /**
   * Which edge of the control it hangs from.
   *
   * The toolbar sits over the message's top right corner, so its panel is
   * pinned right and grows back across the message. The control beside the
   * pills and the one in the composer are at the other end of their rows,
   * where the same pinning would put the panel somewhere the press was not.
   */
  align?: "left" | "right";
}) {
  const [set, setSet] = useState<EmojiSet | null>(null);
  const [choices, setChoices] = useState<EmojiSettings>(NOTHING_YET);
  const panel = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    loadEmoji().then(setSet, (raw: unknown) => {
      console.error("could not read the emoji set", asCommandError(raw).detail);
    });

    emojiSettings().then(setChoices, (raw: unknown) => {
      // Drawn anyway, with an empty row. Failing to remember what somebody
      // reacted with last week is not a reason to stop them reacting now.
      console.error(
        "could not read what the picker remembers",
        asCommandError(raw).detail,
      );
    });
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

  /** Hand the key over, and write it down. */
  function pick(key: string) {
    onPick(key);
    emojiUsed(key).then(setChoices, (raw: unknown) => {
      // The reaction has already gone by now, or the draft already has the
      // character in it. Losing the row entry is the smaller problem.
      console.error(
        "could not remember the emoji that was used",
        asCommandError(raw).detail,
      );
    });
  }

  /** Choose a tone, and write that down too. */
  function tone(chosen: number) {
    setChoices((was) => ({ ...was, tone: chosen }));
    setEmojiTone(chosen).catch((raw: unknown) => {
      console.error(
        "could not remember the skin tone",
        asCommandError(raw).detail,
      );
    });
  }

  return (
    <div
      className={align === "left" ? "picker picker--left" : "picker"}
      role="group"
      // Named after what it is for rather than "Emoji picker", because the two
      // that exist do different things and this is the sentence that says
      // which one somebody has opened.
      aria-label={`${action} an emoji`}
      ref={panel}
    >
      {set === null ? (
        /*
          A line rather than a spinner, and a live region rather than a label,
          because this is the only wait in the picker and it is the one a
          screen reader would otherwise spend in silence.
        */
        <p className="picker__waiting" role="status">
          Fetching the emoji
        </p>
      ) : (
        <EmojiGrid
          set={set}
          action={action}
          chosen={chosen}
          recent={choices.recent}
          tone={choices.tone}
          onTone={tone}
          onPick={pick}
        />
      )}
    </div>
  );
}
