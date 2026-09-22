import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from "react";

/**
 * How far to keep a floating thing from the edge of the window.
 *
 * The same eight pixels `PersonMenu` keeps, and deliberately the same number
 * rather than a second one that happens to be close: the two are the only
 * things in this application that float, and a card that stopped a different
 * distance from the edge than the menu it opens would read as a mistake.
 */
const GAP = 8;

/** How far one arrow press moves it. Far enough to see, small enough to aim. */
const STEP = 16;

/**
 * How far a pointer has to travel before the gesture stops being a click.
 *
 * A press always jitters a pixel or two, so without this every attempt to
 * double-click the card would be read as a drag by the hand that made it.
 */
const SLOP = 3;

/** Which way each arrow goes, in the units the step is measured in. */
const ARROWS: Record<string, { x: number; y: number } | undefined> = {
  ArrowLeft: { x: -1, y: 0 },
  ArrowRight: { x: 1, y: 0 },
  ArrowUp: { x: 0, y: -1 },
  ArrowDown: { x: 0, y: 1 },
};

/** Where something has been put, in the coordinates `position: fixed` takes. */
export interface Place {
  left: number;
  top: number;
}

export interface Draggable<T extends HTMLElement> {
  /**
   * Where it has been moved to, or null while it is still where CSS put it.
   *
   * Null is not a position of its own: it is the absence of one, which is what
   * lets the stylesheet own the opening place and follow the window for free.
   */
  at: Place | null;
  /** Goes on the thing being moved, which is what gets measured. */
  ref: RefObject<T | null>;
  /** Goes on the handle, which is the part that can be taken hold of. */
  handle: {
    onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
    onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void;
  };
  /**
   * Whether the press that just ended was a drag rather than a click.
   *
   * Asked in a click handler, after the gesture is over. A ref rather than
   * state, because nothing is drawn differently for it and a re-render several
   * times a second during a drag is exactly what this is trying to avoid.
   *
   * Only ever about the last press. Every new one clears it, wherever it
   * landed, so a drag cannot go on refusing clicks somewhere else on the page
   * for as long as nobody touches the handle again.
   */
  dragged: () => boolean;
  /**
   * Pull it back inside the window after it has changed size.
   *
   * The resize below covers a window that shrank. This is for the other half:
   * something that grew while the window stayed still, which walks off the
   * right edge just as surely and has nothing to bring it back.
   */
  keepInView: () => void;
}

/** Keep a box inside the window, the way `PersonMenu` keeps its card inside. */
function fit(place: Place, box: { width: number; height: number }): Place {
  return {
    left: Math.max(GAP, Math.min(place.left, window.innerWidth - box.width - GAP)),
    top: Math.max(GAP, Math.min(place.top, window.innerHeight - box.height - GAP)),
  };
}

/**
 * Move something around the window with a pointer or with the arrow keys.
 *
 * Both, rather than either. A control that only exists as a drag target is a
 * control some people cannot reach, and the keyboard half costs one handler
 * once the position is state.
 *
 * The move listeners go on `window` rather than through `setPointerCapture`,
 * for the reason `ThreadPanel` already gives: jsdom does not implement pointer
 * capture, so a drag would be the one thing here no test could reach. It does
 * the same job, which is that a hand moving faster than the card keeps hold of
 * it instead of dropping it the moment the cursor is outside.
 */
export function useDraggable<
  T extends HTMLElement = HTMLElement,
>(): Draggable<T> {
  const [at, setAt] = useState<Place | null>(null);
  const node = useRef<T | null>(null);
  const travelled = useRef(false);
  // The gesture in flight, so that whatever is holding it can be let go of
  // from somewhere other than a `pointerup`.
  const release = useRef<(() => void) | null>(null);

  /*
    Only for something that has been moved: a card still where the stylesheet
    put it is anchored in CSS and follows the window on its own, and correcting
    that would be this hook overruling a position it was never given.
  */
  const keepInView = useCallback(() => {
    const box = node.current?.getBoundingClientRect();
    if (box === undefined) return;
    setAt((current) => {
      if (current === null) return null;
      const next = fit(current, box);
      // The same place said again is not a change. Answering with a fresh
      // object either way would be a render per window resize event, and a
      // render per render for anything that measures after one.
      return next.left === current.left && next.top === current.top
        ? current
        : next;
    });
  }, []);

  useEffect(() => {
    window.addEventListener("resize", keepInView);
    return () => window.removeEventListener("resize", keepInView);
  }, [keepInView]);

  /*
    A press is the start of a new gesture, wherever it lands. Without this the
    last drag's verdict stands until the handle is next taken hold of, and a
    double-click anywhere else goes on being refused by a drag that finished
    several minutes ago.

    On the way down, so it is settled before the handle's own listener runs.
  */
  useEffect(() => {
    function fresh() {
      travelled.current = false;
    }

    window.addEventListener("pointerdown", fresh, true);
    return () => window.removeEventListener("pointerdown", fresh, true);
  }, []);

  /*
    A card can go while the pointer is still down on it: the call ends, the
    card is not drawn, and the hand is still holding the handle. Without this
    its listeners outlive it and keep answering a pointer about a card that is
    no longer there.
  */
  useEffect(() => () => release.current?.(), []);

  function grab(event: ReactPointerEvent<HTMLElement>) {
    const box = node.current?.getBoundingClientRect();
    if (box === undefined) return;

    // Otherwise the drag selects the handle's text, and the card ends up
    // moving with a blue smear across its title.
    event.preventDefault();
    /*
      Which is also what stops the press focusing the handle, because a
      prevented `pointerdown` never becomes a `mousedown`. Put back by hand:
      the arrow keys are the other half of this control, and a handle that has
      to be tabbed to after being clicked is a handle whose keyboard half
      nobody will find.
    */
    event.currentTarget.focus();

    const from = { x: event.clientX, y: event.clientY };
    const origin = at ?? { left: box.left, top: box.top };
    // Taken apart here rather than read off the box inside the handler: a
    // function declaration is hoisted, so the narrowing above does not reach
    // into it.
    const size = { width: box.width, height: box.height };

    function move(moved: PointerEvent) {
      const by = { x: moved.clientX - from.x, y: moved.clientY - from.y };
      if (Math.abs(by.x) > SLOP || Math.abs(by.y) > SLOP) {
        travelled.current = true;
      }
      setAt(fit({ left: origin.left + by.x, top: origin.top + by.y }, size));
    }

    function drop() {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", drop);
      window.removeEventListener("pointercancel", drop);
      release.current = null;
    }

    release.current = drop;
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", drop);
    /*
      A touch the system takes back, which is how a drag ends when a gesture
      is recognised over it or the window loses the pointer. It is the one
      ending that never sends a `pointerup`, so without it the card is left
      following a finger that has gone.
    */
    window.addEventListener("pointercancel", drop);
  }

  function nudge(event: ReactKeyboardEvent<HTMLElement>) {
    const way = ARROWS[event.key];
    if (way === undefined) return;

    const box = node.current?.getBoundingClientRect();
    if (box === undefined) return;

    // The window scrolls under an arrow key otherwise, which would move the
    // page instead of the card and look like the handle doing nothing.
    event.preventDefault();

    const origin = at ?? { left: box.left, top: box.top };
    setAt(
      fit(
        { left: origin.left + way.x * STEP, top: origin.top + way.y * STEP },
        box,
      ),
    );
  }

  return {
    at,
    ref: node,
    handle: { onPointerDown: grab, onKeyDown: nudge },
    dragged: () => travelled.current,
    keepInView,
  };
}
