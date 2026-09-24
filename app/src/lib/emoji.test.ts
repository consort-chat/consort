/// <reference types="vite/client" />
import { describe, expect, it } from "vitest";

import {
  inTone,
  loadEmoji,
  matching,
  type Emoji,
  type EmojiSet,
} from "./emoji";

/** An emoji with only the fields the pure functions read. */
function key(
  key: string,
  name: string,
  terms: string[],
  skins: string[] = [],
): Emoji {
  return { key, name, terms, skins };
}

/**
 * A set small enough to reason about.
 *
 * The pure half of this module is tested against this rather than against the
 * real dataset, so that a rule about ordering is read off four entries instead
 * of nineteen hundred.
 */
const SMALL: EmojiSet = {
  groups: [
    {
      name: "Smileys & Emotion",
      slug: "smileys-emotion",
      emoji: [
        key("😀", "grinning face", [
          "grinning",
          "face",
          "grin",
          "smile",
          "happy",
          "upside",
        ]),
        key("😃", "grinning face with big eyes", [
          "grinning",
          "face",
          "big",
          "eyes",
          "happy",
        ]),
      ],
    },
    {
      name: "People & Body",
      slug: "people-body",
      emoji: [
        key("👍", "thumbs up", ["thumbs", "up", "+1", "thumbsup", "like"]),
        key(
          "👋",
          "waving hand",
          ["waving", "hand", "wave", "hi"],
          ["👋🏻", "👋🏼", "👋🏽", "👋🏾", "👋🏿"],
        ),
      ],
    },
  ],
  tones: [
    { name: "light skin tone", swatch: "🏻" },
    { name: "medium-light skin tone", swatch: "🏼" },
    { name: "medium skin tone", swatch: "🏽" },
    { name: "medium-dark skin tone", swatch: "🏾" },
    { name: "dark skin tone", swatch: "🏿" },
  ],
};

const keys = (found: readonly Emoji[]) => found.map((one) => one.key);

describe("searching the set", () => {
  it("finds nothing at all until something is typed", () => {
    // The grid draws its categories instead, so an empty query is not "every
    // emoji" but "no search running".
    expect(matching(SMALL, "")).toEqual([]);
    expect(matching(SMALL, "   ")).toEqual([]);
  });

  it("matches a term by its start rather than anywhere inside it", () => {
    expect(keys(matching(SMALL, "gr"))).toEqual(["😀", "😃"]);
    // "rin" is inside "grinning", and matching it would make every short
    // query answer with most of the set.
    expect(matching(SMALL, "rin")).toEqual([]);
  });

  it("looks past the name into the tags and the shortcodes", () => {
    expect(keys(matching(SMALL, "happy"))).toEqual(["😀", "😃"]);
    expect(keys(matching(SMALL, "thumbsup"))).toEqual(["👍"]);
    expect(keys(matching(SMALL, "+1"))).toEqual(["👍"]);
  });

  it("ignores case and surrounding space", () => {
    expect(keys(matching(SMALL, "  HAPPY "))).toEqual(["😀", "😃"]);
  });

  it("puts a whole term ahead of one the query only starts", () => {
    /*
      "up" is the whole of a term on the thumb and the start of "upside" on the
      grinning face, and the grinning face is first in the set. Somebody typing
      the whole of a word means that word, so it goes above what it is merely
      a prefix of.
    */
    expect(keys(matching(SMALL, "up"))).toEqual(["👍", "😀"]);
  });

  it("crosses the categories, which a search has to", () => {
    expect(keys(matching(SMALL, "h"))).toContain("😀");
    expect(keys(matching(SMALL, "h"))).toContain("👋");
  });
});

describe("choosing a skin tone", () => {
  const waving = SMALL.groups[1]!.emoji[1]!;
  const grinning = SMALL.groups[0]!.emoji[0]!;

  it("hands back the tone that was asked for", () => {
    expect(inTone(waving, 1)).toBe("👋🏻");
    expect(inTone(waving, 5)).toBe("👋🏿");
  });

  it("hands back the plain one when no tone is set", () => {
    expect(inTone(waving, 0)).toBe("👋");
  });

  it("hands back the plain one for an emoji that has no tones", () => {
    // A tone somebody chose applies to the hands and not to the smileys, and
    // the grid asks for it on every key rather than checking first.
    expect(inTone(grinning, 3)).toBe("😀");
  });
});

