import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";

import {
  asCommandError,
  memberNames,
  onThread,
  resendState,
  threadOpen,
  threadSend,
  timelineCopyLink,
  timelineDelete,
  timelineEdit,
  timelineReact,
  timelineUnreact,
  type Message,
  type Participant,
  type Thread,
} from "../lib/api";
import { useRoomLinks } from "../lib/roomLinks";
import { ComposerTarget } from "./ComposerTarget";
import { MessageGroups, group, previewOf } from "./MessageGroups";
import { PersonMenu } from "./PersonMenu";
import { AT_THE_BOTTOM, COPIED_FOR } from "./RoomTimeline";
import "./ThreadPanel.css";

/**
 * The narrowest the panel may be dragged, in pixels.
 *
 * A thread is a conversation, and a column narrower than this is one word per
 * line with a picture squeezed into the middle of it.
 */
const NARROWEST = 300;

/** The most of the window a thread may take. */
const MOST = 0.6;

/** How far one press of an arrow key moves the edge, in pixels. */
const STEP = 16;

/**
 * The width a thread opens at: three tenths of the window.
 *
 * Proportional rather than fixed, because the fixed 340px it used to be was a
 * strip on a large screen and half of a small one. Clamped on the way out for
 * the same reason it is clamped on the way in.
 */
export function defaultThreadWidth(): number {
  return clampThreadWidth(Math.round(window.innerWidth * 0.3));
}

/**
 * A width the panel may actually be.
 *
 * Exported because the shell holds the number, so that a panel closed and
 * reopened is the width it was left at, and the shell has to re-clamp when the
 * window is made smaller than the panel.
 */
export function clampThreadWidth(width: number): number {
  const most = Math.round(window.innerWidth * MOST);
  // The minimum last, so a window too small for both still leaves a readable
  // column rather than a sliver.
  return Math.max(NARROWEST, Math.min(width, most));
}

/**
 * One thread, beside the room it came out of.
 *
 * Nothing is held here. `threadOpen` tells the room's watcher in Rust which
 * thread to follow, and it publishes the whole of what is loaded on every
 * change, so this draws what it is handed on exactly the terms the room does.
 * Which thread is open is Rust's answer rather than this component's, which is
 * what lets a message in the room open one without the two having to be wired
 * to each other.
 *
 * Absent rather than empty when nothing is open. A shut panel should give its
 * column back to the conversation rather than sit there as a blank strip.
 */
