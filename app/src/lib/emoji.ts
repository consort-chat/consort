/**
 * The standard emoji set, and the two rules the picker applies to it.
 *
 * Split from [`emojiSet`], which is the dataset itself and is reached through
 * [`loadEmoji`] rather than imported. That split is the whole point of this
 * file: the data is the better part of a megabyte and the initial bundle has a
 * budget of 150 kb, so nothing here may name `./emojiSet` outside an
 * `import()`. A static import anywhere would pull the lot into the first
 * chunk, and it would do it silently.
 */

/** One key somebody can press in the picker. */
export interface Emoji {
  /**
   * What gets sent, or typed into the box.
   *
   * Unicode's fully-qualified form. See [`emojiSet`] for why that is a
   * correction rather than a passthrough, and why it decides whether a
   * reaction from here lands on the same pill as everybody else's.
   */
  key: string;
  /** What it is called, which is what a screen reader says. */
  name: string;
  /**
   * Everything [`matching`] looks at, lowercased.
   *
   * The words of the name, the annotation's tags, and the shortcodes. Flat and
   * precomputed, because a search runs on every keystroke over nineteen
   * hundred entries and splitting the names again each time is the one part of
   * this that would be felt.
   */
  terms: readonly string[];
  /**
   * The same emoji in each of the five skin tones, lightest first.
   *
   * Empty for the great majority, which have none. Read out of the dataset
   * rather than built by inserting a modifier: see [`emojiSet`].
   */
  skins: readonly string[];
}

/** One tab's worth. */
export interface EmojiGroup {
  /** What the tab says. */
  name: string;
  /** Stable across releases, unlike the name, so it can be a React key. */
  slug: string;
  emoji: readonly Emoji[];
}

/** One of the five skin tones, as the strip that chooses between them draws it. */
export interface SkinTone {
  name: string;
  /** The modifier character on its own, which renders as a colour. */
  swatch: string;
}

/** The whole standard set, grouped the way the picker draws it. */
export interface EmojiSet {
  groups: readonly EmojiGroup[];
  tones: readonly SkinTone[];
}

/**
 * The set, fetched on first use and held after that.
 *
 * The promise rather than the value, so that two components opening at once
 * share one import instead of racing to start two.
 */
let pending: Promise<EmojiSet> | null = null;

/** The standard set, loading it the first time somebody opens a picker. */
export function loadEmoji(): Promise<EmojiSet> {
  pending ??= import("./emojiSet").then((module) => module.set);
  return pending;
}

/**
 * The emoji whose name, tags or shortcodes begin with `query`.
 *
 * Empty for an empty query, which is "no search running" rather than
 * "everything": the grid draws its categories in that case.
 *
 * Matched on the start of a term rather than anywhere inside one. Substring
 * matching sounds more generous and is not: "rin" would answer with every
 * grinning face in the set, and a two-letter query would answer with most of
 * it.
 */
export function matching(set: EmojiSet, query: string): readonly Emoji[] {
  const wanted = query.trim().toLowerCase();
  if (wanted === "") return [];

  const whole: Emoji[] = [];
  const partial: Emoji[] = [];
  for (const group of set.groups) {
    for (const emoji of group.emoji) {
      if (emoji.terms.includes(wanted)) whole.push(emoji);
      else if (emoji.terms.some((term) => term.startsWith(wanted))) {
        partial.push(emoji);
      }
    }
  }
  // Somebody who typed a whole word means that word, so those come first. Set
  // order within each half, which is Unicode's own.
  return [...whole, ...partial];
}

/**
 * `emoji` in the chosen skin tone, or as it comes.
 *
 * `tone` is 1 to 5, lightest to darkest, and 0 for no tone at all. Anything
 * the emoji has no variant for gives the plain key back, which is most of
 * them: the grid asks for every key it draws rather than checking first.
 */
export function inTone(emoji: Emoji, tone: number): string {
  return emoji.skins[tone - 1] ?? emoji.key;
}
