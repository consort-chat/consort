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
  timelineReact,
  timelineUnreact,
  type Participant,
  type Thread,
} from "../lib/api";
import { MessageGroups, group } from "./MessageGroups";
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
  width,
  onResize,
}: {
  /** Whoever is signed in, so a person's card can tell when it is about them. */
  selfId: string;
  /** Show a room, by ID. Passed to a person's card for its Message button. */
  onOpenRoom: (roomId: string) => void;
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
  const [sending, setSending] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  /* The reply whose address has just gone to the clipboard, or none. */
  const [copied, setCopied] = useState<string | null>(null);
  // The scrolling box, so a jump to an answered message is looked for in this
  // panel rather than in the room beside it, which draws the root as well.
  const scroller = useRef<HTMLDivElement>(null);
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
    What the reply is answering, for the fallback a client with no idea about
    threads draws. The last thing said in the thread, or the message it hangs
    from when nobody has said anything yet.
  */
  const answering = thread.messages.at(-1)?.id ?? thread.rootId;

  async function reply() {
    if (thread === null || draft.trim() === "" || sending) return;

    setSending(true);
    setProblem(null);
    try {
      await threadSend(thread.roomId, thread.rootId, answering, draft);
      // Cleared only once the homeserver has it. A box that empties on a send
      // that failed loses what somebody wrote.
      setDraft("");
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
        <h2 className="thread__name">Thread</h2>
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
          onReact={react}
          onCopyLink={copyLink}
        />
      </div>

      {problem !== null && (
        <p className="thread__problem" role="alert">
          {problem}
        </p>
      )}

      <form
        className="thread__composer"
        onSubmit={(event) => {
          event.preventDefault();
          void reply();
        }}
      >
        <label className="thread__label" htmlFor="thread-draft">
          Reply in this thread
        </label>
        <textarea
          id="thread-draft"
          className="thread__draft"
          rows={1}
          value={draft}
          placeholder="Reply in this thread"
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            // Enter sends and Shift+Enter breaks the line, the same as the
            // room's box. Two boxes on one screen behaving differently is
            // worse than either behaviour on its own.
            if (event.key !== "Enter" || event.shiftKey) return;
            event.preventDefault();
            void reply();
          }}
        />
        <button
          type="submit"
          className="thread__send"
          disabled={draft.trim() === "" || sending}
        >
          Reply
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
