/**
 * The dataset, turned into what the picker draws.
 *
 * Reached only through `loadEmoji`, never imported directly. This module pulls
 * in the better part of a megabyte of JSON and the initial bundle has 150 kb
 * of budget, so it is the far side of an `import()` and has to stay there.
 *
 * ## Why emojibase and not a picker
 *
 * `emojibase-data` is JSON and nothing else. The alternative anybody reaches
 * for first is `emoji-mart`, which is a component library with its own styling
 * opinions, and this frontend has three runtime dependencies. The grid is
 * ours; the words in it are not worth writing by hand.
 *
 * Two things that only the data can answer, and both of them are why the
 * simpler package was put down again:
 *
 * ## The key is corrected on the way out
 *
 * A reaction key is an opaque string, so `1F44D` and `1F44D FE0F` are two
 * different reactions that draw as the same picture and count as two pills.
 * Unicode's emoji-test.txt calls the bare thumbs up fully-qualified and the
 * bare red heart unqualified: a character that is an emoji by default takes no
 * variation selector, and one that is text by default takes one. Emojibase
 * appends the selector to both, so the fully-qualified form is rebuilt here
 * from the hexcode, which carries it for a sequence and omits it for a single
 * character.
 *
 * Element sends emojibase's longer form. That disagreement predates this and
 * is not settled here: what this does is keep sending the twelve keys the
 * quick panel already sent, rather than quietly moving every thumbs up in the
 * history of a room onto a second pill.
 *
 * ## The skin tones are read, not built
 *
 * The obvious rule, insert a modifier after the first character, is wrong for
 * seven emoji: a couple carries two people and needs both toned. The next
 * obvious rule, tone every character that can take one, is wrong for people
 * holding hands, whose middle character is a handshake that can take one and
 * must not. Emojibase ships the RGI sequences, so neither rule is needed.
 */
import data from "emojibase-data/en/data.json";
import messages from "emojibase-data/en/messages.json";
import shortcodes from "emojibase-data/en/shortcodes/iamcal.json";

import type { Emoji, EmojiGroup, EmojiSet, SkinTone } from "./emoji";

/**
 * The group holding the skin tone modifiers and the hair colours.
 *
 * Left out. On its own a modifier is half a character, and sending one as a
 * reaction sends something no font draws.
 */
const COMPONENTS = 2;

/** What the five skin tone components are called, so they can be told from hair. */
const A_SKIN_TONE = /skin tone$/;

/** One emoji as the dataset holds it, narrowed to the fields read here. */
interface Entry {
  label: string;
  hexcode: string;
  tags?: string[];
  type: number;
  group?: number;
  skins?: { hexcode: string; tone: number | number[]; type: number }[];
}

/** Emojibase's `type` for a character that is an emoji without being asked. */
const EMOJI_PRESENTATION = 1;

/** The variation selector that asks a text character to be drawn as an emoji. */
const EMOJI_STYLE = "\u{FE0F}";

/**
 * The fully-qualified form of one entry.
 *
 * The hexcode is the sequence without the trailing selector a lone text
 * character needs, and with every selector a longer sequence needs already in
 * place. So the only correction is on a single character that is text by
 * default.
 */
function qualified(entry: { hexcode: string; type: number }): string {
  const points = entry.hexcode
    .split("-")
    .map((hex) => String.fromCodePoint(Number.parseInt(hex, 16)));
  const glyph = points.join("");
  if (points.length > 1 || entry.type === EMOJI_PRESENTATION) return glyph;
  return glyph + EMOJI_STYLE;
}

/** The words a search matches on: the name, the tags and the shortcodes. */
function termsOf(entry: Entry): readonly string[] {
  const listed = shortcodes[entry.hexcode as keyof typeof shortcodes];
  const short = listed === undefined ? [] : [listed].flat();

  const terms = new Set<string>();
  for (const phrase of [entry.label, ...(entry.tags ?? []), ...short]) {
    const lower = phrase.toLowerCase();
    // The whole of it as well as its words, so that "+1" and "thumbs_up" are
    // both typeable and so is either half of the second one.
    terms.add(lower);
    for (const word of lower.split(/[^a-z0-9+]+/)) {
      if (word !== "") terms.add(word);
    }
  }
  return [...terms];
}

/** One emoji's five tones, lightest first, or nothing where it has none. */
function skinsOf(entry: Entry): readonly string[] {
  const single = (entry.skins ?? []).filter(
    (skin) => typeof skin.tone === "number",
  );
  // Not the whole of `skins`: a couple carries one entry per pair of tones,
  // twenty-five of them, and the strip offers five.
  if (single.length === 0) return [];
  return single
    .sort((a, b) => (a.tone as number) - (b.tone as number))
    .map(qualified);
}

function emojiOf(entry: Entry): Emoji {
  return {
    key: qualified(entry),
    name: entry.label,
    terms: termsOf(entry),
    skins: skinsOf(entry),
  };
}

function groupsOf(entries: readonly Entry[]): readonly EmojiGroup[] {
  const named = new Map(
    messages.groups.map((group) => [group.order, group] as const),
  );
  const groups = new Map<number, Emoji[]>();
  for (const entry of entries) {
    if (entry.group === undefined || entry.group === COMPONENTS) continue;
    const into = groups.get(entry.group) ?? [];
    into.push(emojiOf(entry));
    groups.set(entry.group, into);
  }

  return [...groups.entries()]
    .sort(([a], [b]) => a - b)
    .map(([order, emoji]) => ({
      // Title case, because the dataset writes its group names in lower case
      // and these are labels on tabs rather than prose.
      name: titled(named.get(order)?.message ?? ""),
      slug: named.get(order)?.key ?? String(order),
      emoji,
    }));
}

/** "smileys & emotion" as "Smileys & Emotion". */
function titled(name: string): string {
  return name.replace(/\b[a-z]/g, (letter) => letter.toUpperCase());
}

function tonesOf(entries: readonly Entry[]): readonly SkinTone[] {
  return entries
    .filter(
      (entry) => entry.group === COMPONENTS && A_SKIN_TONE.test(entry.label),
    )
    .map((entry) => ({ name: entry.label, swatch: qualified(entry) }));
}

const entries = data as readonly Entry[];

/** The whole standard set. */
export const set: EmojiSet = {
  groups: groupsOf(entries),
  tones: tonesOf(entries),
};
