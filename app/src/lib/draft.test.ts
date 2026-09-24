import { describe, expect, it } from "vitest";

import { withInserted } from "./draft";

describe("putting an emoji into a draft", () => {
  it("puts it where the caret is, not at the end", () => {
    expect(withInserted("hello world", 5, 5, " 👋")).toEqual({
      text: "hello 👋 world",
      caret: 8,
    });
  });

  it("appends when the caret is at the end, which is the usual case", () => {
    expect(withInserted("hello", 5, 5, "👋")).toEqual({
      text: "hello👋",
      caret: 7,
    });
  });

  it("puts it in an empty box", () => {
    expect(withInserted("", 0, 0, "👋")).toEqual({ text: "👋", caret: 2 });
  });

  it("replaces whatever was selected", () => {
    // Typing a character over a selection replaces it, and pressing a key in
    // the picker is typing a character.
    expect(withInserted("hello world", 6, 11, "👋")).toEqual({
      text: "hello 👋",
      caret: 8,
    });
  });

  /*
    The caret is counted the way a textarea counts it, in UTF-16 units rather
    than characters. Most emoji are two of those and a few are a good many
    more, so a caret moved by the length in characters would land inside the
    emoji just inserted and the next one would break it in half.
  */
  it("moves the caret past the whole of a long key", () => {
    const family = "👨‍👩‍👧";

    const { text, caret } = withInserted("", 0, 0, family);

    expect(caret).toBe(family.length);
    expect(text.slice(0, caret)).toBe(family);
  });
});
