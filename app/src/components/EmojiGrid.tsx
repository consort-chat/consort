import { useEffect, useMemo, useRef, useState } from "react";

import { inTone, matching, type Emoji, type EmojiSet } from "../lib/emoji";
import "./EmojiGrid.css";

/**
 * How many keys are in a row.
 *
 * Shared with `EmojiGrid.css`, which draws that many columns off
 * `--emoji-across`, and held together by a test: the arrow keys move by this
 * number and a grid that wrapped at a different one would send the cursor
 * somewhere the eye is not. jsdom does no layout, so counting the columns at
 * runtime is not on offer, and a number both halves agree on is better than a
 * measurement one half cannot take.
 */
export const ACROSS = 9;

/**
 * The search, the categories and the keys.
 *
 * Drawn once and used twice. The picker hanging off a message sends what is
 * pressed and the one in the composer types it into the draft, and that is the
 * entire difference between them: everything about finding an emoji is the
 * same question in both places, and two copies would be two answers free to
 * drift apart.
 *
 * ## Why one category at a time
 *
 * Nineteen hundred buttons mounted at once is a pause somebody can see on
 * every open. The categories are a row of controls that swap what the grid
 * holds rather than scroll to it, so at most a few hundred are ever mounted.
 *
 * ## Why the search box owns the keyboard
 *
 * It is the only part of this usable without a mouse, so it is where focus
 * starts and where the arrow keys lead back to. The keys carry a roving
 * tabindex, which keeps the whole grid to one stop: Tab out of the box lands
 * on wherever the arrows last were, and Tab again leaves the picker rather
 * than walking through four hundred emoji one at a time.
 */
