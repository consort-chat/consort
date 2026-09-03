import type { RefObject } from "react";

import type { Message, MessageKind, Participant } from "../lib/api";
import { FormattedBody } from "./FormattedBody";
import { MessageMedia } from "./MessageMedia";
import { PresenceDot } from "./PresenceDot";
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
 * How long an answered message stays lit after being jumped to, in
 * milliseconds.
 *
 * Long enough to find with the eye after the scroll settles, short enough that
 * it is not still glowing when somebody starts reading the next thing.
 */
const FLASH = 1_400;

/**
 * A turning arrow, in front of the message being answered.
 *
 * It replaces the words "In reply to", which were not ours: they are the
 * fallback the sender writes for clients that draw no reply of their own, and
 * they arrived as a link that went nowhere.
 */
function ReplyIcon() {
  return (
    <svg
      className="timeline__reply-glyph"
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
 */
function previewOf(message: Message): string {
  if (message.body !== "") return message.body;
  return message.media?.name ?? "an attachment";
}

/**
 * Two overlapping bubbles: a conversation hanging off a message.
 *
 * Deliberately not the reply arrow above. A reply answers a message in the
 * room, and a thread takes the answer somewhere else; drawing both with one
 * glyph would make the two controls look like one control drawn twice.
 */
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
 * room it came out of. Everything it needs is passed in: it holds no state,
 * asks for nothing, and is the same markup in both places by construction
 * rather than by two files being kept in step.
 */
export function MessageGroups({
  groups,
  names,
  roomId,
  selfId,
  known,
  container,
  openingId,
  onAbout,
  onOpenThread,
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
   * A reply pointing at something not in here still draws a row, saying so. A
   * room shows a window of history and a reply can name anything older than
   * it.
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
  /** Open somebody's card, at the point that was clicked. */
  onAbout: (person: Participant, at: { x: number; y: number }) => void;
  /**
   * Open the thread hanging from a message.
   *
   * Absent inside a thread panel, where every message is already in the thread
   * being read and there is nowhere further to go.
   */
  onOpenThread?: (rootId: string) => void;
}) {
  /**
   * Scroll to a message and light it up.
   *
   * The attribute goes on the element rather than through state, and
   * deliberately. The row being jumped to may belong to a different
   * `MessageGroups` than the one that was clicked, which state here could not
   * reach; and React leaves an attribute it never set alone, so a re-render
   * does not fight it.
   */
  function goTo(eventId: string) {
    const box = container?.current;
    const target = box?.querySelector(
      `[data-message-id="${CSS.escape(eventId)}"]`,
    );
    if (!(target instanceof HTMLElement)) return;

    target.scrollIntoView({ block: "center", behavior: "smooth" });
    target.setAttribute("data-flash", "true");
    window.setTimeout(() => target.removeAttribute("data-flash"), FLASH);
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
                          <ReplyIcon />
                          <span className="timeline__reply-said">
                            Replying to a message that is not loaded.
                          </span>
                        </p>
                      ) : (
                        <button
                          type="button"
                          className="timeline__reply"
                          aria-label={`Go to ${names[answered.sender] ?? answered.sender}'s message`}
                          onClick={() => goTo(answered.id)}
                        >
                          <ReplyIcon />
                          <span className="timeline__reply-who">
                            {names[answered.sender] ?? answered.sender}
                          </span>
                          <span className="timeline__reply-said">
                            {previewOf(answered)}
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
                              message.body
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
                          message.body
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
                    {/*
                      Somewhere to begin one. Only on a message with no thread
                      yet, because the count above already opens the ones that
                      have. Last in the row so the words are read before the
                      things that can be done to them, and quiet until the
                      message is hovered or something in it takes focus.
                    */}
                    {message.thread === undefined &&
                      onOpenThread !== undefined && (
                        <div className="timeline__actions">
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
                        </div>
                      )}
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
