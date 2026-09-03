/**
 * Finding the addresses in a message somebody typed.
 *
 * Most clients send a pasted link as plain text. Linkifying is something they
 * do when drawing a message rather than when sending one, so `body` holds the
 * address and there is no `formatted_body` at all. Without this, a link
 * arrives as words with nothing to press.
 *
 * Nothing here decides whether an address may be opened. That is
 * `checked_link` in Rust, and the two are deliberately separate: this one says
 * what looks like a link, that one says what may be acted on, and neither
 * should be trusted to have done the other's job.
 */

/** One run of a message body: words, or an address to go to. */
export interface Piece {
  text: string;
  /** Where it goes, on the pieces that are links. */
  href?: string;
}

/**
 * What an address looks like in running text.
 *
 * The scheme is required, and that is the whole of why a domain name in a
 * sentence is left alone. Half the sentences in a technical room name a host,
 * and a message where every one of them is blue and underlined is a message
 * nobody can read.
 */
const ADDRESS = /https?:\/\/[^\s<]+/gi;

/** Punctuation that ends the sentence rather than the address. */
const TRAILING = /[.,;:!?'"]+$/;

/** How many times one character appears. */
function count(text: string, character: string): number {
  let total = 0;
  for (const one of text) if (one === character) total += 1;
  return total;
}

/**
 * The address without the sentence that follows it.
 *
 * A closing bracket is the awkward one, because it goes both ways: Wikipedia
 * writes them into paths, and people write links inside parentheses. Counting
 * decides it. One the address opened stays, one it did not is the sentence's.
 *
 * A loop rather than one pass, because `(https://example.org).` ends with
 * both kinds and stripping either uncovers the other.
 */
function trimmed(found: string): string {
  let address = found;
  for (;;) {
    const shorter = address.replace(TRAILING, "");
    if (shorter !== address) {
      address = shorter;
      continue;
    }
    if (address.endsWith(")") && count(address, ")") > count(address, "(")) {
      address = address.slice(0, -1);
      continue;
    }
    return address;
  }
}

/** Whether a scheme is followed by somewhere to go. */
function somewhere(address: string): boolean {
  return /^https?:\/\/[^/?#]/i.test(address);
}

/**
 * A run of text, split into what it says and where it points.
 *
 * An address that trims down to a bare scheme is left in the surrounding
 * words, because that is what it is.
 */
export function linkify(text: string): Piece[] {
  const pieces: Piece[] = [];
  let at = 0;

  for (const found of text.matchAll(ADDRESS)) {
    const address = trimmed(found[0]);
    if (!somewhere(address)) continue;

    if (found.index > at) pieces.push({ text: text.slice(at, found.index) });
    pieces.push({ text: address, href: address });
    at = found.index + address.length;
  }

  if (at < text.length) pieces.push({ text: text.slice(at) });
  return pieces;
}
