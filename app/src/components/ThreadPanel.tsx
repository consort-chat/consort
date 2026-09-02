import { useEffect, useMemo, useState } from "react";

import {
  asCommandError,
  memberNames,
  onThread,
  resendState,
  threadOpen,
  threadSend,
  type Participant,
  type Thread,
} from "../lib/api";
import { MessageGroups, group } from "./MessageGroups";
import { PersonMenu } from "./PersonMenu";
import "./ThreadPanel.css";

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
}: {
  /** Whoever is signed in, so a person's card can tell when it is about them. */
  selfId: string;
  /** Show a room, by ID. Passed to a person's card for its Message button. */
  onOpenRoom: (roomId: string) => void;
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

  const replies = useMemo(
    () => group(thread?.messages ?? []),
    [thread?.messages],
  );
  const root = useMemo(
    () => (thread?.root === undefined ? [] : group([thread.root])),
    [thread?.root],
  );

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
    <aside className="thread" aria-label="Thread">
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

      <div className="thread__scroll">
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
              onAbout={(person, at) => setOpened({ person, at })}
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
          onAbout={(person, at) => setOpened({ person, at })}
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
