import { describe, expect, it } from "vitest";

import { arrange, moved } from "./sections";

/** What the sidebar itself orders the sections in, before anybody drags one. */
const BUILT_IN = [{ key: "text" }, { key: "voice" }];

/** The keys of what `arrange` handed back, which is the part under test. */
function keysOf(sections: readonly { key: string }[]): string[] {
  return sections.map((section) => section.key);
}

describe("arrange", () => {
  it("leaves the sidebar's own order alone when nobody has dragged one", () => {
    expect(keysOf(arrange(BUILT_IN, []))).toEqual(["text", "voice"]);
  });

  it("follows a stored order", () => {
    expect(keysOf(arrange(BUILT_IN, ["voice", "text"]))).toEqual([
      "voice",
      "text",
    ]);
  });

  it("ignores a stored key this build draws no section for", () => {
    // #170 lets somebody make a section, and a file written by a build that
    // has them is one this build can be asked to read.
    expect(keysOf(arrange(BUILT_IN, ["voice", "notes", "text"]))).toEqual([
      "voice",
      "text",
    ]);
  });

  it("puts a section the stored order has never heard of after the ones it has", () => {
    // The other way round: a section this build added, read against a file
    // written before it existed. It has to be drawn somewhere, and after the
    // arrangement somebody chose is the only place that does not disturb it.
    const later = [{ key: "text" }, { key: "voice" }, { key: "notes" }];

    expect(keysOf(arrange(later, ["voice", "text"]))).toEqual([
      "voice",
      "text",
      "notes",
    ]);
  });

  it("keeps two sections the stored order never heard of in the order they were given", () => {
    // Stable, rather than whichever way the engine's sort happens to fall.
    const later = [{ key: "notes" }, { key: "files" }, { key: "voice" }];

    expect(keysOf(arrange(later, ["voice"]))).toEqual([
      "voice",
      "notes",
      "files",
    ]);
  });

  it("draws a section a hand-edited file lists twice only once", () => {
    expect(keysOf(arrange(BUILT_IN, ["voice", "voice", "text"]))).toEqual([
      "voice",
      "text",
    ]);
  });
});

describe("moved", () => {
  it("puts a section where the one it was dropped on was", () => {
    expect(moved(["a", "b", "c"], "a", "c")).toEqual(["b", "c", "a"]);
  });

  it("moves one up as well as down", () => {
    expect(moved(["a", "b", "c"], "c", "a")).toEqual(["c", "a", "b"]);
  });

  it("changes nothing when a section is dropped on itself", () => {
    expect(moved(["a", "b", "c"], "b", "b")).toEqual(["a", "b", "c"]);
  });

  it("changes nothing when the thing dragged is not a section", () => {
    // A drag can come from outside the application carrying any text at all,
    // and the list is what it would otherwise be rearranged by.
    expect(moved(["a", "b", "c"], "/etc/passwd", "b")).toEqual(["a", "b", "c"]);
  });

  it("changes nothing when it is dropped on something that is not a section", () => {
    expect(moved(["a", "b", "c"], "a", "elsewhere")).toEqual(["a", "b", "c"]);
  });
});