export function EmojiGrid({
  set,
  action,
  chosen,
  recent,
  tone,
  onTone,
  onPick,
}: {
  set: EmojiSet;
  /**
   * How a key's label reads, as a verb: "React with" or "Insert".
   *
   * The label says what pressing it does rather than only naming the emoji,
   * because the two pickers do different things with the same grid and a
   * screen reader is the one place that difference is otherwise invisible.
   */
  action: string;
  /** The keys this session has already used, drawn as pressed. Reactions only. */
  chosen?: ReadonlySet<string> | undefined;
  /**
   * What to put in the row above the categories, most recent first.
   *
   * Free strings rather than emoji out of the set. One of them may be a key
   * this build has never heard of, which still has to draw: a picker that
   * could only offer what it knows would be a regression the first time
   * somebody in the room used a client with a wider set.
   */
  recent: readonly string[];
  /** Which skin tone to apply, 1 to 5, or 0 for none. */
  tone: number;
  onTone: (tone: number) => void;
  /** Take this key, toned if it has one. */
  onPick: (key: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState(0);
  /*
    Which key the arrows are on, or none while focus is still in the box. An
    index into whatever the grid is currently drawing, so switching category or
    typing puts it back to the start on its own.
  */
  const [at, setAt] = useState<number | null>(null);
  const box = useRef<HTMLInputElement | null>(null);
  const grid = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    box.current?.focus();
  }, []);

  const found = useMemo(() => matching(set, query), [set, query]);
  const searching = query.trim() !== "";
  const showing = searching ? found : (set.groups[category]?.emoji ?? []);

  /*
    Focus follows `at` rather than being moved at the key press, because the
    button being moved to may have only just been drawn: an arrow that changes
    the row is a render away from having anything to focus.
  */
  useEffect(() => {
    if (at === null) return;
    const keys = grid.current?.querySelectorAll("button");
    (keys?.item(at) as HTMLElement | undefined)?.focus();
  }, [at, showing]);

  /** Put the cursor back at the top whenever what is under it changes. */
  useEffect(() => {
    setAt(null);
  }, [query, category]);

  function moveTo(next: number) {
    setAt(Math.max(0, Math.min(showing.length - 1, next)));
  }

  /** The arrows, wherever they are pressed. Returns whether it took the key. */
  function steer(event: React.KeyboardEvent, from: number | null): boolean {
    if (showing.length === 0) return false;

    switch (event.key) {
      case "ArrowDown":
        moveTo(from === null ? 0 : from + ACROSS);
        return true;
      case "ArrowUp":
        if (from === null) return false;
        // Off the top row is back to the box, which is the only way back to it
        // without a mouse.
        if (from < ACROSS) {
          setAt(null);
          box.current?.focus();
          return true;
        }
        moveTo(from - ACROSS);
        return true;
      case "ArrowRight":
        if (from === null) return false;
        moveTo(from + 1);
        return true;
      case "ArrowLeft":
        if (from === null) return false;
        moveTo(from - 1);
        return true;
      case "Home":
        if (from === null) return false;
        moveTo(0);
        return true;
      case "End":
        if (from === null) return false;
        moveTo(showing.length - 1);
        return true;
      default:
        return false;
    }
  }

  /** What the live region says, and what names the grid. */
  const count =
    found.length === 0
      ? "No emoji match"
      : `${found.length} emoji match${found.length === 1 ? "es" : ""}`;

  return (
    <div className="emoji">
      <input
        ref={box}
        type="search"
        className="emoji__search"
        aria-label="Search emoji"
        placeholder="Search"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            const first = found[0];
            if (first !== undefined) onPick(inTone(first, tone));
            return;
          }
          if (steer(event, null)) event.preventDefault();
        }}
      />

      {/*
        Polite and always mounted. A region that appears with its first message
        is a region a screen reader has not been watching, so it is here from
        the start and empty until there is something to say.
      */}
      <p className="emoji__count" role="status">
        {searching ? count : ""}
      </p>

      {recent.length > 0 && !searching && (
        <div className="emoji__row" role="group" aria-label="Recently used">
          {recent.map((key) => (
            <button
              key={key}
              type="button"
              className="emoji__key"
              aria-label={`${action} ${key}`}
              aria-pressed={chosen === undefined ? undefined : chosen.has(key)}
              onClick={() => onPick(key)}
            >
              {key}
            </button>
          ))}
        </div>
      )}

      {!searching && (
        <div className="emoji__tabs" role="group" aria-label="Emoji categories">
          {set.groups.map((group, index) => (
            <button
              key={group.slug}
              type="button"
              className="emoji__tab"
              aria-current={index === category ? "true" : undefined}
              onClick={() => setCategory(index)}
            >
              {group.name}
            </button>
          ))}
        </div>
      )}

      <div
        ref={grid}
        className="emoji__grid"
        role="group"
        aria-label={searching ? count : (set.groups[category]?.name ?? "")}
        onKeyDown={(event) => {
          if (steer(event, at ?? 0)) event.preventDefault();
        }}
      >
        {showing.map((emoji, index) => (
          <Key
            key={emoji.key}
            emoji={emoji}
            action={action}
            tone={tone}
            tones={set.tones}
            chosen={chosen}
            // One stop for the whole grid. Before the arrows have been used
            // that is the first key, so Tab out of the box lands somewhere.
            inTabOrder={index === (at ?? 0)}
            onPick={onPick}
          />
        ))}
      </div>

      <div className="emoji__tones" role="group" aria-label="Skin tone">
        {set.tones.map((skin, index) => (
          <button
            key={skin.swatch}
            type="button"
            className="emoji__tone"
            aria-label={skin.name}
            aria-pressed={tone === index + 1}
            // Pressing the one already chosen takes it back, which is how
            // every other thing here that can be turned on works.
            onClick={() => onTone(tone === index + 1 ? 0 : index + 1)}
          >
            {skin.swatch}
          </button>
        ))}
      </div>
    </div>
  );
}

/** One key in the grid. */
function Key({
  emoji,
  action,
  tone,
  tones,
  chosen,
  inTabOrder,
  onPick,
}: {
  emoji: Emoji;
  action: string;
  tone: number;
  tones: readonly { name: string }[];
  chosen: ReadonlySet<string> | undefined;
  inTabOrder: boolean;
  onPick: (key: string) => void;
}) {
  const key = inTone(emoji, tone);
  // Named with the tone only where one applies, so that a smiley in a session
  // with a tone chosen is not announced as having a skin colour it has not.
  const toned = key === emoji.key ? "" : `, ${tones[tone - 1]?.name ?? ""}`;

  return (
    <button
      type="button"
      className="emoji__key"
      tabIndex={inTabOrder ? 0 : -1}
      aria-label={`${action} ${emoji.name}${toned}`}
      aria-pressed={chosen === undefined ? undefined : chosen.has(key)}
      onClick={() => onPick(key)}
    >
      {key}
    </button>
  );
}
