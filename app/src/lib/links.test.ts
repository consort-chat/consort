import { describe, expect, it } from "vitest";

import { linkify } from "./links";

/** The addresses in a run of text, in order. */
function addresses(text: string): string[] {
  return linkify(text).flatMap((piece) =>
    piece.href === undefined ? [] : [piece.href],
  );
}

describe("linkify", () => {
  it("leaves words alone", () => {
    expect(linkify("nothing to see here")).toEqual([
      { text: "nothing to see here" },
    ]);
  });

  it("says nothing about nothing", () => {
    expect(linkify("")).toEqual([]);
  });

  it("finds an address on its own", () => {
    expect(linkify("https://example.org")).toEqual([
      { text: "https://example.org", href: "https://example.org" },
    ]);
  });

  it("keeps the words on either side of one", () => {
    expect(linkify("see https://example.org for more")).toEqual([
      { text: "see " },
      { text: "https://example.org", href: "https://example.org" },
      { text: " for more" },
    ]);
  });

  it("finds every address in a line", () => {
    expect(
      addresses("https://example.org and http://example.net/x too"),
    ).toEqual(["https://example.org", "http://example.net/x"]);
  });

  it("leaves the full stop at the end of a sentence out of it", () => {
    // The commonest way to write one, and the commonest way to get a 404.
    expect(addresses("go to https://example.org/page.")).toEqual([
      "https://example.org/page",
    ]);
  });

  it("leaves other trailing punctuation out of it", () => {
    expect(addresses("https://example.org, and")).toEqual([
      "https://example.org",
    ]);
    expect(addresses("really? https://example.org!")).toEqual([
      "https://example.org",
    ]);
  });

  it("gives back a closing bracket the address opened", () => {
    // Wikipedia writes them, and stopping at the first one is a broken link.
    expect(addresses("https://example.org/wiki/Salt_(chemistry)")).toEqual([
      "https://example.org/wiki/Salt_(chemistry)",
    ]);
  });

  it("keeps a closing bracket that belongs to the sentence", () => {
    expect(addresses("(see https://example.org)")).toEqual([
      "https://example.org",
    ]);
  });

  it("links nothing but the web", () => {
    // The two schemes a message body writes bare. `mailto:` is written as an
    // anchor by the clients that send one, so there is nothing to find here,
    // and everything else is refused in Rust anyway.
    expect(addresses("ftp://example.org file:///etc/passwd")).toEqual([]);
  });

  it("does not guess a link out of a domain name", () => {
    // Half the sentences in a technical room name a host. Turning each into a
    // link means a message full of blue words nobody can read past.
    expect(addresses("example.org is the homeserver")).toEqual([]);
  });

  it("refuses a scheme with nothing after it", () => {
    expect(addresses("https://.")).toEqual([]);
  });
});
