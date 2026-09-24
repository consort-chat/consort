import { useSyncExternalStore } from "react";

import { readersOf, subscribeToReaders } from "../lib/readers";
import { RoomAvatar } from "./RoomAvatar";
import "./ReadBy.css";

/**
 * What the row says it is.
 *
 * Exported for the tests, on the same terms as the rules in `MessageGroups`:
 * the sentence is the part worth pinning and the markup is not. It is also the
 * whole of this file's answer to the privacy question below, so it is worth
 * being able to read it back without a room.
 *
 * The second half is said whether or not anybody is missing, and that is the
 * point rather than laziness. See the component.
 */
export function readBySays(names: string[], more: number): string {
  const seen = more > 0 ? [...names, `${more} more`] : names;
  const last = seen.at(-1) ?? "";
  const rest = seen.slice(0, -1);
  const listed = rest.length === 0 ? last : `${rest.join(", ")} and ${last}`;

  return `Read by ${listed}. Only people who share read receipts are shown.`;
}

/**
 * Who has read up to one message, as small faces beside it.
 *
 * ## It subscribes for itself, and that is the design
 *
 * Nothing hands this component the receipts. It reads `lib/readers` through
 * `useSyncExternalStore`, which means a receipt arriving re-renders the one or
 * two rows that moved and leaves the conversation alone. Handed the same
 * answer as a prop it would cost 5.3ms of redraw per receipt against 0.14ms,
 * and a busy room produces about as many receipts as messages. The measurement
 * is in `lib/readers`.
 *
 * ## What it says about the people it cannot see
 *
 * A person sending `m.read.private` is visible to nobody. They never appear in
 * anybody's row, and turning your own receipts off does not stop you seeing
 * other people, so a row is regularly fewer faces than there are people in the
 * room and nothing about it says why. That is indistinguishable from the
 * feature being broken, which is the bug report this sentence exists to
 * prevent.
 *
 * It is said always, in every room, whether or not anybody here has turned
 * theirs off. Saying it only when somebody was missing would be the
 * disclosure: a sentence that appeared the moment a particular colleague went
 * quiet tells you the thing they chose not to share.
 *
 * For the same reason the row never says how many people it cannot see. "Three
 * of twelve have read this" is exact disclosure in a room of two and a
 * narrowing one in a room of ten. The overflow count is a different claim and
 * is safe: it counts people whose receipts this account can see and the row
 * had no space for.
 */
export function ReadBy({
  roomId,
  threadRoot,
  eventId,
  names,
}: {
  roomId: string;
  /**
   * The thread this row is inside, when it is inside one.
   *
   * Absent in the room's own timeline. A thread keeps receipts of its own and
   * a receipt in the room does not answer for it, so a panel that read the
   * room's answer would show the room's readers against every reply.
   */
  threadRoot?: string | undefined;
  /** The message the row belongs to. */
  eventId: string;
  /** Display names by user ID, for whoever the room has told us about. */
  names: Record<string, string>;
}) {
  const read = useSyncExternalStore(
    subscribeToReaders,
    () => readersOf(roomId, threadRoot, eventId),
    () => undefined,
  );

  if (read === undefined || read.readers.length === 0) return null;

  const who = read.readers.map((id) => names[id] ?? id);
  const more = read.more ?? 0;

  return (
    <div className="read-by" role="group" aria-label={readBySays(who, more)}>
      {read.readers.map((id, at) => (
        /*
          The name is on the pointer and in the group's label above, so the
          face itself is not a second announcement of it. A screen reader gets
          one sentence naming everybody rather than one word per picture.
        */
        <span
          key={id}
          className="read-by__face"
          title={who[at]}
          aria-hidden="true"
        >
          <RoomAvatar
            roomId={roomId}
            userId={id}
            name={who[at] ?? id}
            className="read-by__avatar"
          />
        </span>
      ))}
      {more > 0 && (
        <span className="read-by__more" aria-hidden="true">
          +{more}
        </span>
      )}
    </div>
  );
}
