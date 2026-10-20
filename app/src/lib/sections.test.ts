import { describe, expect, it } from "vitest";

import { UNJOINED, arrange, moved, sectionsOf } from "./sections";
import type { Channel, CustomSection } from "./api";
import type { Drawn } from "./sections";

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

/** A channel, reduced to what `sectionsOf` reads off one. */
function channel(id: string, kind: Channel["kind"] = "text"): Channel {
  return {
    id,
    name: id,
    kind,
    avatar: null,
    joined: true,
    participants: [],
    unread: 0,
    mentions: 0,
  };
}

/** The same, for a room this account is not in. */
function onOffer(id: string, kind: Channel["kind"] = "text"): Channel {
  return { ...channel(id, kind), joined: false };
}

/** A section somebody made, under the one space these tests use. */
function custom(key: string, name: string, rooms: string[]): CustomSection {
  return { key, name, space: "!s:example.org", rooms };
}

/** The keys and the room IDs of what was drawn, which is what is under test. */
function shapeOf(sections: readonly Drawn[]) {
  return sections.map((section) => ({
    key: section.key,
    rooms: section.channels.map((one) => one.id),
  }));
}

describe("sectionsOf", () => {
  it("splits a space into Text and Voice when nobody has made a section", () => {
    const channels = [channel("!a"), channel("!b", "voice")];

    expect(shapeOf(sectionsOf(channels, [], []))).toEqual([
      { key: "text", rooms: ["!a"] },
      { key: "voice", rooms: ["!b"] },
    ]);
  });

  it("omits a built-in section with nothing in it", () => {
    // A "VOICE" heading over nothing reads as a list that failed to load.
    expect(shapeOf(sectionsOf([channel("!a")], [], []))).toEqual([
      { key: "text", rooms: ["!a"] },
    ]);
  });

  it("draws a section somebody made even when it is empty", () => {
    // The opposite rule, and deliberately: an empty section that was not
    // drawn would be one nothing could ever be put into.
    expect(
      shapeOf(sectionsOf([], [custom("custom-1", "Projects", [])], [])),
    ).toEqual([{ key: "custom-1", rooms: [] }]);
  });

  it("takes a room out of Text once it is in a section of its own", () => {
    const channels = [channel("!a"), channel("!b")];

    expect(
      shapeOf(sectionsOf(channels, [custom("custom-1", "Projects", ["!a"])], [])),
    ).toEqual([
      { key: "text", rooms: ["!b"] },
      { key: "custom-1", rooms: ["!a"] },
    ]);
  });

  it("takes a voice room out of Voice the same way", () => {
    const channels = [channel("!a", "voice")];

    expect(
      shapeOf(sectionsOf(channels, [custom("custom-1", "Projects", ["!a"])], [])),
    ).toEqual([{ key: "custom-1", rooms: ["!a"] }]);
  });

  it("holds text and voice rooms side by side in one section", () => {
    // The point of a section of your own: a project is the channel and the
    // call about it, and splitting those by kind is what #170 is undoing.
    const channels = [channel("!a"), channel("!b", "voice")];

    expect(
      shapeOf(
        sectionsOf(channels, [custom("custom-1", "Projects", ["!a", "!b"])], []),
      ),
    ).toEqual([{ key: "custom-1", rooms: ["!a", "!b"] }]);
  });

  it("ignores a room a section claims that this space does not have", () => {
    // A room left behind by a leave, or one whose space changed. The section
    // is still drawn, because the claim is stale rather than the section.
    expect(
      shapeOf(sectionsOf([], [custom("custom-1", "Projects", ["!gone"])], [])),
    ).toEqual([{ key: "custom-1", rooms: [] }]);
  });

  it("keeps the order rooms arrive in rather than the order they were added", () => {
    // The order comes from Rust, which follows MSC1772. A section that drew
    // its rooms in the order somebody dragged them in would disagree with
    // every other section about where a channel sits.
    const channels = [channel("!a"), channel("!b")];

    expect(
      shapeOf(
        sectionsOf(channels, [custom("custom-1", "Projects", ["!b", "!a"])], []),
      ),
    ).toEqual([{ key: "custom-1", rooms: ["!a", "!b"] }]);
  });

  it("follows the order somebody dragged the sections into", () => {
    const channels = [channel("!a"), channel("!b")];
    const sections = sectionsOf(
      channels,
      [custom("custom-1", "Projects", ["!a"])],
      ["custom-1", "text"],
    );

    expect(shapeOf(sections)).toEqual([
      { key: "custom-1", rooms: ["!a"] },
      { key: "text", rooms: ["!b"] },
    ]);
  });

  it("says which sections can be renamed and deleted", () => {
    const sections = sectionsOf(
      [channel("!a")],
      [custom("custom-1", "Projects", [])],
      [],
    );

    expect(sections.map((one) => ({ key: one.key, custom: one.custom }))).toEqual([
      { key: "text", custom: false },
      { key: "custom-1", custom: true },
    ]);
  });

  it("labels a section somebody made with the name they gave it", () => {
    const sections = sectionsOf([], [custom("custom-1", "Projects", [])], []);

    expect(sections.map((one) => one.label)).toEqual(["Projects"]);
  });

  describe("the rooms this account is not in", () => {
    it("puts them in a group of their own rather than beside the rest", () => {
      // #128: a space's unjoined children arrive in the same list as the
      // channels somebody actually uses, and on a server of any size they
      // outnumber them.
      const channels = [channel("!a"), onOffer("!b"), onOffer("!c", "voice")];

      expect(shapeOf(sectionsOf(channels, [], []))).toEqual([
        { key: "text", rooms: ["!a"] },
        { key: UNJOINED, rooms: ["!b", "!c"] },
      ]);
    });

    it("holds both kinds, because joining is the question either way", () => {
      const channels = [onOffer("!a"), onOffer("!b", "voice")];

      expect(shapeOf(sectionsOf(channels, [], []))).toEqual([
        { key: UNJOINED, rooms: ["!a", "!b"] },
      ]);
    });

    it("is dropped when there is nothing on offer", () => {
      // A built-in section's rule rather than a made one's: a heading over
      // nothing reads as a list that failed to load, and there is nothing to
      // put in this one by hand.
      expect(shapeOf(sectionsOf([channel("!a")], [], []))).toEqual([
        { key: "text", rooms: ["!a"] },
      ]);
    });

    it("leaves a room somebody filed in a section of their own alone", () => {
      // Putting it there was a choice. A group that swallowed it would be the
      // sidebar overruling somebody, and the room would move the moment they
      // were let in anyway.
      const channels = [onOffer("!a"), onOffer("!b")];

      expect(
        shapeOf(
          sectionsOf(channels, [custom("custom-1", "Projects", ["!a"])], []),
        ),
      ).toEqual([
        { key: "custom-1", rooms: ["!a"] },
        { key: UNJOINED, rooms: ["!b"] },
      ]);
    });

    it("is drawn last until somebody drags it, sections and all", () => {
      const channels = [channel("!a"), onOffer("!b")];
      const sections = sectionsOf(
        channels,
        [custom("custom-1", "Projects", [])],
        [],
      );

      expect(keysOf(sections)).toEqual(["text", "custom-1", UNJOINED]);
    });

    it("goes where somebody dragged it", () => {
      const channels = [channel("!a"), onOffer("!b")];

      expect(
        keysOf(sectionsOf(channels, [], [UNJOINED, "text"])),
      ).toEqual([UNJOINED, "text"]);
    });

    it("cannot be renamed or deleted", () => {
      const sections = sectionsOf([onOffer("!a")], [], []);

      expect(sections.map((one) => one.custom)).toEqual([false]);
    });
  });
});
