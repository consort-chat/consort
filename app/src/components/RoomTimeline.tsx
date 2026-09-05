import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { flashMessage } from "../lib/flash";
import { channelLabel, typingLabel } from "../lib/labels";
import { useRoomLinks } from "../lib/roomLinks";
import {
  asCommandError,
  memberNames,
  onThread,
  onTimeline,
  onTyping,
  resendState,
  timelineClose,
  timelineCopyLink,
  timelineEarlier,
  timelineGoTo,
  timelineLater,
  timelineOpen,
  timelinePresent,
  timelineReact,
  timelineReply,
  timelineSend,
  timelineTyping,
  timelineUnreact,
  threadOpen,
  NO_TIMELINE,
  type Channel,
  type Message,
  type Participant,
  type Timeline,
} from "../lib/api";
import { MessageGroups, ReplyIcon, group, previewOf } from "./MessageGroups";
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
 * How long to leave between saying this session is typing, in milliseconds.
 *
 * Not for the homeserver's sake: the SDK already holds the time of the last
 * notice per room and sends nothing while one is current. This is so that a
 * sentence is a handful of IPC calls rather than one per key pressed.
 */
const TYPING_EVERY = 3_000;

/**
 * How long a copied message address stays acknowledged, in milliseconds.
 *
 * A copy is silent otherwise, and a control that says nothing invites a second
 * press. Long enough to read the tick, short enough that the toolbar is back to
 * its usual self before anybody looks again.
 *
 * Exported because a thread panel offers the same control on the same messages,
 * and two ticks on one screen holding for different lengths of time is a
 * difference nobody could see the reason for.
 */
