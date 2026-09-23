/**
 * Where the shell is, and how it gets back to where it was.
 *
 * The window's own history rather than a stack of our own. Those entries are
 * what the webview traverses whenever anything else asks it to, so keeping the
 * places in them leaves one history rather than two that can disagree about
 * where Back goes.
 *
 * Each entry carries the place it is for, so coming back is reading the answer
 * off the entry rather than counting steps. Nothing is written into the URL:
 * the page is served from a custom protocol whose address is the application
 * itself, and a room ID in it would be a second place to keep the selection.
 */
import { useCallback, useEffect, useRef, useState } from "react";

/** Somewhere the shell can be: a rail entry, and a room in it or none. */
export interface Where {
  spaceId: string;
  channelId: string | null;
}

/**
 * The key a place is filed under inside a history entry.
 *
 * Named rather than stored bare, because `history.state` is one object and an
 * entry that predates the shell being mounted has to be recognisable as not
 * ours. There is nowhere to go back to on one of those, and moving the screen
 * anyway would be inventing a destination.
 */
const WHERE = "consortWhere";

function samePlace(one: Where, other: Where): boolean {
  return one.spaceId === other.spaceId && one.channelId === other.channelId;
}

/** The place an entry carries, or null when the entry is not one of ours. */
function whereIn(state: unknown): Where | null {
  if (state === null || typeof state !== "object") return null;
  return (state as { [WHERE]?: Where })[WHERE] ?? null;
}

/**
 * The place the shell is at, and the way to send it somewhere else.
 *
 * Shaped like `useState` because that is what it replaces, and because every
 * caller either reads the current place or moves to a new one. The difference
 * is that moving is recorded: what came before stays reachable.
 */
export function useHistory(initial: Where): [Where, (next: Where) => void] {
  const [where, setWhere] = useState(initial);
  /*
    Where we are, for the sake of the next move. A ref as well as state because
    pushing an entry is a side effect, and an updater runs twice under
    StrictMode: deciding inside one would file the same room under two entries.
  */
  const at = useRef(where);

  /*
    The entry the window loaded with carries nothing, so a Back arriving at it
    would be a traversal with no place to restore. Marking it on mount is what
    makes wherever the shell opened somewhere it can return to.
  */
  useEffect(() => {
    window.history.replaceState({ [WHERE]: at.current }, "");
  }, []);

  /*
    A traversal, however it was asked for. The two extra buttons on a mouse
    are deliberately not handled here: wry inhibits them at the GTK level and
    dispatches a `mousedown` and a `mouseup` of its own for them, then
    traverses on the `mouseup` unless the page cancelled that event. Doing it
    here as well moved two entries per press, which somebody pressing Back saw
    as the room behind flashing up and the empty pane settling in its place.
    `wry/src/webkitgtk/synthetic_mouse_events.rs` is where that is written.
  */
  useEffect(() => {
    function arrive(event: PopStateEvent) {
      const there = whereIn(event.state);
      if (there === null) return;
      at.current = there;
      setWhere(there);
    }

    window.addEventListener("popstate", arrive);
    return () => window.removeEventListener("popstate", arrive);
  }, []);

  const goTo = useCallback((next: Where) => {
    // Clicking the room you are reading is not somewhere new. An entry for it
    // would make the first press of Back a button that appears to do nothing.
    if (samePlace(at.current, next)) return;
    at.current = next;
    window.history.pushState({ [WHERE]: next }, "");
    setWhere(next);
  }, []);

  return [where, goTo];
}
