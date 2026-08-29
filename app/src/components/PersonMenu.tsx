import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { asCommandError, audioSettings, setPersonVolume } from "../lib/api";
import "./PersonMenu.css";

/** Full volume, which is also what somebody nobody has adjusted is at. */
const FULL = 100;

/** How long to wait after a slider stops moving before writing it down. */
const SETTLE_MS = 150;

/** Roughly the menu's own size, for keeping it on screen. */
const MENU_WIDTH = 232;
const MENU_HEIGHT = 128;

export interface PersonMenuProps {
  /** The Matrix user ID, which is what the level is remembered against. */
  userId: string;
  /** What to call them, for the heading. */
  name: string;
  /** Where the pointer was, in viewport coordinates. */
  at: { x: number; y: number };
  onClose: () => void;
}

/**
 * What one person's name in a voice channel opens.
 *
 * Only a volume, for now, and that is the whole reason it exists: there is
 * nowhere else to put "this one is too loud in my headphones". A master volume
 * cannot say it, because the problem is one person against the others, and the
 * settings screen cannot say it either, because it does not know who is in the
 * call.
 *
 * ## Why the level lives here rather than in the account
 *
 * There is no Matrix account data for it and there should not be. How loud
 * somebody sounds is a fact about the headphones on and the room being sat in,
 * so it belongs to the machine rather than to the account, and carrying it
 * between them would be carrying the wrong thing. It is kept in the settings
 * file, so it survives leaving the call, rejoining, and closing the
 * application for a week.
 *
 * ## Why it loads its own settings
 *
 * One request, when a menu opens, against threading a map of levels through
 * every component between here and the top of the sidebar for the sake of a
 * panel that is closed almost all of the time.
 */
export function PersonMenu({ userId, name, at, onClose }: PersonMenuProps) {
  const [percent, setPercent] = useState<number | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const menu = useRef<HTMLDivElement | null>(null);
  const slider = useRef<HTMLInputElement | null>(null);
  const writing = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pending = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;

    void audioSettings()
      .then((settings) => {
        if (cancelled) return;
        setPercent(settings.personVolumes?.[userId] ?? FULL);
      })
      .catch((raw: unknown) => {
        if (cancelled) return;
        // Drawn at full rather than left blank. A menu that never resolves is
        // worse than one showing the value almost everybody is at.
        setPercent(FULL);
        setProblem(asCommandError(raw).message);
      });

    return () => {
      cancelled = true;
    };
  }, [userId]);

  // Escape and a click elsewhere both close it, which is what every menu on
  // every platform does. `pointerdown` rather than `click`, so that the menu
  // is gone before whatever was underneath it reacts.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    function onDown(event: PointerEvent) {
      const node = menu.current;
      if (node !== null && event.target instanceof Node && node.contains(event.target)) {
        return;
      }
      onClose();
    }

    document.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onDown);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onDown);
    };
  }, [onClose]);

  // Whatever the slider was last left at, written out on the way out. Without
  // this, dragging and immediately pressing Escape loses the change, which
  // reads as a control that does not work.
  useEffect(
    () => () => {
      if (writing.current === null) return;
      clearTimeout(writing.current);
      const last = pending.current;
      if (last !== null) void setPersonVolume(userId, last);
    },
    [userId],
  );

  // Focus lands on the one control, so the menu is usable from the keyboard
  // the moment it opens rather than after several tabs from wherever focus
  // happened to be in the sidebar.
  useLayoutEffect(() => {
    if (percent !== null) slider.current?.focus();
  }, [percent]);

  function change(next: number) {
    setPercent(next);
    pending.current = next;
    if (writing.current !== null) clearTimeout(writing.current);
    writing.current = setTimeout(() => {
      writing.current = null;
      const value = pending.current;
      pending.current = null;
      if (value === null) return;
      void setPersonVolume(userId, value).catch((raw: unknown) => {
        setProblem(asCommandError(raw).message);
      });
    }, SETTLE_MS);
  }

  // Clamped so a name near the bottom of a long sidebar does not open a menu
  // half off the screen. Measured against the window rather than the sidebar,
  // because the menu is positioned against the window.
  const left = Math.min(at.x, Math.max(0, window.innerWidth - MENU_WIDTH - 8));
  const top = Math.min(at.y, Math.max(0, window.innerHeight - MENU_HEIGHT - 8));

  return (
    <div
      ref={menu}
      className="person-menu"
      role="dialog"
      aria-label={`${name}'s volume`}
      style={{ left, top }}
    >
      <p className="person-menu__who">{name}</p>
      {percent === null ? (
        <p className="person-menu__note">Reading the saved volume…</p>
      ) : (
        <>
          <div className="person-menu__row">
            <label className="person-menu__label" htmlFor="person-menu-volume">
              Volume
            </label>
            {/*
              `aria-hidden`, because the slider announces its own value and
              reading the same number twice is noise. This one is for the eye.
            */}
            <output
              className="person-menu__value"
              htmlFor="person-menu-volume"
              aria-hidden="true"
            >
              {percent}%
            </output>
          </div>
          <input
            ref={slider}
            id="person-menu-volume"
            className="person-menu__slider"
            type="range"
            min={0}
            max={100}
            step={1}
            value={percent}
            onChange={(event) => change(Number(event.target.value))}
          />
          <p className="person-menu__note">
            {percent === FULL
              ? "Just for you, on this computer."
              : "Just for you, on this computer. Remembered for next time."}
          </p>
        </>
      )}
      {problem !== null && (
        <p className="person-menu__note person-menu__note--warn" role="alert">
          {problem}
        </p>
      )}
    </div>
  );
}
