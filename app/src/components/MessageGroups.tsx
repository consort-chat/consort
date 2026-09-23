import { Fragment, useMemo, useState, type RefObject } from "react";

import type { Message, MessageKind, Participant, SystemMessage } from "../lib/api";
import { flashMessage } from "../lib/flash";
import { withAddressesNamed } from "../lib/matrixTo";
import { useRoomLinks } from "../lib/roomLinks";
import { FormattedBody } from "./FormattedBody";
import { PlainBody } from "./PlainBody";
import { MessageMedia } from "./MessageMedia";
import { PresenceDot } from "./PresenceDot";
import { ConfirmDelete } from "./ConfirmDelete";
import { ReactionPicker } from "./ReactionPicker";
import { RoomAvatar } from "./RoomAvatar";

/**
 * How long a gap before two messages from the same person stop being one
 * group, in milliseconds.
 *
 * Five minutes. Somebody typing three sentences in a row is one person
 * talking; the same person answering an hour later is a new thing to read, and
 * repeating their name is how a reader is told which.
 */
const SAME_BREATH = 5 * 60 * 1000;

/**
 * Which control a picker was opened from.
 *
 * A message with reactions has two of them, and the panel is drawn under
 * whichever was pressed rather than always in the corner.
 */
type Anchor = "toolbar" | "row";

/** Consecutive messages from one person, close enough together to read as one. */
export interface Group {
  /** The first message's ID, which is what makes this row's key stable. */
  id: string;
  sender: string;
  at: number;
  messages: Message[];
}

/**
 * The first message that arrived after `readUpTo`, if any is loaded.
 *
 * What the "new messages" line is drawn above. Undefined when nothing has been
 * read here, when the marker names the newest message loaded, and when it names
 * something outside the window: in all three there is nothing new to point at,
 * and a line drawn anyway would either sit above the whole room or below the
 * end of it.
 *
 * Exported for the tests, on the same terms as [`group`] below: the rule is
 * the part worth pinning and the markup is not.
 */
export function firstUnread(
  messages: Message[],
  readUpTo: string | undefined,
): string | undefined {
  if (readUpTo === undefined) return undefined;

  const at = messages.findIndex((message) => message.id === readUpTo);
  if (at === -1) return undefined;

  return messages[at + 1]?.id;
}

/**
 * The messages that open a local calendar day, by ID.
 *
 * What a date separator is drawn above. The comparison is between calendar
 * days rather than elapsed time, because a conversation running from 20:00 to
 * 04:00 crosses one day and never crosses twenty-four hours, and a rule
 * measuring milliseconds would put the line in neither of the places a reader
 * expects.
 *
 * The first message loaded is always in here. The day it belongs to has to be
 * said, and the consequence of saying it is that paging older history in may
 * take the line away again when the message above turns out to be the same
 * day. That is the right answer recomputing, and the timeline anchors its
 * scroll from the bottom, so nothing jumps.
 *
 * Its own rule rather than day awareness inside [`group`] below, which has one
 * job and tests that pin it. Exported on the same terms as that one: the rule
 * is the part worth pinning and the markup is not.
 */
export function firstOfEachDay(messages: Message[]): ReadonlySet<string> {
  const opens = new Set<string>();
  let previous: string | undefined;

  for (const message of messages) {
    const day = new Date(message.at).toDateString();
    if (day !== previous) opens.add(message.id);
    previous = day;
  }

  return opens;
}

/** A day, in milliseconds, for counting calendar days apart. */
const DAY = 24 * 60 * 60 * 1000;

/** Midnight at the start of the local day this moment falls in. */
function dayStart(at: number): number {
  const midnight = new Date(at);
  midnight.setHours(0, 0, 0, 0);
  return midnight.getTime();
}

/**
 * What a date separator says.
 *
 * `now` is a parameter rather than a call inside, so a test can pin it without
 * faking the clock for everything else in the file.
 *
 * A room left open across midnight keeps saying "Today" over yesterday until
 * something makes it render again, which in a live room is the next message.
 * Accepted rather than fixed: a timer whose only job is to relabel one line at
 * midnight is more machinery than the defect is worth.
 *
 * Rounded rather than truncated, because two of the days in a year are 23 and
 * 25 hours long and a division would put one of them a day out.
 */
export function dayLabel(at: number, now = Date.now()): string {
  const back = Math.round((dayStart(now) - dayStart(at)) / DAY);

  if (back === 0) return "Today";
  if (back === 1) return "Yesterday";

  const day = new Date(at);
  // Inside the last week a weekday places it on its own, and the date proper
  // would be more words for the same fact.
  if (back > 1 && back < 7) {
    return day.toLocaleDateString(undefined, { weekday: "long" });
  }

  return day.toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
    // Dropped for the year being lived in, which is the year a reader assumes.
    ...(day.getFullYear() === new Date(now).getFullYear()
      ? {}
      : { year: "numeric" }),
  });
}

/**
 * Collapse consecutive messages from one person into groups.
 *
 * Exported for the tests, because the rule is the only thing here worth
 * pinning: everything else is markup, and a test that asserted the markup
 * would fail on every change to the design without ever noticing a wrong
 * grouping.
 */
