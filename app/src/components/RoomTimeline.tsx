import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { channelLabel } from "../lib/labels";
import {
  asCommandError,
  memberNames,
  onTimeline,
  resendState,
  timelineClose,
  timelineEarlier,
  timelineOpen,
  timelineSend,
  NO_TIMELINE,
  type Channel,
  type Message,
  type Participant,
  type Timeline,
} from "../lib/api";
import { FormattedBody } from "./FormattedBody";
import { MessageMedia } from "./MessageMedia";
import { PersonMenu } from "./PersonMenu";
import { PresenceDot } from "./PresenceDot";
import { RoomAvatar } from "./RoomAvatar";
import "./RoomTimeline.css";

/**
 * How close to the bottom counts as being at the bottom, in pixels.
 *
 * Not zero. A browser's scroll arithmetic is fractional once anything is
 * zoomed or on a display that is not one device pixel per CSS pixel, so an
 * exact comparison reads as "scrolled up" for somebody who is at the bottom
 * and never follows the conversation again.
 */
const AT_THE_BOTTOM = 48;

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
interface Group {
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
export function group(messages: readonly Message[]): Group[] {
  const groups: Group[] = [];

  for (const message of messages) {
    const last = groups.at(-1);
    const sameBreath =
      last !== undefined &&
      last.sender === message.sender &&
      message.at - (last.messages.at(-1)?.at ?? message.at) < SAME_BREATH;

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
function timeOf(at: number): string {
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
 * A room's messages, and somewhere to add to them.
 *
 * The list is not held here. `timelineOpen` starts a watcher in Rust that
 * publishes the whole of what is loaded on every change, so this component
 * draws what it is handed rather than keeping a copy it has to patch. That is
 * the same arrangement the room list uses, and for the same reason: the rules
 * about ordering, deduplication and which events are messages are all in
 * `consort_matrix::timeline`, where they are tested without a browser.
 *
 * Both kinds of channel come here. A voice channel is an ordinary Matrix room
 * that happens to carry a call, so it has an ordinary timeline sitting beside
 * the call, and there is nothing different to draw.
 */
export function RoomTimeline({ channel }: { channel: Channel }) {
  const [timeline, setTimeline] = useState<Timeline>(NO_TIMELINE);
  const [names, setNames] = useState<Record<string, string>>({});
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  /*
    Whose card is open, and where it was asked for. One at a time, for the
    reason the sidebar has the same rule: two cards about two people are two
    volume sliders somebody has to tell apart by the heading.
  */
  const [opened, setOpened] = useState<{
    person: Participant;
    at: { x: number; y: number };
  } | null>(null);

  const scroller = useRef<HTMLDivElement>(null);
  /*
    Whether the reader was at the bottom before this render. Read in a layout
    effect after the list has changed, which is too late to measure it: by then
    the new message is already in the box and everybody looks scrolled up.
  */
  const following = useRef(true);

  useEffect(() => {
    let cancelled = false;
    const unlisten = onTimeline((published) => {
      if (!cancelled) setTimeline(published);
    });

    void (async () => {
      await timelineOpen(channel.id).catch((raw: unknown) => {
        if (!cancelled) setProblem(asCommandError(raw).message);
      });
      // The room may already have been open, in which case opening it again
      // deliberately publishes nothing rather than throwing away whatever had
      // been scrolled back through. This is how a remount gets the list
      // anyway, and it is the same catch-up every other channel uses.
      await resendState().catch(() => {});
    })();

    return () => {
      cancelled = true;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, [channel.id]);

  // Separate from the subscription above, so that switching rooms does not
  // close the one being opened: this runs only when the component itself goes.
  useEffect(
    () => () => {
      void timelineClose().catch(() => {});
    },
    [],
  );

  /*
    The senders currently on screen, as a stable string so the effect below
    runs when the set changes rather than on every arriving timeline. A room
    where four people are talking resolves four names once, not four names per
    message.
  */
  const senders = useMemo(
    () => [...new Set(timeline.messages.map((message) => message.sender))].sort().join(" "),
    [timeline.messages],
  );

  useEffect(() => {
    if (senders === "") return;

    let cancelled = false;
    void memberNames(channel.id, senders.split(" "))
      .then((resolved) => {
        if (!cancelled) setNames((known) => ({ ...known, ...resolved }));
      })
      .catch(() => {
        // Their user ID is drawn instead, which is still something a person
        // recognises. A dialog about a display name would be worse.
      });

    return () => {
      cancelled = true;
    };
  }, [channel.id, senders]);

  // Before the browser paints, so following the conversation is not a visible
  // jump from where the list was to where it should have been.
  useLayoutEffect(() => {
    const box = scroller.current;
    if (box === null || !following.current) return;
    box.scrollTop = box.scrollHeight;
  }, [timeline.messages, timeline.roomId]);

  const remember = useCallback(() => {
    const box = scroller.current;
    if (box === null) return;
    following.current =
      box.scrollHeight - box.scrollTop - box.clientHeight < AT_THE_BOTTOM;
  }, []);

  async function send() {
    if (draft.trim() === "" || sending) return;

    setSending(true);
    setProblem(null);
    try {
      await timelineSend(channel.id, draft);
      // Cleared only once the homeserver has it. A box that empties on a send
      // that failed loses what somebody wrote, and retyping it is the one
      // thing an interface must never ask for.
      setDraft("");
      // Whatever they said belongs at the bottom, wherever they were reading.
      following.current = true;
    } catch (raw: unknown) {
      setProblem(asCommandError(raw).message);
    } finally {
      setSending(false);
    }
  }

  // A timeline for the room before this one, still in flight when the channel
  // changed. Drawing it would put the last room's conversation under this
  // room's name for a moment.
  const mine = timeline.roomId === channel.id;
  const messages = mine ? timeline.messages : [];
  const groups = useMemo(() => group(messages), [messages]);
  const name = channelLabel(channel);

  return (
    <section className="timeline" aria-label={`Messages in ${name}`}>
      <div className="timeline__head">
        <h1 className="timeline__name">
          {channel.kind === "voice" ? name : `#${name}`}
        </h1>
        {/*
          One line, whatever the room wrote. A topic is free text and some are
          paragraphs, and a heading that grows to four lines pushes the
          conversation off the bottom of the window. The whole of it is on the
          pointer.
        */}
        {channel.topic !== undefined && (
          <p className="timeline__topic" title={channel.topic}>
            {channel.topic}
          </p>
        )}
      </div>

      <div
        className="timeline__scroll"
        ref={scroller}
        onScroll={remember}
        // A scrollable region has to be reachable by keyboard, or the only way
        // to read a long room is a mouse.
        tabIndex={0}
        role="log"
        aria-label={`Messages in ${name}`}
      >
        {mine && timeline.moreBefore && (
          <button
            type="button"
            className="timeline__earlier"
            disabled={timeline.loading}
            onClick={() => void timelineEarlier()}
          >
            {timeline.loading ? "Loading..." : "Load older messages"}
          </button>
        )}

        {mine && !timeline.loading && groups.length === 0 && (
          <p className="timeline__empty">
            Nothing has been said here yet.
          </p>
        )}

        {groups.map((one) => {
          // Their display name if the room has told us one, and their user ID
          // if it has not. Whichever it is, it is what the byline draws, what
          // the group announces itself as, and what the card is about.
          const who = names[one.sender] ?? one.sender;
          const about = (event: { clientX: number; clientY: number }) =>
            setOpened({
              person: { id: one.sender, name: who },
              at: { x: event.clientX, y: event.clientY },
            });

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
                  roomId={channel.id}
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
                    The whole date lives here rather than on the words. A
                    tooltip that follows the pointer across every sentence in a
                    room appears over the one thing somebody is reading; the
                    clock time is already the thing being asked about.
                  */}
                  <time
                    className="timeline__at"
                    dateTime={new Date(one.at).toISOString()}
                    title={dateOf(one.at)}
                  >
                    {timeOf(one.at)}
                  </time>
                </p>
                {one.messages.map((message) =>
                  /*
                    An attachment is drawn instead of its body rather than
                    beside it. The body of an image is its filename, and a line
                    reading "screenshot.png" above the screenshot is the thing
                    somebody sent a picture to avoid.
                  */
                  message.media !== undefined ? (
                    <MessageMedia
                      key={message.id}
                      kind={message.kind === "video" ? "video" : "image"}
                      media={message.media}
                      name={message.body}
                    />
                  ) : (
                    /*
                      A `div` rather than a `p`, because a formatted body can be
                      a heading or a list and a paragraph may hold neither. One
                      element for both kinds beats two that have to be kept
                      looking alike.

                      `data-selectable` because the shell turns selection off,
                      dragging across the chrome of a desktop application never
                      being deliberate. A message is what a reader does mean to
                      select, and opting back in is also what puts a text cursor
                      over the words instead of an arrow.
                    */
                    <div
                      key={message.id}
                      className={
                        message.html === undefined
                          ? "timeline__body"
                          : "timeline__body timeline__body--rich"
                      }
                      data-kind={message.kind}
                      data-selectable
                    >
                      {message.kind === "emote" && `${who} `}
                      {message.html === undefined ? (
                        message.body
                      ) : (
                        <FormattedBody html={message.html} />
                      )}
                    </div>
                  ),
                )}
              </div>
            </article>
          );
        })}
      </div>

      {problem !== null && (
        <p className="timeline__problem" role="alert">
          {problem}
        </p>
      )}

      <form
        className="timeline__composer"
        onSubmit={(event) => {
          event.preventDefault();
          void send();
        }}
      >
        <label className="timeline__label" htmlFor="timeline-draft">
          Message {channel.kind === "voice" ? name : `#${name}`}
        </label>
        <textarea
          id="timeline-draft"
          className="timeline__draft"
          rows={1}
          value={draft}
          placeholder={`Message ${channel.kind === "voice" ? name : `#${name}`}`}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            // Enter sends and Shift+Enter breaks the line, which is what every
            // client anybody already uses does. Without the modifier check a
            // paragraph is impossible to type.
            if (event.key !== "Enter" || event.shiftKey) return;
            event.preventDefault();
            void send();
          }}
        />
        <button
          type="submit"
          className="timeline__send"
          disabled={draft.trim() === "" || sending}
        >
          Send
        </button>
      </form>

      {/*
        Outside the scrolling list rather than in the group that opened it, so
        that it survives its own row being scrolled away and so that one is
        open at a time.
      */}
      {opened !== null && (
        <PersonMenu
          key={opened.person.id}
          person={opened.person}
          roomId={channel.id}
          at={opened.at}
          onClose={() => setOpened(null)}
        />
      )}
    </section>
  );
}
