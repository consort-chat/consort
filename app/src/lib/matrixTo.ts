/**
 * Reading a `matrix.to` address.
 *
 * It is how every client writes a link to something inside Matrix: a person, a
 * room, or one message in one room. The interesting part is that it is not a
 * website. `matrix.to` serves a page that offers to open a client, so following
 * one in a browser is a detour through a redirect page to arrive back where the
 * reader already was. What a client is supposed to do with one is go there.
 *
 * Nothing here decides whether an address may be acted on, in the way
 * `checked_link` in Rust decides that for the web. It does not have to: an
 * address that parses to one of these names something inside this account's own
 * homeserver, and the worst it can do is name something that is not there.
 */

import { linkify } from "./links";

/** Somebody, drawn as a name rather than as a destination. */
export interface PersonTarget {
  kind: "person";
  userId: string;
}

/** A room, by ID or by alias. Which of the two is the sigil. */
export interface RoomTarget {
  kind: "room";
  roomOrAlias: string;
}

/** One message in one room. */
export interface MessageTarget {
  kind: "message";
  roomOrAlias: string;
  eventId: string;
}

/** Where a `matrix.to` address points. */
export type MatrixTarget = PersonTarget | RoomTarget | MessageTarget;

/**
 * The two that name somewhere to go.
 *
 * A person is the odd one out: pressing a name opens nothing, because the
 * destination would be a matrix.to page rather than anything in Consort.
 */
export type PlaceTarget = RoomTarget | MessageTarget;

/** One percent-encoded part, decoded, or as it was when it will not decode. */
function decoded(part: string): string {
  try {
    return decodeURIComponent(part);
  } catch {
    // A stray `%` in an address a stranger wrote. The raw text is still the
    // best guess at what was meant, and throwing here would take the whole
    // message down with it.
    return part;
  }
}

/**
 * Where an address points, or nothing when it points nowhere in Matrix.
 *
 * Everything a `matrix.to` link carries is in the fragment, which is what makes
 * the format private to the client: a fragment never reaches a server, so the
 * site cannot know what anybody looked at. That is also why this reads
 * `address.hash` rather than the path.
 *
 * The sigils are what say which kind of thing was named, and they are the whole
 * of the grammar: `@` is somebody, `!` and `#` are a room, `$` is a message.
 * Anything else is an address this build has no idea about, and a link that
 * opens a browser is a better answer than a control that does nothing.
 */
export function matrixTarget(
  raw: string | null | undefined,
): MatrixTarget | undefined {
  if (raw === null || raw === undefined) return undefined;

  let address: URL;
  try {
    address = new URL(raw);
  } catch {
    return undefined;
  }
  if (address.hostname !== "matrix.to") return undefined;
  if (!address.hash.startsWith("#/")) return undefined;

  // The `?via=` on the end names servers likely to know the room, which is for
  // joining one this account is not in. Consort does not join from a link.
  const [path = ""] = address.hash.slice(2).split("?");
  const parts = path
    .split("/")
    .map(decoded)
    .filter((part) => part !== "");

  const [first, second] = parts;
  if (first === undefined) return undefined;

  if (first.startsWith("@")) return { kind: "person", userId: first };
  if (!first.startsWith("!") && !first.startsWith("#")) return undefined;

  if (second !== undefined && second.startsWith("$")) {
    return { kind: "message", roomOrAlias: first, eventId: second };
  }
  return { kind: "room", roomOrAlias: first };
}

/**
 * An alias as something to read, or nothing for a room ID.
 *
 * An alias is a name somebody chose and it says which room it is. A room ID is
 * eighteen random characters and says nothing at all, so it is better left out
 * of a sentence than put in one.
 */
function readable(roomOrAlias: string): string | null {
  return roomOrAlias.startsWith("#") ? roomOrAlias : null;
}

/**
 * What to write on a link, given what this account calls the room.
 *
 * `roomName` is what the channel list knows, which is nothing for a room this
 * account is not in and nothing for an alias nobody has resolved yet. Both fall
 * back to something true rather than to the address: a badge reading
 * `!nBcXyZ:example.org` is the raw link the badge exists to replace.
 */
export function linkLabel(
  target: PlaceTarget,
  roomName: string | null,
): string {
  const where = roomName ?? readable(target.roomOrAlias);
  if (target.kind === "room") return where ?? "A room";
  return where === null ? "A message" : `Message in ${where}`;
}

/**
 * One message body as a line of words, with its addresses named.
 *
 * For the places that quote a message rather than drawing it: the row above a
 * reply, and the line above the composer saying what is about to be answered.
 * Both are one line of plain text, so neither can hold the badge the message
 * itself draws, and both were showing sixty characters of room ID and event ID
 * where the message showed "Message in #voice".
 *
 * Plain text rather than elements, and that is a constraint rather than a
 * preference: the reply row is a button, and a badge is a button too.
 */
export function withAddressesNamed(
  text: string,
  nameOf: (roomOrAlias: string) => string | null,
): string {
  return linkify(text)
    .map((piece) => {
      if (piece.href === undefined) return piece.text;

      const target = matrixTarget(piece.href);
      if (target === undefined || target.kind === "person") return piece.text;
      return linkLabel(target, nameOf(target.roomOrAlias));
    })
    .join("");
}
