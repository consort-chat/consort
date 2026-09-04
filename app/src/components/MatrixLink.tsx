import { linkLabel, type PlaceTarget } from "../lib/matrixTo";
import { useRoomLinks } from "../lib/roomLinks";

/** A hash: this points at a room. */
function RoomIcon() {
  return (
    <svg
      className="body__pill-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M4 9h16" />
      <path d="M4 15h16" />
      <path d="M10 3 8 21" />
      <path d="M16 3l-2 18" />
    </svg>
  );
}

/** A speech bubble: this points at one thing somebody said. */
function SaidIcon() {
  return (
    <svg
      className="body__pill-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 11.5a8.4 8.4 0 0 1-9 8.4 8.5 8.5 0 0 1-3.9-.9L3 21l1.9-5.1A8.4 8.4 0 0 1 4 11.5a8.4 8.4 0 0 1 8.5-8.4h.5a8.4 8.4 0 0 1 8 8z" />
    </svg>
  );
}

/**
 * A link into Matrix, drawn as a badge rather than as an address.
 *
 * The address itself is unreadable, which is the whole reason this exists: a
 * link to a message is sixty characters of room ID and event ID, and a room
 * full of them is a room nobody can read. What is drawn instead says where it
 * goes, and pressing it goes there rather than opening a redirect page in a
 * browser.
 *
 * `label` is what the sender wrote, when they wrote something other than the
 * address. A markdown link with words in it means those words, and replacing
 * them with a generated phrase would throw away the only part somebody chose.
 *
 * A button rather than an anchor. There is no address to put on one: the
 * destination is a room in this window, not a page.
 */
export function MatrixLink({
  target,
  label,
}: {
  target: PlaceTarget;
  label?: string;
}) {
  const links = useRoomLinks();
  const drawn = label ?? linkLabel(target, links.nameOf(target.roomOrAlias));

  return (
    <button
      type="button"
      className="body__pill"
      onClick={(event) => {
        // A message that has a thread opens it when the words are clicked, and
        // this is inside those words. Without this, one press both changes
        // room and opens a panel about the room being left.
        event.stopPropagation();
        links.open(target);
      }}
    >
      {target.kind === "room" ? <RoomIcon /> : <SaidIcon />}
      {drawn}
    </button>
  );
}
