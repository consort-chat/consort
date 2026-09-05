import { useState, type RefObject } from "react";

import type { Message, MessageKind, Participant } from "../lib/api";
import { flashMessage } from "../lib/flash";
import { withAddressesNamed } from "../lib/matrixTo";
import { useRoomLinks } from "../lib/roomLinks";
import { FormattedBody } from "./FormattedBody";
import { PlainBody } from "./PlainBody";
import { MessageMedia } from "./MessageMedia";
import { PresenceDot } from "./PresenceDot";
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

/** Consecutive messages from one person, close enough together to read as one. */
export interface Group {
  /** The first message's ID, which is what makes this row's key stable. */
  id: string;
  sender: string;
  at: number;
  messages: Message[];
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
 * Exported because the room's composer draws the same arrow in front of what
 * is about to be answered, and one glyph drawn twice beats two that have to be
 * kept looking alike.
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
  if (message.body !== "") return withAddressesNamed(message.body, nameOf);
  return message.media?.name ?? "an attachment";
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
  onReact,
  onCopyLink,
  onGoTo,
}: {
  groups: Group[];
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
   * Answer a message in the room it is in.
   *
   * The whole message rather than its ID, because the composer draws a line of
   * what is being answered and the reply itself has to name who wrote it.
   *
   * Absent inside a thread panel, where answering is what the box at the
   * bottom already does and a second kind of reply would be two controls with
   * one meaning.
   */
  onReply?: (message: Message) => void;
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
    Which message has its picker open, or none. One at a time, for the reason
    a person's card has the same rule: two open at once is two panels of the
    same twelve keys with nothing saying which message either belongs to.
  */
  const [picking, setPicking] = useState<string | null>(null);
  // For the quoted line above a reply, which cannot hold a badge and so says
  // what the badge would have said.
  const { nameOf } = useRoomLinks();

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
      {groups.map((one) => {
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
          <article
            className="timeline__group"
            key={one.id}
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
                  <div
                    key={message.id}
                    className="timeline__message"
                    data-message-id={message.id}
                    {...(message.mentions?.includes(selfId)
                      ? { "data-mentions-me": "true" }
                      : {})}
                  >
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
                    {message.media !== undefined ? (
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
                        </div>
                      )}
                    {/*
                      What can be done to this message. Last in the row, so the
                      words are read before the things that can be done to
                      them, and quiet until the message is hovered or something
                      in it takes focus.
                    */}
                    <div className="timeline__actions">
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
                          aria-expanded={picking === message.id}
                          onClick={() =>
                            setPicking((open) =>
                              open === message.id ? null : message.id,
                            )
                          }
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
                      {picking === message.id && onReact !== undefined && (
                        <ReactionPicker
                          chosen={
                            new Set(
                              (message.reactions ?? [])
                                .filter((one) => one.mine !== undefined)
                                .map((one) => one.key),
                            )
                          }
                          onChoose={(key) => {
                            const already = message.reactions?.find(
                              (one) => one.key === key,
                            );
                            onReact(message.id, key, already?.mine);
                            setPicking(null);
                          }}
                          onClose={() => setPicking(null)}
                        />
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          </article>
        );
      })}
    </>
  );
}
