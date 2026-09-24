import { describe, expect, it } from "vitest";

import { matchingChannels } from "./channelSearch";
import type { Channel } from "./api";

function channel(name: string | null, joined = true): Channel {
  return {
    id: `!${name ?? "none"}:example.org`,
    name,
    kind: "text",
    avatar: null,
    joined,
    participants: [],
    unread: 0,
    mentions: 0,
  };
}

/** The names that came back, so a test reads as what somebody would see. */
function names(channels: Channel[]): (string | null)[] {
  return channels.map((found) => found.name);
}

describe("matchingChannels", () => {
  const all = [
    channel("general"),
    channel("Announcements"),
    channel("design-crit"),
  ];

  it("keeps only the channels whose name holds what was typed", () => {
    expect(names(matchingChannels(all, "gene"))).toEqual(["general"]);
  });

  it("drops the channels whose name does not", () => {
    expect(names(matchingChannels(all, "zzz"))).toEqual([]);
  });

  it("matches without caring about case, in either direction", () => {
    // Typing in lower case is what everybody does, and a channel named with a
    // capital is not a channel somebody should have to spell exactly.
    expect(names(matchingChannels(all, "announce"))).toEqual(["Announcements"]);
    expect(names(matchingChannels(all, "GENERAL"))).toEqual(["general"]);
  });

  it("matches inside a name and not only at the start of one", () => {
    // Channel names are compounds far more often than they are single words,
    // and "crit" is how somebody looks for design-crit.
    expect(names(matchingChannels(all, "crit"))).toEqual(["design-crit"]);
  });

  it("keeps more than one when more than one matches", () => {
    expect(names(matchingChannels([channel("news"), channel("newsletter")], "news"))).toEqual([
      "news",
      "newsletter",
    ]);
  });

  it("keeps the order it was given rather than ranking", () => {
    // The order comes from Rust, which follows MSC1772. A list that re-sorted
    // itself by how well each name matched would move under the pointer as
    // somebody typed.
    expect(names(matchingChannels(all, "n"))).toEqual([
      "general",
      "Announcements",
      "design-crit",
    ]);
  });

  it("keeps everything when nothing has been typed", () => {
    expect(names(matchingChannels(all, ""))).toEqual([
      "general",
      "Announcements",
      "design-crit",
    ]);
  });

  it("keeps everything when only spaces have been typed", () => {
    // A field somebody has tabbed through and left a space in is a field with
    // no query in it, and answering it with nothing would read as a space that
    // had lost its channels.
    expect(names(matchingChannels(all, "   "))).toEqual([
      "general",
      "Announcements",
      "design-crit",
    ]);
  });

  it("ignores the spaces either side of what was typed", () => {
    // A name pasted in brings its whitespace with it.
    expect(names(matchingChannels(all, "  gene  "))).toEqual(["general"]);
  });

  it("searches what an unnamed channel is drawn as", () => {
    // A room a space lists and nobody has joined has no name until the
    // hierarchy request fills one in, and it is drawn as "Unknown channel".
    // Searching the null would be searching something nobody can see.
    expect(names(matchingChannels([channel(null, false)], "unknown"))).toEqual([
      null,
    ]);
  });
});