export const COPIED_FOR = 1_800;

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
  focus,
  onOpenRoom,
  onUnfold,
}: {
  channel: Channel;
  /** Whoever is signed in, so a person's card can tell when it is about them. */
  selfId: string;
  /**
   * A message somebody followed a link to, to be jumped to once it is drawn.
   *
   * A fresh object per press rather than the ID on its own, so that following
   * the same link twice flashes the message twice. Nothing here can fetch a
   * message older than what is loaded, so a link into last year opens the room
   * and stops there.
   */
  focus?: { eventId: string } | null;
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
    Which message the composer is answering, or none. The whole message rather
    than its ID, because the line above the box quotes it and the send has to
    name who wrote it.
  */
  const [answering, setAnswering] = useState<Message | null>(null);
  /* The message whose address has just gone to the clipboard, or none. */
  const [copied, setCopied] = useState<string | null>(null);
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
  /** Whether the reader is near the bottom of what is loaded, on the same terms. */
  const [nearTheBottom, setNearTheBottom] = useState(false);
  /** Who is typing in this room, as Matrix user IDs, ours already removed. */
  const [typists, setTypists] = useState<string[]>([]);

  // For the line above the composer, which quotes a message and so needs the
  // same words the message's own badge draws.
  const { nameOf } = useRoomLinks();

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
  /*
    When this session last said it was typing. A ref rather than state because
    nothing is drawn from it: it exists only to keep a sentence from being one
    IPC call per keystroke.
  */
  const said = useRef(0);
  /*
    The message a link asked to be shown, until it has been. A ref because it
    is consumed rather than drawn: the message may not be loaded when the room
    opens, and the effect below tries again on every timeline that arrives
    until one carries it.
  */
  const wanted = useRef<string | null>(null);
  /*
    The message a window has already been asked for, so that a link naming
    something unreachable asks once rather than on every timeline that arrives
    afterwards. Cleared when the jump lands, so the same message can be gone to
    again later.
  */
  const asked = useRef<string | null>(null);
  // So the box takes what somebody types next after pressing Reply. Without it
  // the control moves the conversation and then asks them to click again.
  const draftBox = useRef<HTMLTextAreaElement>(null);

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

  useEffect(() => {
    let cancelled = false;
    const unlisten = onTyping((published) => {
      // One channel serves whichever room is open, so an answer about the
      // room before this one is dropped rather than drawn under this room's
      // name.
      if (!cancelled && published.roomId === channel.id) {
        setTypists(published.users);
      }
    });

    return () => {
      cancelled = true;
      setTypists([]);
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, [channel.id]);

  /*
    Stop saying this session is typing when the room changes or the pane goes.
    Without it, a half-written message abandoned by clicking another channel
    leaves a name typing in the room just left until the homeserver's own
    timeout expires it.
  */
  useEffect(
    () => () => {
      said.current = 0;
      void timelineTyping(channel.id, false).catch(() => {});
    },
    [channel.id],
  );

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
    () =>
      [
        ...new Set([
          ...timeline.messages.map((message) => message.sender),
          // The typists too. Somebody can be typing without having said
          // anything yet, and their user ID is not a name to put in front of
          // "is typing".
          ...typists,
        ]),
      ]
        .sort()
        .join(" "),
    [timeline.messages, typists],
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
    const below = box.scrollHeight - box.scrollTop - box.clientHeight;
    following.current = below < AT_THE_BOTTOM;
    fromBottom.current = box.scrollHeight - box.scrollTop;
    setNearTheTop(box.scrollTop < NEAR_THE_TOP);
    // The same distance at the other end, and for the same reason. Only ever
    // acted on inside a window somebody jumped into: the live end has nothing
    // after it to ask for, and being near the bottom of it is the ordinary
    // state of reading a room.
    setNearTheBottom(below < NEAR_THE_TOP);
  }, []);

  /*
    Declared before the effect below so it runs first. A different room is a
    different conversation: it opens at its own bottom, and nothing about where
    the last one was left applies to it.
  */
  useLayoutEffect(() => {
    following.current = true;
    oldest.current = undefined;
    // A jump into the room before this one is not one to carry into this one.
    wanted.current = null;
    asked.current = null;
  }, [channel.id]);

  /*
    Declared before the effect below, like the one above, so it runs first.

    A window somebody jumped into replaces the whole list rather than adding to
    either end of it, so neither of the anchors below applies: they are at the
    message they asked for, which the jump itself scrolls to. Coming back to
    the present is the opposite ask and lands at the bottom.
  */
  useLayoutEffect(() => {
    if (!mine) return;
    following.current = timeline.focus === undefined;
    oldest.current = undefined;
  }, [timeline.focus, mine]);

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
    Stay at the bottom while the list is still growing.

    An attachment's bytes arrive long after the effect above has run: the URL
    is on the `consortmedia` scheme, so Rust fetches and decrypts the file
    before the browser has a picture to lay out. The box then gets taller under
    somebody who was at the bottom of it, and growing is not scrolling. No
    event fires, nothing re-renders, and they are left as far up as the picture
    is tall until the next message happens to arrive.

    `load` does not bubble, but it does capture, so one listener on the box
    catches every picture inside it including the ones drawn later.
  */
  useEffect(() => {
    const box = scroller.current;
    if (box === null) return;

    const settle = () => {
      if (!following.current) return;
      box.scrollTop = box.scrollHeight;
      measure();
    };

    box.addEventListener("load", settle, true);
    return () => {
      box.removeEventListener("load", settle, true);
    };
  }, [measure]);

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

  /*
    And the page below, on the same terms. `moreAfter` is false at the live
    end, so outside a window somebody jumped into this never fires.
  */
  useEffect(() => {
    if (!mine || !nearTheBottom || !timeline.moreAfter || timeline.loadingAfter) {
      return;
    }
    void timelineLater().catch(() => {});
  }, [mine, nearTheBottom, timeline.moreAfter, timeline.loadingAfter]);

  // Recorded rather than acted on, so that a press arriving before the room has
  // any messages is not lost. The effect below is what spends it.
  useEffect(() => {
    const eventId = focus?.eventId;
    if (eventId !== undefined) wanted.current = eventId;
  }, [focus]);

  /*
    Go to a message somebody followed a link to, once it is drawn.

    Runs again on every timeline, because opening the room and drawing its
    messages are two moments and the link is pressed before either. Cleared the
    first time it lands, so a later message arriving does not drag the reader
    back to the same place.
  */
  useEffect(() => {
    const eventId = wanted.current;
    if (eventId === null || !mine) return;
    if (!flashMessage(scroller.current, eventId)) {
      /*
        Not drawn. Asked for from the homeserver instead, once the room has
        finished reading its own page: a message near the bottom is about to
        arrive on its own, and fetching a window around it would move the
        reader away from the room to arrive back at the same place.

        Once per message, because the answer to a link naming something this
        account cannot read is that nothing changes, and a condition that does
        not change is not one to keep asking about.
      */
      if (!timeline.loading && asked.current !== eventId) {
        asked.current = eventId;
        goTo(eventId);
      }
      return;
    }

    wanted.current = null;
    asked.current = null;
    // The jump is where they asked to be. Following the bottom from here would
    // undo it the moment anybody says anything.
    following.current = false;
  }, [focus, timeline.messages, timeline.loading, mine]);

  // Nothing but the passing of time takes the tick off the copy control.
  useEffect(() => {
    if (copied === null) return;
    const timer = window.setTimeout(() => setCopied(null), COPIED_FOR);
    return () => window.clearTimeout(timer);
  }, [copied]);

  /**
   * Tell the room this session is typing, at most every few seconds.
   *
   * Emptying the box says so immediately rather than waiting, because
   * abandoning a message is exactly when the name should come down.
   */
  function report(text: string) {
    const now = Date.now();
    if (text.trim() === "") {
      if (said.current === 0) return;
      said.current = 0;
      void timelineTyping(channel.id, false).catch(() => {});
      return;
    }
    if (now - said.current < TYPING_EVERY) return;

    said.current = now;
    void timelineTyping(channel.id, true).catch(() => {});
  }

  /** Answer this message, with the box ready for what comes next. */
  function reply(message: Message) {
    setAnswering(message);
    draftBox.current?.focus();
  }

  /**
   * Go to a message older than the window of history loaded.
   *
   * Recorded before it is asked for, on the same terms as a link: the window
   * arrives as a whole new timeline, and the effect above is what lights the
   * message up once it is drawn.
   */
  function goTo(eventId: string) {
    wanted.current = eventId;
    asked.current = eventId;
    void timelineGoTo(eventId).catch((raw: unknown) => {
      setProblem(asCommandError(raw).message);
    });
  }

  /** Put one message's address on the clipboard, and say that it worked. */
  function copyLink(eventId: string) {
    void timelineCopyLink(channel.id, eventId)
      .then(() => setCopied(eventId))
      .catch((raw: unknown) => {
        setProblem(asCommandError(raw).message);
      });
  }

  async function send() {
    if (draft.trim() === "" || sending) return;

    setSending(true);
    setProblem(null);
    try {
      // A reply and a message differ only in what they name. Both land in the
      // room, and both appear when the sync brings them back.
      await (answering === null
        ? timelineSend(channel.id, draft)
        : timelineReply(channel.id, answering.id, answering.sender, draft));
      // Cleared only once the homeserver has it. A box that empties on a send
      // that failed loses what somebody wrote, and retyping it is the one
      // thing an interface must never ask for.
      setDraft("");
      setAnswering(null);
      // Said, so no longer typing. Before the scroll rather than after it,
      // because the room should stop showing this name the moment the message
      // it was writing arrives.
      said.current = 0;
      void timelineTyping(channel.id, false).catch(() => {});
      // Whatever they said belongs at the bottom, wherever they were reading.
      following.current = true;
      // Including out of a window of last March, which is where it does not
      // belong: the message went to the live end of the room, and watching it
      // not appear is worse than being moved to where it did.
      if (timeline.focus !== undefined) {
        void timelinePresent().catch(() => {});
      }
    } catch (raw: unknown) {
      setProblem(asCommandError(raw).message);
    } finally {
      setSending(false);
    }
  }

  const messages = mine ? timeline.messages : [];
  const groups = useMemo(() => group(messages), [messages]);
  /*
    What a reply in this room may point at: whatever is loaded, and beside it
    whatever the room looked up for the replies naming something older than
    that. The row is drawn the same way either way. What differs is where
    pressing it goes, which `MessageGroups` decides by whether the message it
    names is on screen.
  */
  const answered = mine ? timeline.answered : undefined;
  const known = useMemo(
    () =>
      new Map(
        [...messages, ...(answered ?? [])].map((message) => [
          message.id,
          message,
        ]),
      ),
    [messages, answered],
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

      {/*
        Said whenever the room is not showing the present. A conversation from
        last March and one that has gone quiet look the same otherwise, and
        somebody who jumped into the middle of the history has no other way
        back to the bottom: scrolling would be a page at a time.

        Above the scrolling box rather than in it, so it stays put while the
        window it is describing is read.
      */}
      {mine && timeline.focus !== undefined && (
        <div className="timeline__elsewhere">
          <p className="timeline__elsewhere-said">
            Showing older messages.
          </p>
          <button
            type="button"
            className="timeline__elsewhere-back"
            onClick={() => {
              void timelinePresent().catch((raw: unknown) => {
                setProblem(asCommandError(raw).message);
              });
            }}
          >
            Back to the present
          </button>
        </div>
      )}

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
          <p className="timeline__paging">Loading earlier messages...</p>
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
          onReact={(eventId, key, mine) => {
            // Whichever of the two it is. A pill is one control and the
            // annotation this session already has on that key is what decides,
            // which is exactly what `mine` carries.
            const done =
              mine === undefined
                ? timelineReact(channel.id, eventId, key)
                : timelineUnreact(channel.id, mine);
            void done.catch((raw: unknown) => {
              setProblem(asCommandError(raw).message);
            });
          }}
          openingId={opening}
          copiedId={copied}
          onReply={reply}
          onCopyLink={copyLink}
          onOpenThread={(rootId) => {
            setOpening(rootId);
            // Cleared here as well as on the channel, because a command that
            // failed publishes nothing and the control would keep turning.
            void threadOpen(rootId).catch(() => setOpening(null));
          }}
          onGoTo={goTo}
        />

        {/* The other end's version of the line above, and it is the same news. */}
        {mine && timeline.loadingAfter && (
          <p className="timeline__paging">Loading later messages...</p>
        )}
      </div>

      {problem !== null && (
        <p className="timeline__problem" role="alert">
          {problem}
        </p>
      )}

      {/*
        Always drawn, empty and all. A line that appeared and disappeared
        would change the height of the scrolling box above it, which moves the
        conversation under whoever is reading it, twice per sentence somebody
        else types.
      */}
      <p className="timeline__typing" role="status">
        {typingLabel(
          typists.filter((who) => who !== selfId).map((who) => names[who] ?? who),
        )}
      </p>

      {/*
        What the next message will answer, above the box that writes it.

        A line rather than the quoted fallback other clients put in the message
        itself. Nothing Consort sends carries a quote: the specification stopped
        asking for one because every client that draws replies has to strip it
        again, and the row above the answer is drawn from the relation.
      */}
      {answering !== null && (
        <div className="timeline__answering">
          <ReplyIcon className="timeline__answering-glyph" />
          <span className="timeline__answering-who">
            {names[answering.sender] ?? answering.sender}
          </span>
          <span className="timeline__answering-said">
            {previewOf(answering, nameOf)}
          </span>
          <button
            type="button"
            className="timeline__answering-stop"
            aria-label="Stop replying"
            onClick={() => setAnswering(null)}
          >
            &times;
          </button>
        </div>
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
          ref={draftBox}
          rows={1}
          value={draft}
          placeholder={`Message ${channel.kind === "voice" ? name : `#${name}`}`}
          onChange={(event) => {
            setDraft(event.target.value);
            report(event.target.value);
          }}
          onKeyDown={(event) => {
            /*
              Escape stops answering, which is what it does to everything else
              here that can be dismissed. Stopped where it is caught, so that
              one press cancels the reply rather than also shutting the thread
              panel listening on the window behind it.
            */
            if (event.key === "Escape" && answering !== null) {
              event.stopPropagation();
              setAnswering(null);
              return;
            }
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