export function group(messages: Message[]): Group[] {
  const groups: Group[] = [];

  for (const message of messages) {
    const last = groups.at(-1);
    const sameBreath =
      last !== undefined &&
      last.sender === message.sender &&
      // Against the last message rather than the group's first, or somebody
      // talking steadily for ten minutes is split in the middle.
      message.at - (last.messages.at(-1)?.at ?? last.at) < SAME_BREATH;

    if (sameBreath) last.messages.push(message);
    else
      groups.push({
        id: message.id,
        sender: message.sender,
        at: message.at,
        messages: [message],
      });
  }

  return groups;
}

/** The clock time to draw beside a group. */
export function timeOf(at: number): string {
  return new Date(at).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** The whole date, for the tooltip a clock time is not enough for. */
function dateOf(at: number): string {
  return new Date(at).toLocaleString();
}

/**
 * Which of the four attachment kinds a message is.
 *
 * A message carrying `media` is always one of them, and the fallback is a
 * picture because that is the one whose failure is visible: a card drawn where
 * a photograph should be is obvious, and a photograph drawn where a card
 * should be is a broken image.
 */
function attachmentKind(
  kind: MessageKind,
): "image" | "video" | "file" | "audio" {
  return kind === "video" || kind === "file" || kind === "audio"
    ? kind
    : "image";
}

/**
 * A turning arrow: the message being answered, or the control that answers one.
 *
 * It replaces the words "In reply to", which were not ours: they are the
 * fallback the sender writes for clients that draw no reply of their own, and
 * they arrived as a link that went nowhere.
 *
 * Exported because the line above a composer draws the same arrow in front of
 * what is about to be answered, and one glyph drawn twice beats two that have
 * to be kept looking alike.
 */
export function ReplyIcon({ className }: { className: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M9 17 4 12l5-5" />
      <path d="M20 18v-2a4 4 0 0 0-4-4H4" />
    </svg>
  );
}

/**
 * One line standing in for a message somebody answered.
 *
 * The filename for an attachment nobody captioned, because the alternative is
 * an empty quote, which reads as a message that failed to load rather than as
 * a picture.
 *
 * `nameOf` is what turns an address in the body into the words the message
 * itself draws on its badge. Without it a reply to "look at <permalink>" quotes
 * sixty characters of room ID, which is both unreadable and not what the line
 * above it says.
 */
export function previewOf(
  message: Message,
  nameOf: (roomOrAlias: string) => string | null,
): string {
  /*
    What the formatting says, when there is any, because `body` is the
    markdown somebody typed rather than the sentence they wrote, and a quote
    reading `**ship it**` is one nobody sent.

    The body is what is left for a message whose whole content is a picture:
    a custom emoji is an `img` with no words in it, and the shortcode in the
    body is what a person would read out.
  */
  // Before the body, which a deleted message has none of. Without this the
  // fallback below reads it as an attachment and the row above the reply says
  // somebody sent a file.
  if (message.kind === "deleted") return "Message deleted";
  const said = wordsOf(message.html) || message.body;
  if (said !== "") return withAddressesNamed(said, nameOf);
  return message.media?.name ?? "an attachment";
}

/**
 * A `formatted_body` with the markup gone, or nothing.
 *
 * Inert, on the same terms as `FormattedBody`: the document `DOMParser`
 * returns is never attached to the page, runs no scripts and fetches no
 * images, and what is taken out of it here is text rather than elements. See
 * that file for why parsing a stranger's HTML this way is the safe move
 * rather than the dangerous one.
 */
function wordsOf(html: string | undefined): string {
  if (html === undefined) return "";
  const parsed = new DOMParser().parseFromString(html, "text/html");
  // Collapsed, because the quote is one line and a paragraph break arrives
  // here as a newline that would otherwise sit in the middle of it.
  return (parsed.body.textContent ?? "").replace(/\s+/g, " ").trim();
}

/**
 * Two overlapping bubbles: a conversation hanging off a message.
 *
 * Deliberately not the reply arrow above. A reply answers a message in the
 * room, and a thread takes the answer somewhere else; drawing both with one
 * glyph would make the two controls look like one control drawn twice.
 */
/**
 * A face with a plus: react to this.
 *
 * The plus is the whole message. A bare smiley is what a reaction already
 * drawn looks like, and the control that adds one has to be distinguishable
 * from the ones that are already there.
 */
function ReactIcon() {
  return (
    <svg
      className="timeline__action-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M20.9 12.6a9 9 0 1 1-9.5-9.5" />
      <path d="M9 9h.01" />
      <path d="M15 9h.01" />
      <path d="M8.5 14.5a4 4 0 0 0 6 .3" />
      <path d="M19 2v5" />
      <path d="M16.5 4.5h5" />
    </svg>
  );
}

/** A chain: an address for this message, to give to somebody else. */
function LinkIcon() {
  return (
    <svg
      className="timeline__action-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M9 17H7A5 5 0 0 1 7 7h2" />
      <path d="M15 7h2a5 5 0 0 1 0 10h-2" />
      <path d="M8 12h8" />
    </svg>
  );
}

/** A tick: the address is on the clipboard. */
function CopiedIcon() {
  return (
    <svg
      className="timeline__action-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="m5 13 4 4L19 7" />
    </svg>
  );
}

/** A pencil: change what this says. */
function EditIcon() {
  return (
    <svg
      className="timeline__action-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M17 3a2.8 2.8 0 0 1 4 4L7.5 20.5 2 22l1.5-5.5z" />
      <path d="m15 5 4 4" />
    </svg>
  );
}

function ThreadIcon() {
  return (
    <svg
      className="timeline__action-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M14 9a2 2 0 0 1-2 2H6l-4 4V4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2z" />
      <path d="M18 9h2a2 2 0 0 1 2 2v11l-4-4h-6a2 2 0 0 1-2-2v-1" />
    </svg>
  );
}

/**
 * Where this account stopped reading.
 *
 * A rule with a word on it rather than a bare line, because a bare line in a
 * conversation is a divider and reads as one: somebody scrolling past it would
 * take it for the start of a day or a gap in the history. The words are what
 * make it a place.
 *
 * Not focusable and not a control. Nothing happens when it is pressed, and the
 * room scrolls to it on its own when it opens.
 */
function NewMessagesLine() {
  return (
    <p className="timeline__unread" role="separator" data-unread-line="true">
      <span className="timeline__unread-said">New messages</span>
    </p>
  );
}

/**
 * The day the messages under it were said.
 *
 * The same rule-and-words treatment as the line above, because it answers the
 * same kind of question about a place in the conversation. What differs is
 * that the rule runs on both sides with the words centred: a date is a heading
 * over the day below it, where "New messages" is a label on the conversation
 * that follows.
 */
function DaySeparator({ label }: { label: string }) {
  return (
    <p className="timeline__day" role="separator" data-day-line="true">
      <span className="timeline__day-said">{label}</span>
    </p>
  );
}

/**
 * What a [`SystemMessage`] says, in words.
 *
 * `actor` and `subject` are already resolved to display names, or left as
 * Matrix IDs when the room has not told us a name: see the caller, which
 * reads both out of the same `names` lookup the bylines use.
 *
 * Exported for the tests, on the same terms as [`group`] above: the sentence
 * is the part worth pinning and the markup around it is not.
 */
export function systemMessageText(
  kind: SystemMessage["kind"],
  actor: string,
  subject: string,
): string {
  switch (kind) {
    case "joined":
      return `${subject} joined the room`;
    case "invited":
      return `${actor} invited ${subject}`;
    case "left":
      return `${subject} left the room`;
    case "kicked":
      return `${actor} removed ${subject} from the room`;
    case "banned":
      return `${actor} banned ${subject}`;
  }
}

/**
 * One membership change, drawn as a quiet line rather than as a message.
 *
 * No byline, no bubble, no actions: a join or a leave is not something
 * anybody said, and drawing it as though it were would put "Add a reaction"
 * under a sentence Consort wrote about the room rather than a person wrote
 * in it.
 */
function SystemMessageLine({
  message,
  names,
}: {
  message: SystemMessage;
  names: Record<string, string>;
}) {
  const actor = names[message.actor] ?? message.actor;
  const subject = names[message.subject] ?? message.subject;

  return (
    <p className="timeline__system" role="note">
      {systemMessageText(message.kind, actor, subject)}
    </p>
  );
}

/** One row of the timeline: a run of messages, or one membership change. */
type Row =
  | { kind: "group"; at: number; group: Group }
  | { kind: "system"; at: number; message: SystemMessage };

/**
 * Groups and membership changes, in the order they are drawn in.
 *
 * Merged by `at` rather than by the room's own event order, which neither
 * side carries any more once split into two lists: see [`SystemMessage.at`]
 * in `lib/api.ts` for what that costs. `Array.prototype.sort` is stable, so
 * two rows landing on the same millisecond keep the order they were built
 * in, which is groups before the system lines beside them. An arbitrary
 * choice between two things that are, at that resolution, simultaneous.
 *
 * A membership change can only land between two groups, never inside one:
 * a group's `at` is its first message's, fixed once the group exists, so a
 * join in the middle of somebody's burst of messages sorts to one side of
 * the whole burst rather than the exact message it fell between. Accepted
 * rather than fixed, on the same terms as the approximation above: a
 * membership change inside somebody else's burst of messages is rare, and
 * splitting a group to seat one correctly would be a second sorting rule
 * for a case this uncommon.
 */
function merged(groups: Group[], system: SystemMessage[]): Row[] {
  const rows: Row[] = [
    ...groups.map((group): Row => ({ kind: "group", at: group.at, group })),
    ...system.map(
      (message): Row => ({ kind: "system", at: message.at, message }),
    ),
  ];
  rows.sort((left, right) => left.at - right.at);
  return rows;
}

/**
 * Whether the reader could correct this message, if a caller offers the way.
 *
 * Their own, and one of the three kinds an edit can replace. The list is not a
 * subset of what the interface can draw: it is exactly what `facts::replacement`
 * in Rust reads back out, so a control never appears where pressing it would
 * send a correction no client would fold.
 *
 * An attachment is the case worth naming. An edit carries replacement text,
 * and the SDK would happily build one that swaps a picture for a sentence; the
 * caption is the thing somebody would mean to change, and there is no surface
 * for that yet.
 */
export function correctable(message: Message, selfId: string): boolean {
  return (
    message.sender === selfId &&
    (message.kind === "text" ||
      message.kind === "emote" ||
      message.kind === "notice")
  );
}

/**
 * Whether the reader could delete this message, if a caller offers the way.
 *
 * Their own, and that is the whole of it. Deliberately not [`correctable`],
 * which also asks what kind the message is: an edit carries replacement text
 * and there are three kinds it can replace, where a redaction removes an event
 * whatever was inside it. An attachment nobody meant to send is what the
 * narrower rule would miss, and it is the case people want this for most.
 *
 * Somebody else's is not offered here at all. A moderator removing one is a
 * power level read, a different question, and a different sentence to put in
 * front of whoever is about to do it.
 *
 * Says nothing about a message already deleted, which has no action row at
 * all: there is nothing left to answer, correct or remove on one, and the row
 * is skipped wholesale rather than each control in it refusing separately.
 */
function deletable(message: Message, selfId: string): boolean {
  return message.sender === selfId;
}

/**
 * The mark on a message its author has since corrected.
 *
 * Inside the body and at the end of the words rather than on a line of its
 * own. A row would space the conversation out by a line on every message
 * anybody has ever fixed a typo in, and what it says is a footnote to the
 * sentence rather than a statement beside it.
 *
 * What it said before is deliberately not offered. Nothing here holds it: the
 * superseded text never crosses the IPC, because a client that kept a copy of
 * every sentence somebody took back is a client people would stop correcting
 * themselves in.
 */
function EditedMark() {
  return (
    <span className="timeline__edited" title="This message was edited">
      (edited)
    </span>
  );
}

/**
 * A wastebasket, for the control that deletes a message.
 */
function TrashIcon() {
  return (
    <svg
      className="timeline__action-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M3 6h18" />
      <path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2" />
      <path d="M19 6v14a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
    </svg>
  );
}

/**
 * The mark left where a message was.
 *
 * Drawn rather than closing the conversation over the gap, which is the same
 * argument the undecryptable and unsupported kinds make and the one with the
 * most behind it: a reply sitting under a message that vanished answers
 * nothing and reads as a non-sequitur, and every other client in the room
 * draws a mark for the same redaction.
 *
 * "Message deleted" and not "deleted by" whoever wrote it, because the words
 * have to hold for both stories this can be. Only a redaction sent by somebody
 * other than the author names anybody, which is the one case where saying
 * nothing would leave the mark under a name that did not do it.
 *
 * What it said before is not here and is nowhere on this side. The homeserver
 * emptied the event, and the interface has no copy to draw even if it wanted
 * one.
 */
function DeletedBody({ by }: { by: string | null }) {
  return (
    <p className="timeline__deleted">
      <TrashIcon />
      <span>{by === null ? "Message deleted" : `Message deleted by ${by}`}</span>
    </p>
  );
}

/**
 * A run of grouped messages, drawn.
 *
 * Its own component because a thread panel draws the same thing beside the
 * room it came out of. Everything about the conversation is passed in: it
 * keeps no copy of the messages, asks for nothing, and is the same markup in
 * both places by construction rather than by two files being kept in step.
 *
 * The one thing it does hold is which message has its reaction picker open.
 * That is chrome rather than content: nothing outside can act on it, and one
 * at a time is a rule about this component's own drawing.
 */
export function MessageGroups({
  groups,
  system,
  names,
  roomId,
  selfId,
  known,
  container,
  openingId,
  copiedId,
  onAbout,
  onOpenThread,
  onReply,
  onEdit,
  onDelete,
  onReact,
  onCopyLink,
  onGoTo,
  newFrom,
  newDay,
}: {
  groups: Group[];
  /**
   * Membership changes to draw alongside `groups`, merged in by when they
   * happened.
   *
   * Absent inside the thread panel, which has no membership changes of its
   * own to show: a join or a leave is a room-level event, not a reply.
   */
  system?: SystemMessage[];
  /** Display names by user ID, for whoever the room has told us about. */
  names: Record<string, string>;
  roomId: string;
  /**
   * Whoever is signed in, so a message naming them can be marked.
   *
   * The comparison is here rather than in Rust because it is a question about
   * the reader rather than about the message, and the message is the same one
   * for everybody in the room.
   */
  selfId: string;
  /**
   * The messages a reply may point at, by event ID.
   *
   * Passed in rather than worked out from `groups`, because the thread panel
   * draws its root and its replies as two of these and a reply answering the
   * root has to be able to find it.
   *
   * Not every one of these is drawn. A room shows a window of history and a
   * reply can name anything older than it, and the room looks those up so the
   * row can say who wrote it and what it said; pressing one of those goes to
   * it through `onGoTo` rather than scrolling.
   *
   * A reply pointing at something not in here at all still draws a row, saying
   * so. That is a message the homeserver would not hand over, which a
   * redaction and a missing key both look like.
   */
  known?: ReadonlyMap<string, Message>;
  /**
   * Where to look for the message a reply names, when one is pressed.
   *
   * The scrolling box, so the search is scoped: the thread panel draws the
   * same component beside the room, and a root message is in both.
   */
  container?: RefObject<HTMLElement | null>;
  /**
   * The thread that has been asked for and has not arrived, if any.
   *
   * Its control stops taking presses and turns instead. Opening one is a
   * message to Rust that answers before the panel exists, so without this a
   * press looks like nothing happened and invites another.
   */
  openingId?: string | null;
  /**
   * The message whose address has just gone to the clipboard, if any.
   *
   * Held by the caller rather than here, on the same terms as `openingId`: a
   * copy is a command that can fail, and only the caller can tell a clipboard
   * that took the text from one that would not.
   */
  copiedId?: string | null;
  /** Open somebody's card, at the point that was clicked. */
  onAbout: (person: Participant, at: { x: number; y: number }) => void;
  /**
   * Open the thread hanging from a message.
   *
   * Absent inside a thread panel, where every message is already in the thread
   * being read and there is nowhere further to go.
   */
  onOpenThread?: (rootId: string) => void;
  /**
   * Answer a message in the conversation it is in.
   *
   * The whole message rather than its ID, because the composer draws a line of
   * what is being answered and the reply itself has to name who wrote it.
   *
   * The thread panel passes this too. The box at the bottom of it answers the
   * thread, which is not the same as answering one line of a long one: a
   * thread reply that names nobody carries an `m.in_reply_to` that is falling
   * back, and no client draws a quoted row for that.
   */
  onReply?: (message: Message) => void;
  /**
   * Correct a message this account sent.
   *
   * The whole message rather than its ID, because the composer opens on what
   * it currently says and an ID alone cannot fill the box.
   *
   * The control is drawn only where [`correctable`] says it can be, which is
   * the reader's own plain-text messages.
   */
  onEdit?: (message: Message) => void;
  /**
   * Delete a message this account sent.
   *
   * Called only once the question hanging off the control has been answered,
   * so a caller may send the redaction on being called rather than asking
   * again. The whole message rather than its ID, on the same terms as the two
   * above, and because a caller that wants to say what it removed has it.
   *
   * The control is drawn only where [`deletable`] says it can be, which is the
   * reader's own messages of every kind.
   */
  onDelete?: (message: Message) => void;
  /**
   * React to a message, or take a reaction back.
   *
   * `mine` is this session's own annotation on that key, when there is one, so
   * the caller can tell an addition from a removal without looking the message
   * up again. One callback rather than two, because a pill is one control that
   * does whichever of the two applies.
   *
   * Absent where reacting is not offered, which is nowhere yet: both the room
   * and the thread panel pass it.
   */
  onReact?: (eventId: string, key: string, mine: string | undefined) => void;
  /** Put one message's address on the clipboard. */
  onCopyLink?: (eventId: string) => void;
  /**
   * The first message that arrived after this account last read here.
   *
   * The "new messages" line is drawn above it. Absent inside a thread panel,
   * which has no marker of its own: `m.fully_read` is per room, and drawing
   * the room's line inside a thread would put it somewhere it does not
   * describe. See [`firstUnread`], which is where the caller gets this.
   */
  newFrom?: string | undefined;
  /**
   * The messages that open a day, which get a date separator above them.
   *
   * Absent inside a thread panel, and the default is no separators. A thread
   * is one conversation read as a unit and its root is regularly weeks older
   * than its replies, so a line between the root and every reply would be the
   * ordinary case rather than the exception. See [`firstOfEachDay`].
   */
  newDay?: ReadonlySet<string>;
  /**
   * Go to a message that is named by a reply but is not drawn.
   *
   * Only reached when the scroll above could not find it, which is a reply
   * naming something older than the window of history loaded. Absent in the
   * thread panel, where a message outside the thread is not somewhere the
   * panel can go.
   */
  onGoTo?: (eventId: string) => void;
}) {
  /*
    Which message has its picker open and which control opened it, or none.
    One at a time, for the reason a person's card has the same rule: two open
    at once is two panels of the same twelve keys with nothing saying which
    message either belongs to.
  */
  const [picking, setPicking] = useState<{ id: string; at: Anchor } | null>(
    null,
  );
  /*
    Which message has been asked about but not yet deleted, or none. One at a
    time for the reason the picker has that rule, and held here for the same
    one: it is chrome rather than content, nothing outside can act on it, and a
    caller learns about it only when the question has been answered.
  */
  const [confirming, setConfirming] = useState<string | null>(null);
  // For the quoted line above a reply, which cannot hold a badge and so says
  // what the badge would have said.
  const { nameOf } = useRoomLinks();
  const rows = useMemo(
    () => merged(groups, system ?? []),
    [groups, system],
  );

  /** Open the panel under this control, or shut the one already open there. */
  function toggle(id: string, at: Anchor) {
    setPicking((open) =>
      open?.id === id && open.at === at ? null : { id, at },
    );
  }

  /**
   * The picker, when this is the control it was opened from.
   *
   * One function for both sites rather than the same twenty lines twice. What
   * a key means when it is pressed does not depend on which control opened
   * the panel, and two copies would be two answers free to drift apart.
   */
  function pickerFor(message: Message, at: Anchor) {
    if (
      onReact === undefined ||
      picking?.id !== message.id ||
      picking.at !== at
    ) {
      return null;
    }

    return (
      <ReactionPicker
        align={at === "row" ? "left" : "right"}
        chosen={
          new Set(
            (message.reactions ?? [])
              .filter((one) => one.mine !== undefined)
              .map((one) => one.key),
          )
        }
        onChoose={(key) => {
          const already = message.reactions?.find((one) => one.key === key);
          onReact(message.id, key, already?.mine);
          setPicking(null);
        }}
        onClose={() => setPicking(null)}
      />
    );
  }

  /**
   * Open a thread, unless the press was the end of a selection.
   *
   * Dragging across a message to copy it finishes with a click, and opening a
   * panel on that would move the words out from under what was selected.
   */
  function opening(message: Message) {
    if (message.thread === undefined || onOpenThread === undefined) {
      return undefined;
    }
    return () => {
      if (window.getSelection()?.isCollapsed === false) return;
      onOpenThread(message.id);
    };
  }

  return (
    <>
      {rows.map((row) => {
        if (row.kind === "system") {
          return (
            <SystemMessageLine
              key={row.message.id}
              message={row.message}
              names={names}
            />
          );
        }

        const one = row.group;
        // Their display name if the room has told us one, and their user ID if
        // it has not. Whichever it is, it is what the byline draws, what the
        // group announces itself as, and what the card is about.
        const who = names[one.sender] ?? one.sender;
        const about = (event: { clientX: number; clientY: number }) =>
          onAbout(
            { id: one.sender, name: who },
            { x: event.clientX, y: event.clientY },
          );

        return (
          /*
            A fragment because a line falling here belongs above the group
            rather than inside it: drawn within the article it would sit under
            the byline, which reads as the person having said it.

            The date goes above the unread mark when both land on the same
            message. A day is the larger container, and a place in the
            conversation belongs inside the day it falls in.
          */
          <Fragment key={one.id}>
            {newDay?.has(one.id) && <DaySeparator label={dayLabel(one.at)} />}
            {one.messages[0]?.id === newFrom && <NewMessagesLine />}
            <article
              className="timeline__group"
              aria-label={`${who} at ${timeOf(one.at)}`}
            >
              {/*
                Two controls opening one card. The face is the larger target and
                the name is the one being read, and a hand goes for either.
              */}
              <button
                type="button"
                className="timeline__face-button"
                aria-haspopup="dialog"
                aria-label={`${who}'s picture`}
                onClick={about}
              >
                <RoomAvatar
                  roomId={roomId}
                  userId={one.sender}
                  name={who}
                  className="timeline__face"
                />
                <PresenceDot userId={one.sender} />
              </button>
              <div className="timeline__said">
                <p className="timeline__byline">
                  <button
                    type="button"
                    className="timeline__who"
                    aria-haspopup="dialog"
                    onClick={about}
                  >
                    {who}
                  </button>
                  {/*
                    The whole date lives here rather than on the words. A tooltip
                    that follows the pointer across every sentence in a room
                    appears over the one thing somebody is reading; the clock
                    time is already the thing being asked about.
                  */}
                  <time
                    className="timeline__at"
                    dateTime={new Date(one.at).toISOString()}
                    title={dateOf(one.at)}
                  >
                    {timeOf(one.at)}
                  </time>
                </p>
                {one.messages.map((message) => {
                  const open = opening(message);

                  const answered =
                    message.replyTo === undefined
                      ? undefined
                      : known?.get(message.replyTo);

                  return (
                    <Fragment key={message.id}>
                      {/*
                        The other half of both lines. A group is one person
                        talking without pause, so midnight and the place reading
                        stopped regularly fall between two messages inside one of
                        them rather than between two groups. Skipped for the
                        group's first message, which the fragment above has
                        already drawn them for.
                      */}
                      {newDay?.has(message.id) &&
                        message.id !== one.messages[0]?.id && (
                          <DaySeparator label={dayLabel(message.at)} />
                        )}
                      {message.id === newFrom &&
                        message.id !== one.messages[0]?.id && <NewMessagesLine />}
                      <div
                        className="timeline__message"
                        data-message-id={message.id}
                        {...(message.mentions?.includes(selfId)
                          ? { "data-mentions-me": "true" }
                          : {})}
                      >
                        {/*
                          The time, for everything the byline above does not
                          speak for. A group is one person talking without
                          pause, so six messages regularly carried one time on
                          the first of them and nothing on the other five, and
                          "when was that said" is a question about the message
                          rather than about the burst it arrived in.

                          Skipped on the group's own first message, which has
                          the byline's time directly above it. Drawn there as
                          well would be the same fact twice on two lines.

                          In the avatar's column rather than beside the words,
                          which is where Element puts it and where a reader can
                          run an eye down the times without reading the room.
                        */}
                        {message.id !== one.messages[0]?.id && (
                          <time
                            className="timeline__message-at"
                            dateTime={new Date(message.at).toISOString()}
                            title={dateOf(message.at)}
                          >
                            {timeOf(message.at)}
                          </time>
                        )}
                        {message.replyTo !== undefined &&
                          (answered === undefined ? (
                            <p className="timeline__reply timeline__reply--gone">
                              <ReplyIcon className="timeline__reply-glyph" />
                              <span className="timeline__reply-said">
                                Replying to a message that is not loaded.
                              </span>
                            </p>
                          ) : (
                            <button
                              type="button"
                              className="timeline__reply"
                              aria-label={`Go to ${names[answered.sender] ?? answered.sender}'s message`}
                              onClick={() => {
                                /*
                                  Drawn first, because a message on screen is
                                  already where somebody asked to be and asking
                                  the homeserver for it would throw away the
                                  conversation around it to arrive back at the
                                  same place. The fall-through is the reply that
                                  names something older than what is loaded.
                                */
                                if (
                                  !flashMessage(container?.current ?? null, answered.id)
                                ) {
                                  onGoTo?.(answered.id);
                                }
                              }}
                            >
                              <ReplyIcon className="timeline__reply-glyph" />
                              <span className="timeline__reply-who">
                                {names[answered.sender] ?? answered.sender}
                              </span>
                              <span className="timeline__reply-said">
                                {previewOf(answered, nameOf)}
                              </span>
                            </button>
                          ))}
                        {message.kind === "deleted" ? (
                          /*
                            The mark, in place of everything below. A deleted
                            message has no body, no attachment and no caption,
                            so neither branch under this has anything to draw.
                          */
                          <DeletedBody
                            by={
                              message.deletedBy === undefined ||
                              message.deletedBy === message.sender
                                ? null
                                : (names[message.deletedBy] ?? message.deletedBy)
                            }
                          />
                        ) : message.media !== undefined ? (
                          /*
                            The attachment, and under it whatever words were sent
                            with it. The filename is on the card rather than above
                            the picture: a line reading "screenshot.png" over the
                            screenshot is what somebody sent a picture to avoid. A
                            caption is a different thing and is drawn, which is how
                            a bot's quoted post survives the clip it came with.
                          */
                          <div className="timeline__attachment">
                            <MessageMedia
                              kind={attachmentKind(message.kind)}
                              media={message.media}
                            />
                            {message.body !== "" && (
                              <div
                                className="timeline__body"
                                data-selectable
                                onClick={open}
                              >
                                {message.html === undefined ? (
                                  <PlainBody text={message.body} />
                                ) : (
                                  <FormattedBody html={message.html} />
                                )}
                                {message.edited && <EditedMark />}
                              </div>
                            )}
                          </div>
                        ) : (
                          /*
                            A `div` rather than a `p`, because a formatted body can
                            be a heading or a list and a paragraph may hold neither.
                            One element for both kinds beats two that have to be
                            kept looking alike.

                            `data-selectable` because the shell turns selection off,
                            dragging across the chrome of a desktop application
                            never being deliberate. A message is what a reader does
                            mean to select, and opting back in is also what puts a
                            text cursor over the words instead of an arrow.

                            The click is not on a button, and deliberately. A
                            message can hold links, and a link inside a button is
                            both invalid and unreachable from the keyboard, so
                            wrapping one would break every link in a threaded
                            message. The control below is what the keyboard uses.
                          */
                          <div
                            className={
                              message.html === undefined
                                ? "timeline__body"
                                : "timeline__body timeline__body--rich"
                            }
                            data-kind={message.kind}
                            data-selectable
                            onClick={open}
                          >
                            {message.kind === "emote" && `${who} `}
                            {message.html === undefined ? (
                              <PlainBody text={message.body} />
                            ) : (
                              <FormattedBody html={message.html} />
                            )}
                            {message.edited && <EditedMark />}
                          </div>
                        )}
                        {message.thread !== undefined &&
                          onOpenThread !== undefined && (
                            <button
                              type="button"
                              className="timeline__thread"
                              data-participated={String(
                                message.thread.participated,
                              )}
                              data-opening={String(openingId === message.id)}
                              disabled={openingId === message.id}
                              onClick={() => onOpenThread(message.id)}
                            >
                              {message.thread.count}{" "}
                              {message.thread.count === 1 ? "reply" : "replies"}
                            </button>
                          )}
                        {message.reactions !== undefined &&
                          message.reactions.length > 0 && (
                            <div className="timeline__reactions">
                              {message.reactions.map((one) => (
                                <button
                                  key={one.key}
                                  type="button"
                                  className="timeline__reaction"
                                  aria-pressed={one.mine !== undefined}
                                  aria-label={`${one.key}, ${one.count}`}
                                  disabled={onReact === undefined}
                                  onClick={() =>
                                    onReact?.(message.id, one.key, one.mine)
                                  }
                                >
                                  <span aria-hidden="true">{one.key}</span>
                                  <span
                                    className="timeline__reaction-count"
                                    aria-hidden="true"
                                  >
                                    {one.count}
                                  </span>
                                </button>
                              ))}
                              {/*
                                One more item in the row, drawn quieter than a
                                pill: it is an action rather than something
                                anybody has said. Only here, because a message
                                with no reactions has nothing for it to sit beside
                                and the toolbar is where its first one comes from.
                              */}
                              {onReact !== undefined && (
                                <span className="timeline__add">
                                  <button
                                    type="button"
                                    className="timeline__add-key"
                                    aria-label="Add a reaction"
                                    title="Add a reaction"
                                    aria-expanded={
                                      picking?.id === message.id &&
                                      picking.at === "row"
                                    }
                                    onClick={() => toggle(message.id, "row")}
                                  >
                                    <ReactIcon />
                                  </button>
                                  {pickerFor(message, "row")}
                                </span>
                              )}
                            </div>
                          )}
                        {/*
                          What can be done to this message. Last in the row, so the
                          words are read before the things that can be done to
                          them, and quiet until the message is hovered or something
                          in it takes focus.

                          Not drawn at all on a message that has been deleted.
                          There is nothing left to answer, react to or correct,
                          and the only control that would still mean anything is
                          the one that has already been used.
                        */}
                        {message.kind !== "deleted" && (
                          <div
                            className="timeline__actions"
                            data-asking={String(confirming === message.id)}
                          >
                            {onEdit !== undefined &&
                              correctable(message, selfId) && (
                                <button
                                  type="button"
                                  className="timeline__action"
                                  aria-label="Edit"
                                  title="Edit"
                                  onClick={() => onEdit(message)}
                                >
                                  <EditIcon />
                                </button>
                              )}
                            {onDelete !== undefined &&
                              deletable(message, selfId) && (
                                /*
                                  A span, because the question hangs off this
                                  control the way the picker hangs off the one
                                  below and needs something positioned to hang
                                  from.
                                */
                                <span className="timeline__remove">
                                  <button
                                    type="button"
                                    className="timeline__action"
                                    aria-label="Delete"
                                    title="Delete"
                                    aria-haspopup="dialog"
                                    aria-expanded={confirming === message.id}
                                    onClick={() =>
                                      setConfirming((open) =>
                                        open === message.id ? null : message.id,
                                      )
                                    }
                                  >
                                    <TrashIcon />
                                  </button>
                                  {confirming === message.id && (
                                    <ConfirmDelete
                                      onConfirm={() => {
                                        setConfirming(null);
                                        onDelete(message);
                                      }}
                                      onCancel={() => setConfirming(null)}
                                    />
                                  )}
                                </span>
                              )}
                            {onReply !== undefined && (
                              <button
                                type="button"
                                className="timeline__action"
                                aria-label="Reply"
                                title="Reply"
                                onClick={() => onReply(message)}
                              >
                                <ReplyIcon className="timeline__action-glyph" />
                              </button>
                            )}
                            {onReact !== undefined && (
                              <button
                                type="button"
                                className="timeline__action"
                                aria-label="React"
                                title="React"
                                aria-expanded={
                                  picking?.id === message.id &&
                                  picking.at === "toolbar"
                                }
                                onClick={() => toggle(message.id, "toolbar")}
                              >
                                <ReactIcon />
                              </button>
                            )}
                            {/*
                              Only on a message with no thread yet: the count above
                              already opens the ones that have one.
                            */}
                            {message.thread === undefined &&
                              onOpenThread !== undefined && (
                                <button
                                  type="button"
                                  className="timeline__action"
                                  aria-label="Reply in thread"
                                  title="Reply in thread"
                                  disabled={openingId === message.id}
                                  onClick={() => onOpenThread(message.id)}
                                >
                                  <ThreadIcon />
                                </button>
                              )}
                            {onCopyLink !== undefined && (
                              /*
                                The address, on the clipboard, rather than a panel
                                offering five services to post it to. Pasting it
                                somewhere is what almost everybody wanted, and the
                                glyph turning into a tick is how they are told it
                                worked: a copy is silent otherwise, and a silent
                                control invites a second press.
                              */
                              <button
                                type="button"
                                className="timeline__action"
                                data-done={String(copiedId === message.id)}
                                aria-label={
                                  copiedId === message.id ? "Link copied" : "Copy link"
                                }
                                title={
                                  copiedId === message.id ? "Link copied" : "Copy link"
                                }
                                onClick={() => onCopyLink(message.id)}
                              >
                                {copiedId === message.id ? (
                                  <CopiedIcon />
                                ) : (
                                  <LinkIcon />
                                )}
                              </button>
                            )}
                            {pickerFor(message, "toolbar")}
                          </div>
                        )}
                      </div>
                    </Fragment>
                  );
                })}
              </div>
            </article>
          </Fragment>
        );
      })}
    </>
  );
}
