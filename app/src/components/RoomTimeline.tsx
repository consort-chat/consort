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
  onThread,
  onTimeline,
  resendState,
  timelineClose,
  timelineEarlier,
  timelineOpen,
  timelineSend,
  threadOpen,
  NO_TIMELINE,
  type Channel,
  type Participant,
  type Timeline,
} from "../lib/api";
import { MessageGroups, group } from "./MessageGroups";
import { PersonMenu } from "./PersonMenu";
import { SidebarToggle } from "./SidebarToggle";
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
export function RoomTimeline({
  channel,
  selfId,
  onOpenRoom,
  onUnfold,
}: {
  channel: Channel;
  /** Whoever is signed in, so a person's card can tell when it is about them. */
  selfId: string;
  /** Show a room, by ID. Passed to a person's card for its Message button. */
  onOpenRoom: (roomId: string) => void;
  /**
   * Bring the channel list back, when it has been folded away.
   *
   * Absent while it is on screen, because the control that folds it lives in
   * its own header and two of them would be one job with two answers.
   */
  onUnfold?: () => void;
}) {
  const [timeline, setTimeline] = useState<Timeline>(NO_TIMELINE);
  const [names, setNames] = useState<Record<string, string>>({});
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  /*
    Which thread has been asked for and not yet arrived. `threadOpen` answers
    immediately, because it is a message to the room's watcher in Rust rather
    than a fetch, so the command settling says nothing about whether the panel
    is there. What does is the panel's own channel, below.
  */
  const [opening, setOpening] = useState<string | null>(null);
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

  /*
    A second listener on the thread channel, which the panel also holds. Not a
    duplicate of its job: the panel draws the thread and this only wants to
    know that one arrived, so the reply count somebody pressed can stop
    turning. Cleared on any thread rather than on a matching one, which is the
    condition that cannot stick: opening always ends in a publish, and two
    quick presses would otherwise leave the first turning for ever.
  */
  useEffect(() => {
    let cancelled = false;
    const unlisten = onThread(() => {
      if (!cancelled) setOpening(null);
    });

    return () => {
      cancelled = true;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

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
  // What a reply in this room may point at, which is whatever is loaded.
  const known = useMemo(
    () => new Map(messages.map((message) => [message.id, message])),
    [messages],
  );
  const name = channelLabel(channel);

  return (
    <section className="timeline" aria-label={`Messages in ${name}`}>
      <div className="timeline__head">
        {onUnfold !== undefined && (
          <SidebarToggle folded onToggle={onUnfold} />
        )}
        <div className="timeline__titles">
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

        <MessageGroups
          groups={groups}
          names={names}
          roomId={channel.id}
          selfId={selfId}
          known={known}
          container={scroller}
          onAbout={(person, at) => setOpened({ person, at })}
          openingId={opening}
          onOpenThread={(rootId) => {
            setOpening(rootId);
            // Cleared here as well as on the channel, because a command that
            // failed publishes nothing and the control would keep turning.
            void threadOpen(rootId).catch(() => setOpening(null));
          }}
        />
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
          selfId={selfId}
          at={opened.at}
          onClose={() => setOpened(null)}
          onOpenRoom={onOpenRoom}
        />
      )}
    </section>
  );
}
