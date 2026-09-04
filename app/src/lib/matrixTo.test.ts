import { describe, expect, it } from "vitest";

import { linkLabel, matrixTarget, withAddressesNamed } from "./matrixTo";

describe("matrixTarget", () => {
  it("reads a person out of the fragment", () => {
    expect(matrixTarget("https://matrix.to/#/@ada:example.org")).toEqual({
      kind: "person",
      userId: "@ada:example.org",
    });
  });

  it("reads a room by its ID", () => {
    expect(matrixTarget("https://matrix.to/#/!general:example.org")).toEqual({
      kind: "room",
      roomOrAlias: "!general:example.org",
    });
  });

  it("reads a room by its alias", () => {
    expect(matrixTarget("https://matrix.to/#/#general:example.org")).toEqual({
      kind: "room",
      roomOrAlias: "#general:example.org",
    });
  });

  it("reads a message as the room it is in and the event it is", () => {
    expect(
      matrixTarget("https://matrix.to/#/!general:example.org/$said:example.org"),
    ).toEqual({
      kind: "message",
      roomOrAlias: "!general:example.org",
      eventId: "$said:example.org",
    });
  });

  it("reads a percent-encoded address, which is what Element writes", () => {
    // Consort's own permalinks are not encoded and other clients' are. Both
    // are valid, and a client that read only one of them would draw half the
    // links in a room as raw addresses.
    expect(
      matrixTarget(
        "https://matrix.to/#/%21general%3Aexample.org/%24said%3Aexample.org",
      ),
    ).toEqual({
      kind: "message",
      roomOrAlias: "!general:example.org",
      eventId: "$said:example.org",
    });
  });

  it("ignores the servers a link suggests joining through", () => {
    // Consort does not join a room from a link, so the routing hints are of no
    // use here. Reading them as part of the event ID would be.
    expect(
      matrixTarget(
        "https://matrix.to/#/!general:example.org/$said:example.org?via=example.org&via=other.example",
      ),
    ).toEqual({
      kind: "message",
      roomOrAlias: "!general:example.org",
      eventId: "$said:example.org",
    });
  });

  it("is nothing for an ordinary website", () => {
    expect(matrixTarget("https://example.org/general")).toBeUndefined();
  });

  it("is nothing for a matrix.to address naming nothing", () => {
    expect(matrixTarget("https://matrix.to/")).toBeUndefined();
    expect(matrixTarget("https://matrix.to/#/")).toBeUndefined();
  });

  it("is nothing for a sigil this build has no idea about", () => {
    // Better an address that opens a browser than a badge that goes nowhere.
    expect(matrixTarget("https://matrix.to/#/+space:example.org")).toBeUndefined();
  });

  it("survives an address that is not an address", () => {
    expect(matrixTarget("not a link")).toBeUndefined();
    expect(matrixTarget(null)).toBeUndefined();
    expect(matrixTarget(undefined)).toBeUndefined();
  });

  it("survives a stray percent somebody typed", () => {
    // `decodeURIComponent` throws on one, and a message body is not a place to
    // throw from.
    expect(matrixTarget("https://matrix.to/#/!100%:example.org")).toEqual({
      kind: "room",
      roomOrAlias: "!100%:example.org",
    });
  });
});

describe("linkLabel", () => {
  const room = { kind: "room", roomOrAlias: "!general:example.org" } as const;
  const message = {
    kind: "message",
    roomOrAlias: "!general:example.org",
    eventId: "$said:example.org",
  } as const;

  it("uses what this account calls the room", () => {
    expect(linkLabel(room, "#general")).toBe("#general");
    expect(linkLabel(message, "#general")).toBe("Message in #general");
  });

  it("falls back to the alias, which is a name somebody chose", () => {
    const alias = { kind: "room", roomOrAlias: "#general:example.org" } as const;

    expect(linkLabel(alias, null)).toBe("#general:example.org");
  });

  it("never puts a room ID in front of a person", () => {
    // Eighteen random characters say nothing about where the link goes, which
    // is the whole thing a badge exists to fix.
    expect(linkLabel(room, null)).toBe("A room");
    expect(linkLabel(message, null)).toBe("A message");
  });
});

describe("withAddressesNamed", () => {
  const nameOf = (roomOrAlias: string) =>
    roomOrAlias === "!voice:example.org" ? "#voice" : null;

  it("says what the badge says, because a quote cannot hold one", () => {
    // The complaint this exists for: a reply quoting a permalink showed sixty
    // characters of room ID where the message above it showed a badge.
    expect(
      withAddressesNamed(
        "Testing https://matrix.to/#/!voice:example.org/$said:example.org",
        nameOf,
      ),
    ).toBe("Testing Message in #voice");
  });

  it("leaves an ordinary address alone", () => {
    // It is a website, and the message draws it as one.
    expect(withAddressesNamed("see https://example.org/x", nameOf)).toBe(
      "see https://example.org/x",
    );
  });

  it("leaves words with no address in them exactly as they are", () => {
    expect(withAddressesNamed("nothing to see here", nameOf)).toBe(
      "nothing to see here",
    );
  });

  it("leaves a link to a person alone, having no name to put there", () => {
    expect(
      withAddressesNamed("https://matrix.to/#/@ada:example.org", nameOf),
    ).toBe("https://matrix.to/#/@ada:example.org");
  });
});