export function ThreadPanel({
  selfId,
  onOpenRoom,
  onOpen,
  width,
  onResize,
}: {
  /** Whoever is signed in, so a person's card can tell when it is about them. */
  selfId: string;
  /** Show a room, by ID. Passed to a person's card for its Message button. */
  onOpenRoom: (roomId: string) => void;
  /**
   * Said whenever a thread is on screen.
   *
   * Which thread is open is Rust's answer rather than the shell's, so the
   * shell has no other way to learn that this column is now spoken for. It
   * uses it to put the room's details away: two panels beside a room leave the
   * room a strip.
   */
  onOpen: () => void;
  /** How wide to draw, in pixels. Held by the shell, so a shut panel keeps it. */
  width: number;
  /** Report a width the grip was dragged or nudged to. Already clamped. */
  onResize: (width: number) => void;
}) {
  const [thread, setThread] = useState<Thread | null>(null);
  const [names, setNames] = useState<Record<string, string>>({});
  const [opened, setOpened] = useState<{
    person: Participant;
    at: { x: number; y: number };
  } | null>(null);
  const [draft, setDraft] = useState("");
  /*
    Which reply the box is answering, or none, in which case it answers the
    thread. The whole message rather than its ID, because the line above the
    box quotes it and the send has to name who wrote it.
  */
  const [answering, setAnswering] = useState<Message | null>(null);
  /*
    Which message the box is correcting, or none. Mutually exclusive with
    `answering` by construction: one box with one Send cannot have two things
    to do and no way to say which.
  */
  const [editing, setEditing] = useState<Message | null>(null);
  /*
    The same, readable without making the effect that clears it depend on it.
    An effect naming `editing` in its dependencies would fire on every press
    of Edit and undo the press.

    Written during the render rather than from an effect, which is safe here
    because nothing reads it during one.
  */
  const editingNow = useRef<Message | null>(null);
  editingNow.current = editing;
  const [sending, setSending] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  /* The reply whose address has just gone to the clipboard, or none. */
  const [copied, setCopied] = useState<string | null>(null);
  // The scrolling box, so a jump to an answered message is looked for in this
  // panel rather than in the room beside it, which draws the root as well.
  const scroller = useRef<HTMLDivElement>(null);
  // The box, so pressing Edit or Reply on a message puts the cursor where the
  // answer is typed rather than leaving it on the control that was pressed.
  const draftBox = useRef<HTMLTextAreaElement>(null);
  /*
    Whether the reader was at the bottom before this render, on the room's
    terms. True to begin with, which is what opens a thread at its newest
    reply: a panel that opened at the top put somebody at the oldest thing
    loaded and made them scroll through a conversation to find the part they
    pressed it for.
  */
  const following = useRef(true);

  useEffect(() => {
    let cancelled = false;
    const unlisten = onThread((published) => {
      if (!cancelled) setThread(published);
    });

    void (async () => {
      // After the listener is attached, and not before. The panel can mount
      // with a thread already open, and the thing that would otherwise fill it
      // is somebody replying, which in a finished conversation is never. Ask
      // first and the answer goes to nobody, which is the bug the resend
      // exists for.
      await unlisten;
      if (!cancelled) await resendState().catch(() => {});
    })();

    return () => {
      cancelled = true;
      void unlisten.then((stop) => {
        stop();
      });
    };
  }, []);

  useEffect(() => {
    if (thread !== null) onOpen();
  }, [thread, onOpen]);

  const roomId = thread?.roomId ?? "";
  /*
    Everybody in the panel, as a stable string, so names are resolved when the
    set of people changes rather than on every arriving reply.
  */
  const senders = useMemo(() => {
    if (thread === null) return "";
    const everybody = [...thread.messages, ...(thread.root ? [thread.root] : [])];
    return [...new Set(everybody.map((message) => message.sender))].sort().join(" ");
  }, [thread]);

  useEffect(() => {
    if (senders === "" || roomId === "") return;

    let cancelled = false;
    void memberNames(roomId, senders.split(" "))
      .then((resolved) => {
        if (!cancelled) setNames((known) => ({ ...known, ...resolved }));
      })
      .catch(() => {
        // Their user ID is drawn instead, which is still something a person
        // recognises.
      });

    return () => {
      cancelled = true;
    };
  }, [roomId, senders]);

  /*
    Declared before the effect that scrolls, and that order is the point. A
    different thread is a different conversation, so it opens at its own bottom
    however far up the last one was left.
  */
  useLayoutEffect(() => {
    following.current = true;
  }, [thread?.rootId]);

  /*
    The composer goes with it, for the same reason. A message in the thread
    that was closed is not something the one now open can answer or correct,
    and a correction left pointed at it would send the words typed here to
    another conversation.

    The draft survives an ordinary reply and not a correction, which is the
    rule the room's composer has: what is in the box while editing is the old
    message rather than something somebody typed.
  */
  useEffect(() => {
    setAnswering(null);
    stopEditing();
    // Neither of those is read here, and both are the same function on every
    // render. What this depends on is the thread changing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [thread?.rootId]);

  // Before the browser paints, so opening a thread is not a visible fall from
  // the top of it to the bottom.
  useLayoutEffect(() => {
    const box = scroller.current;
    if (box === null || !following.current) return;
    box.scrollTop = box.scrollHeight;
  }, [thread?.messages, thread?.rootId]);

  const remember = useCallback(() => {
    const box = scroller.current;
    if (box === null) return;
    following.current =
      box.scrollHeight - box.scrollTop - box.clientHeight < AT_THE_BOTTOM;
  }, []);

  /*
    Stay at the bottom while the replies are still growing. The room beside
    this one carries the same listener and the reason it is needed, which is
    that a picture finishing its download is not a scroll and so goes
    unnoticed by everything else here.
  */
  useEffect(() => {
    const box = scroller.current;
    if (box === null) return;

    const settle = () => {
      if (!following.current) return;
      box.scrollTop = box.scrollHeight;
    };

    box.addEventListener("load", settle, true);
    return () => {
      box.removeEventListener("load", settle, true);
    };
    // The box goes with the panel, so the listener is attached again each time
    // one opens rather than once for the life of the component.
  }, [thread?.rootId]);

  /*
    Escape shuts the panel, which is what it does to everything else here that
    can be dismissed.

    On `window` rather than on `document`, and that is the whole of why the
    picture viewer, the settings dialog, the reaction picker and a person's
    card each stop the key where they catch it. Theirs are on the document,
    which is one step nearer the press, so stopping there is what keeps one
    press to one thing while any of them is open over this.
  */
  const open = thread !== null;
  useEffect(() => {
    if (!open) return;

    function shut(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      void threadOpen(null).catch(() => {});
    }

    window.addEventListener("keydown", shut);
    return () => {
      window.removeEventListener("keydown", shut);
    };
  }, [open]);

  // Nothing but the passing of time takes the tick off the copy control.
  useEffect(() => {
    if (copied === null) return;
    const timer = window.setTimeout(() => setCopied(null), COPIED_FOR);
    return () => window.clearTimeout(timer);
  }, [copied]);

  const { nameOf } = useRoomLinks();
  /*
    What the panel is called: the message the whole thing hangs from, in one
    line. "Thread" is what this used to say, which is true of every thread and
    so tells somebody reading a long one nothing about which conversation they
    are in. It goes back to the word when the root could not be fetched, which
    a redaction and a missing key both look like.

    Remembered rather than worked out on every render, because the panel
    redraws on every keystroke in the box below it and this reads a message
    that has not changed.
  */
  const topic = useMemo(
    () => (thread?.root === undefined ? "Thread" : previewOf(thread.root, nameOf)),
    [thread?.root, nameOf],
  );
  const replies = useMemo(
    () => group(thread?.messages ?? []),
    [thread?.messages],
  );
  const root = useMemo(
    () => (thread?.root === undefined ? [] : group([thread.root])),
    [thread?.root],
  );
  /*
    What a reply in here may point at. The root as well as the replies: the
    first answer in a thread names the message it hangs from, and that is drawn
    above the rule rather than in the list.
  */
  const known = useMemo(() => {
    const everything = [...(thread?.root ? [thread.root] : []), ...(thread?.messages ?? [])];
    return new Map(everything.map((message) => [message.id, message]));
  }, [thread?.root, thread?.messages]);

  /**
   * Follow the pointer until it is let go.
   *
   * On `window` rather than through `setPointerCapture`, which jsdom does not
   * implement, so a drag would be the one thing here no test could reach.
   * Capture would also be the wrong shape: what is being dragged is the edge
   * of the panel, not the seven pixels the hand landed on.
   */
  function grab(event: ReactPointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const from = event.clientX;
    const started = width;

    const move = (moved: PointerEvent) => {
      // The panel is on the right, so the pointer moving left widens it.
      onResize(clampThreadWidth(started + (from - moved.clientX)));
    };
    const drop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", drop);
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", drop);
  }

  /** The same edge, for a keyboard. A splitter only a mouse can move is half a
   * control. */
  function nudge(event: ReactKeyboardEvent<HTMLDivElement>) {
    const by =
      event.key === "ArrowLeft" ? STEP : event.key === "ArrowRight" ? -STEP : 0;
    if (by === 0) return;

    event.preventDefault();
    onResize(clampThreadWidth(width + by));
  }

  /**
   * React to something in the panel, or take that reaction back.
   *
   * Declared here rather than inline at the two call sites below, because the
   * root and the replies are two `MessageGroups` and a reaction works the same
   * way in both.
   */
  function react(eventId: string, key: string, mine: string | undefined) {
    if (thread === null) return;
    const done =
      mine === undefined
        ? timelineReact(thread.roomId, eventId, key)
        : timelineUnreact(thread.roomId, mine);
    void done.catch((raw: unknown) => {
      setProblem(asCommandError(raw).message);
    });
  }

  /** Answer one reply in the thread, with the box ready for what comes next. */
  function reply(message: Message) {
    setAnswering(message);
    setEditing(null);
    draftBox.current?.focus();
  }

  /**
   * Correct a message, with the box opened on what it currently says.
   *
   * Opened on the sentence rather than empty, because almost every edit is one
   * word in a sentence somebody otherwise meant. What goes back is the
   * plaintext: `html` is what the sender's markdown became, and putting tags
   * in the box would have somebody editing the rendering of their message.
   */
  function edit(message: Message) {
    setEditing(message);
    setAnswering(null);
    setDraft(message.body);
    draftBox.current?.focus();
  }

  /**
   * Put the composer back to an ordinary reply.
   *
   * The box is emptied as well, unlike calling off a reply. What is in there
   * is the old message rather than something somebody typed, so leaving it
   * would put a sentence already in the thread into the next reply.
   *
   * Which is why it does nothing at all when no correction is in progress.
   * Escape and a thread closing both reach this whatever the box holds, and
   * emptying it either time would throw away a half-written reply.
   */
  function stopEditing() {
    if (editingNow.current === null) return;
    setEditing(null);
    setDraft("");
  }

  /**
   * Delete a message this account sent.
   *
   * Reached once the question hanging off the control has been answered, so
   * this sends the redaction rather than asking again.
   *
   * The composer is put back first when it was pointed at this message.
   * Pressing Edit and then Delete is two presses apart, and what it would
   * otherwise leave behind is a correction addressed to an event with nothing
   * left to correct.
   *
   * Nothing is echoed, on the same terms as every other send: the words stay
   * on screen until the sync brings the redaction back.
   */
  function remove(message: Message) {
    // The same guard `copyLink` carries. A panel with no thread in it draws
    // nothing, so nothing in it can be pressed, but the room ID is read off
    // the thread and the compiler is right that it could be absent.
    if (thread === null) return;
    if (editingNow.current?.id === message.id) stopEditing();
    // The box is left alone here, unlike the line above. What is in it while
    // answering is something somebody typed rather than a copy of the old
    // message, and it is still worth sending somewhere else.
    if (answering?.id === message.id) setAnswering(null);
    void timelineDelete(thread.roomId, message.id).catch((raw: unknown) => {
      setProblem(asCommandError(raw).message);
    });
  }

  /**
   * Put one reply's address on the clipboard.
   *
   * A reply in a thread has an address like anything else said in the room, and
   * the panel is where somebody reading a long thread wants to link to one line
   * of it rather than to the message the whole thing hangs from.
   */
  function copyLink(eventId: string) {
    if (thread === null) return;
    void timelineCopyLink(thread.roomId, eventId)
      .then(() => setCopied(eventId))
      .catch((raw: unknown) => {
        setProblem(asCommandError(raw).message);
      });
  }

  if (thread === null) return null;

  /*
    What a reply points at when it is answering nobody in particular: the last
    thing said in the thread, or the message it hangs from when nobody has said
    anything yet. That is the fallback a client with no idea about threads
    draws, and nothing about which thread the reply lands in depends on it.
  */
  const latest = thread.messages.at(-1)?.id ?? thread.rootId;

  async function send() {
    if (thread === null || draft.trim() === "" || sending) return;

    setSending(true);
    setProblem(null);
    try {
      // A correction replaces a message that is already in the room, so it is
      // the room's command rather than the thread's: an edit carries no
      // thread relation of its own and is folded onto whatever it names.
      if (editing !== null) {
        await timelineEdit(thread.roomId, editing.id, draft);
      } else {
        await threadSend(
          thread.roomId,
          thread.rootId,
          answering?.id ?? latest,
          answering?.sender ?? null,
          draft,
        );
      }
      // Cleared only once the homeserver has it. A box that empties on a send
      // that failed loses what somebody wrote.
      setDraft("");
      setAnswering(null);
      setEditing(null);
    } catch (raw: unknown) {
      setProblem(asCommandError(raw).message);
    } finally {
      setSending(false);
    }
  }

  return (
    <aside className="thread" aria-label="Thread" style={{ width }}>
      {/*
        The edge, as something to take hold of. A separator rather than a
        button, because that is what it is, and focusable so the arrows work.
      */}
      <div
        className="thread__grip"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize the thread panel"
        aria-valuenow={width}
        aria-valuemin={NARROWEST}
        aria-valuemax={Math.round(window.innerWidth * MOST)}
        tabIndex={0}
        onPointerDown={grab}
        onKeyDown={nudge}
      />
      <div className="thread__head">
        <h2 className="thread__name">{topic}</h2>
        <button
          type="button"
          className="thread__close"
          aria-label="Close thread"
          onClick={() => void threadOpen(null).catch(() => {})}
        >
          &times;
        </button>
      </div>

      <div className="thread__scroll" ref={scroller} onScroll={remember}>
        {/*
          The message it hangs from, above a rule rather than in the list. It
          is what the replies are about rather than the first of them, and a
          thread whose root is indistinguishable from its replies reads as a
          conversation starting halfway through.
        */}
        {root.length > 0 && (
          <div className="thread__root">
            <MessageGroups
              groups={root}
              names={names}
              roomId={thread.roomId}
              selfId={selfId}
              known={known}
              container={scroller}
              copiedId={copied}
              onAbout={(person, at) => setOpened({ person, at })}
              onReply={reply}
              onEdit={edit}
              onDelete={remove}
              onReact={react}
              onCopyLink={copyLink}
            />
          </div>
        )}

        {thread.moreBefore && (
          <p className="thread__more">
            Earlier replies in this thread are not loaded yet.
          </p>
        )}

        {/*
          No way further in. Every message here is already in the thread being
          read, so a control offering to open one would go nowhere.
        */}
        <MessageGroups
          groups={replies}
          names={names}
          roomId={thread.roomId}
          selfId={selfId}
          known={known}
          container={scroller}
          copiedId={copied}
          onAbout={(person, at) => setOpened({ person, at })}
          onReply={reply}
          onEdit={edit}
          onDelete={remove}
          onReact={react}
          onCopyLink={copyLink}
        />
      </div>

      {problem !== null && (
        <p className="thread__problem" role="alert">
          {problem}
        </p>
      )}

      {/*
        What the next reply will answer, when it is one line of the thread
        rather than the thread itself. Without one it answers the conversation,
        which is what the box does when nothing has been pressed.
      */}
      {answering !== null && (
        <ComposerTarget
          doing="reply"
          who={names[answering.sender] ?? answering.sender}
          message={answering}
          onStop={() => setAnswering(null)}
        />
      )}

      {/* The same line, for the other thing the box can be pointed at. */}
      {editing !== null && (
        <ComposerTarget doing="edit" message={editing} onStop={stopEditing} />
      )}

      <form
        className="thread__composer"
        onSubmit={(event) => {
          event.preventDefault();
          void send();
        }}
      >
        <label className="thread__label" htmlFor="thread-draft">
          Reply in this thread
        </label>
        <textarea
          id="thread-draft"
          className="thread__draft"
          ref={draftBox}
          rows={1}
          value={draft}
          placeholder="Reply in this thread"
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            /*
              Escape puts the box back to an ordinary reply. Stopped where it
              is caught, because the listener that shuts the panel is on the
              window one step further out, and without this one press would
              both abandon the correction and close the thread it was in.
            */
            if (
              event.key === "Escape" &&
              (answering !== null || editing !== null)
            ) {
              event.stopPropagation();
              setAnswering(null);
              stopEditing();
              return;
            }
            // Enter sends and Shift+Enter breaks the line, the same as the
            // room's box. Two boxes on one screen behaving differently is
            // worse than either behaviour on its own.
            if (event.key !== "Enter" || event.shiftKey) return;
            event.preventDefault();
            void send();
          }}
        />
        {/*
          "Send" rather than "Reply", which is what this said while it was the
          only control in the panel that could mean anything. Every message in
          here now has a Reply of its own, and two controls with one word on
          them are two controls nobody can tell apart.
        */}
        <button
          type="submit"
          className="thread__send"
          disabled={draft.trim() === "" || sending}
        >
          Send
        </button>
      </form>

      {opened !== null && (
        <PersonMenu
          key={opened.person.id}
          person={opened.person}
          roomId={thread.roomId}
          selfId={selfId}
          at={opened.at}
          onClose={() => setOpened(null)}
          onOpenRoom={onOpenRoom}
        />
      )}
    </aside>
  );
}
