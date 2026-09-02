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
  onAbout,
  onOpenThread,
}: {
  groups: Group[];
  /** Display names by user ID, for whoever the room has told us about. */
  names: Record<string, string>;
  roomId: string;
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

                return (
                  <div key={message.id} className="timeline__message">
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
                          onClick={() => onOpenThread(message.id)}
                        >
                          {message.thread.count}{" "}
                          {message.thread.count === 1 ? "reply" : "replies"}
                        </button>
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
