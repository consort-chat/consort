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
 *
 * Exported because the thread panel follows its own conversation on the same
 * terms, and two boxes on one screen disagreeing about what counts as the
 * bottom would be a difference nobody could see the reason for.
 */
export const AT_THE_BOTTOM = 48;

/**
 * How close to the top of what is loaded starts fetching the page above it,
 * in pixels.
 *
 * Not zero. A page takes a homeserver round trip and a round of decryption, so
 * asking at the moment somebody reaches the wall makes every one of them a
 * stop followed by a jump. Asking a screenful early means the page is usually
 * already there.
 */
const NEAR_THE_TOP = 200;

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

  /*
    Whether the reader is near the top of what is loaded. State rather than a
    ref because it is what asks for the page above, and that has to happen when
    the answer changes rather than when a render happens to notice.
  */
  const [nearTheTop, setNearTheTop] = useState(false);

  const scroller = useRef<HTMLDivElement>(null);
  /*
    Whether the reader was at the bottom before this render. Read in a layout
    effect after the list has changed, which is too late to measure it: by then
    the new message is already in the box and everybody looks scrolled up.
  */
  const following = useRef(true);
  /*
    How far the bottom was from the reader before this render. The anchor a
    page of history has to be held against: it arrives above them and pushes
    everything down by its own height, so this is the one number that does not
    change when it lands.
  */
  const fromBottom = useRef(0);
  /*
    The oldest message drawn, so a render can tell a page landing on the front
    from a message landing on the back. The two need opposite anchors and there
    is nothing else in a published timeline that says which happened.
  */
  const oldest = useRef<string | undefined>(undefined);

  // A timeline for the room before this one, still in flight when the channel
  // changed. Drawing it would put the last room's conversation under this
  // room's name for a moment.
  const mine = timeline.roomId === channel.id;

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

  /** Read where the reader is. On every scroll, and after the list changes. */
  const measure = useCallback(() => {
    const box = scroller.current;
    if (box === null) return;
    following.current =
      box.scrollHeight - box.scrollTop - box.clientHeight < AT_THE_BOTTOM;
    fromBottom.current = box.scrollHeight - box.scrollTop;
    setNearTheTop(box.scrollTop < NEAR_THE_TOP);
  }, []);

  /*
    Declared before the effect below so it runs first. A different room is a
    different conversation: it opens at its own bottom, and nothing about where
    the last one was left applies to it.
  */
  useLayoutEffect(() => {
    following.current = true;
    oldest.current = undefined;
  }, [channel.id]);

  // Before the browser paints, so neither following the conversation nor
  // holding a reader's place through a page of history is a visible jump.
  useLayoutEffect(() => {
    const box = scroller.current;
    if (box === null) return;

    const first = timeline.messages[0]?.id;
    const older = oldest.current !== undefined && first !== oldest.current;
    oldest.current = first;

    if (following.current) {
      box.scrollTop = box.scrollHeight;
    } else if (older) {
      // Anchored on the bottom rather than on scrollTop. A page lands above
      // the reader and moves everything down by its own height, so holding
      // scrollTop would leave them at the top of a page they have not read.
      box.scrollTop = box.scrollHeight - fromBottom.current;
    }

    // After the two above, so what is recorded is where the reader ended up.
    // Also what covers a room short enough that nothing can be scrolled: the
    // ask below would otherwise wait for a scroll event that cannot happen.
    measure();
  }, [timeline.messages, timeline.roomId, measure]);

  /*
    Ask for the page above when the reader gets near the top of what is loaded.

    An effect rather than a branch in the scroll handler, and that is the whole
    guard against asking twenty times for one page. A scroll handler runs at
    frame rate, and the `loading` it would test against has to travel out to
    Rust and back before it is true here. An effect runs when one of these four
    changes, and none of them changes while the ask is in flight.
  */
  useEffect(() => {
    if (!mine || !nearTheTop || !timeline.moreBefore || timeline.loading) {
      return;
    }
    void timelineEarlier().catch(() => {
      // The watcher has gone, which is what a scroll landing at the same
      // moment as a room change is. There is nothing to say about it.
    });
  }, [mine, nearTheTop, timeline.moreBefore, timeline.loading]);

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
        onScroll={measure}
        // A scrollable region has to be reachable by keyboard, or the only way
        // to read a long room is a mouse.
        tabIndex={0}
        role="log"
        aria-label={`Messages in ${name}`}
      >
        {/*
          Said rather than offered. Reaching the top asks for the page above on
          its own, so the only thing left to report is that it is on its way.
          The start of a room says nothing at all: there is no news in history
          that does not exist.
        */}
        {mine && timeline.loading && (
          <p className="timeline__earlier">Loading earlier messages...</p>
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