describe("the dataset behind it", () => {
  it("starts the load once and hands the same promise back after that", async () => {
    /*
      The promise rather than what it resolves to. Two pickers opening at once
      must share one import, and comparing the resolved sets would pass on a
      module that started the import twice: the dataset is a module-level
      constant either way, so only the promise tells the two apart.
    */
    const first = loadEmoji();
    const second = loadEmoji();

    expect(second).toBe(first);
    await first;
  });

  it("is grouped the way the picker draws its tabs", async () => {
    const set = await loadEmoji();

    expect(set.groups.map((one) => one.name)).toEqual([
      "Smileys & Emotion",
      "People & Body",
      "Animals & Nature",
      "Food & Drink",
      "Travel & Places",
      "Activities",
      "Objects",
      "Symbols",
      "Flags",
    ]);
  });

  it("leaves out the components, which are not emoji anybody sends", async () => {
    const set = await loadEmoji();

    // The skin tone modifiers and the hair colours are their own group in the
    // data. On their own they are half a character.
    expect(set.groups.map((one) => one.slug)).not.toContain("component");
    expect(everyEmoji(set).map((one) => one.key)).not.toContain("🏻");
  });

  it("names the five skin tones from the data rather than a list here", async () => {
    const set = await loadEmoji();

    expect(set.tones).toEqual([
      { name: "light skin tone", swatch: "🏻" },
      { name: "medium-light skin tone", swatch: "🏼" },
      { name: "medium skin tone", swatch: "🏽" },
      { name: "medium-dark skin tone", swatch: "🏾" },
      { name: "dark skin tone", swatch: "🏿" },
    ]);
  });

  /*
    The one that decides whether a reaction sent from here lands on the same
    pill as everybody else's.

    A reaction key is an opaque string, so `1F44D` and `1F44D FE0F` are two
    different reactions that draw as the same picture. Unicode's own
    emoji-test.txt calls the bare form of a thumbs up fully-qualified and the
    bare form of a red heart unqualified, and those are the two shapes: a
    character that is an emoji by default takes no variation selector, and one
    that is text by default takes one.

    Emojibase hands out `1F44D FE0F` for the first of those, so this is a
    correction rather than a passthrough and it needs holding down.
  */
  it("sends the fully-qualified form of a key and nothing longer", async () => {
    const set = await loadEmoji();
    const byKey = new Map(everyEmoji(set).map((one) => [one.key, one]));

    expect(byKey.has("\u{1F44D}")).toBe(true);
    expect(byKey.has("\u{1F44D}\u{FE0F}")).toBe(false);
    expect(byKey.has("\u{2764}\u{FE0F}")).toBe(true);
    expect(byKey.has("\u{2764}")).toBe(false);
  });

  it("keeps the twelve keys the quick panel used to offer", async () => {
    // What this picker replaces. Every one of them has to still be reachable,
    // and they are the default contents of the recently used row.
    const set = await loadEmoji();
    const all = new Set(everyEmoji(set).map((one) => one.key));

    for (const one of ["👍", "👎", "😄", "🎉", "😕", "❤️", "🚀", "👀", "✅", "🙏", "🔥", "😢"]) {
      expect(all).toContain(one);
    }
  });

  it("has one entry per key", async () => {
    const set = await loadEmoji();
    const all = everyEmoji(set);

    expect(new Set(all.map((one) => one.key)).size).toBe(all.length);
  });

  it("carries five tones for an emoji that has them and none for one that does not", async () => {
    const set = await loadEmoji();
    const byKey = new Map(everyEmoji(set).map((one) => [one.key, one]));

    expect(byKey.get("👋")?.skins).toEqual(["👋🏻", "👋🏼", "👋🏽", "👋🏾", "👋🏿"]);
    expect(byKey.get("🎉")?.skins).toEqual([]);
  });

  /*
    People holding hands is the one the obvious rule gets wrong. Its sequence
    holds three tone-capable characters, and the handshake in the middle is one
    of them, so inserting a modifier after each would produce a sequence no
    font has. The tones are read out of the data rather than built here, which
    is why this passes.
  */
  it("takes a multi-person tone from the data rather than building one", async () => {
    const set = await loadEmoji();
    const byKey = new Map(everyEmoji(set).map((one) => [one.key, one]));

    expect(byKey.get("🧑‍🤝‍🧑")?.skins[4]).toBe("🧑🏿‍🤝‍🧑🏿");
  });

  it("searches the real set by the words somebody would actually type", async () => {
    const set = await loadEmoji();

    expect(keys(matching(set, "thumbsup"))).toContain("👍");
    expect(keys(matching(set, "happy"))).toContain("😀");
    expect(keys(matching(set, "shrug"))).toContain("🤷");
    expect(keys(matching(set, "rocket"))).toContain("🚀");
  });

  it("takes a shortcode or a name whole, not only a word out of one", async () => {
    // Somebody who knows the shortcode types the shortcode, underscores and
    // all, and somebody reading the name off a tooltip types the space.
    const set = await loadEmoji();

    expect(keys(matching(set, "raised_hands"))).toContain("🙌");
    expect(keys(matching(set, "thumbs up"))).toContain("👍");
  });
});

/*
  The dataset is the better part of a megabyte and the landing budget is 150 kb
  of JavaScript, so it lives behind an `import()` and has to stay there. The
  failure is silent: one static import anywhere folds the whole thing into the
  first chunk, everything still works, and nothing says so until somebody reads
  a build log they had no reason to read.

  Read off the source rather than off a build, because a test that had to build
  first is a test nobody runs.
*/
describe("keeping the dataset out of the first chunk", () => {
  /*
    Vite's own raw glob rather than `node:fs`, which would want `@types/node`
    in a tree that has three runtime dependencies and no need of a fourth.
  */
  const sources = Object.entries(
    import.meta.glob("../**/*.{ts,tsx}", {
      query: "?raw",
      import: "default",
      eager: true,
    }) as Record<string, string>,
  )
    .filter(([at]) => !/\.test\.tsx?$/.test(at))
    .map(([at, text]) => ({ at, text }));

  it("found the source to read, so an empty sweep cannot pass", () => {
    expect(sources.length).toBeGreaterThan(20);
  });

  it("is imported statically by nothing at all", () => {
    const statically = sources.filter(
      (file) =>
        !file.at.endsWith("/emojiSet.ts") &&
        /from\s+["'][^"']*emojiSet["']/.test(file.text),
    );

    expect(statically.map((file) => file.at)).toEqual([]);
  });

  it("is reached through an import() that is still there", () => {
    const at = sources.find((file) => file.at.endsWith("/emoji.ts"));

    expect(at?.text).toMatch(/import\(["']\.\/emojiSet["']\)/);
  });
});

/** Every emoji in the set, in the order the picker would draw them. */
function everyEmoji(set: EmojiSet): readonly Emoji[] {
  return set.groups.flatMap((one) => one.emoji);
}
