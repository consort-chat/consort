import { useEffect, useLayoutEffect, useRef, useState } from "react";

import {
  asCommandError,
  audioSettings,
  directRoom,
  memberProfile,
  setPersonVolume,
  type MemberProfile,
  type Participant,
} from "../lib/api";
import { elapsedLabel, presenceLabel } from "../lib/labels";
import { RoomAvatar } from "./RoomAvatar";
import "./PersonMenu.css";

/** Full volume, which is also what somebody nobody has adjusted is at. */
const FULL = 100;

/**
 * As loud as one person can be made.
 *
 * Past full rather than up to it, because the case this control is for is
 * somebody who arrives too quiet rather than too loud. A laptop microphone
 * across a room comes in well under everybody else, and the only repair a
 * slider that stopped at full could offer was to turn the rest of the call
 * down to meet them, which makes four voices worse to fix one.
 *
 * The percentage is slider travel and not amplitude, here as everywhere else
 * on this control: the curve is squared, so half is already a quarter and this
 * top is a little over six times. It matches
 * `consort_audio::MAX_PERSON_VOLUME`, which is where it is actually enforced.
 */
const LOUDEST = 250;

/** How long to wait after a slider stops moving before writing it down. */
const SETTLE_MS = 150;

/** How far to keep the card from the edge of the window. */
const GAP = 8;

export interface PersonMenuProps {
  /**
   * Who this is about, straight off the roster.
   *
   * The whole participant rather than an ID and a name, because everything the
   * roster already knows is drawn here and asking for it again would be a
   * second request for facts that are on screen behind this panel.
   */
  person: Participant;
  /** The channel they are in, which is half of the key their avatar takes. */
  roomId: string;
  /**
   * Whoever is signed in, so the card can tell when it is about them.
   *
   * The card opens from a name in a room and your own name is one of the names
   * in it, so without this the Message button would offer to make a
   * note-to-self room by accident.
   */
  selfId: string;
  /**
   * Where to put its top left corner, in viewport coordinates.
   *
   * A request rather than an instruction. It is clamped against the window
   * below, so a card asked for near an edge comes back inside one.
   */
  at: { x: number; y: number };
  onClose: () => void;
  /**
   * Show a room, by ID.
   *
   * The card has no idea where a room lives in the rail, and it should not:
   * the shell owns both selections, and this hands it a room ID and lets it
   * work out which space that is under.
   */
  onOpenRoom: (roomId: string) => void;
}

/**
 * What one person's name in a voice channel opens.
 *
 * Two things somebody wants from a name in a list, in the order they want
 * them. Who is this, and then, occasionally, change how loud they are.
 *
 * ## What is on it, and what is deliberately not
 *
 * Everything drawn here is something a server or a call actually said.
 * Presence comes from the homeserver, the join time from the SFU's own record,
 * and the call state from the roster this panel was opened out of. Nothing is
 * inferred and nothing is invented: where the answer is not known the card
 * says so, because most homeservers have presence switched off and a card that
 * quietly drew "Offline" for that would be putting a grey dot on somebody
 * sitting right there.
 *
 * A power level is deliberately not on it. The card used to badge one, and it
 * read "Admin" for everybody, because a room whose `m.room.power_levels` has
 * never been fetched hands back the creator's level for whoever is asked
 * about. A label that is the same for every person is a label carrying no
 * information, and this one was carrying something false while it did it.
 *
 * Messaging is the one thing on it that leaves the card. It opens the direct
 * message with whoever this is about, making the room if the account has never
 * had one with them, and hands the room to the shell to select. See
 * `consort_matrix::rooms::direct` for why it creates rather than refusing.
 *
 * ## Why the volume lives here rather than in the account
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
 * One request each, when a card opens, against threading a map of levels and a
 * map of profiles through every component between here and the top of the
 * sidebar for the sake of a panel that is closed almost all of the time.
 */
export function PersonMenu({
  person,
  roomId,
  selfId,
  at,
  onClose,
  onOpenRoom,
}: PersonMenuProps) {
  const [percent, setPercent] = useState<number | null>(null);
  const [profile, setProfile] = useState<MemberProfile | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);
  const [placement, setPlacement] = useState({ left: at.x, top: at.y });
  const menu = useRef<HTMLDivElement | null>(null);
  const slider = useRef<HTMLInputElement | null>(null);
  const writing = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pending = useRef<number | null>(null);
  const userId = person.id;

  useEffect(() => {
    let cancelled = false;

    void audioSettings()
      .then((settings) => {
        if (cancelled) return;
        setPercent(settings.personVolumes?.[userId] ?? FULL);
      })
      .catch((raw: unknown) => {
        if (cancelled) return;
        // Drawn at full rather than left blank. A slider that never resolves
        // is worse than one showing the value almost everybody is at.
        setPercent(FULL);
        setProblem(asCommandError(raw).message);
      });

    return () => {
      cancelled = true;
    };
  }, [userId]);

  // The one request this panel makes of the homeserver. Its failure is not
  // reported: the command already degrades every part of the answer to
  // "nothing known" on its own, and this catch is for the case where there is
  // no signed-in client at all, which the rest of the window is already
  // shouting about.
  useEffect(() => {
    let cancelled = false;

    void memberProfile(userId)
      .then((profile) => {
        if (cancelled) return;
        setProfile(profile);
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, [userId]);

  // Escape and a click elsewhere both close it, which is what every menu on
  // every platform does. `pointerdown` rather than `click`, so that the card
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

  // Measured rather than assumed. The card's height depends on what is known
  // about the person, so it grows when the profile lands, and a constant here
  // would be a guess that goes stale the next time a line is added. Before
  // paint, so nothing is drawn in the wrong place first.
  useLayoutEffect(() => {
    const node = menu.current;
    if (node === null) return;
    const box = node.getBoundingClientRect();
    setPlacement({
      left: Math.max(GAP, Math.min(at.x, window.innerWidth - box.width - GAP)),
      top: Math.max(GAP, Math.min(at.y, window.innerHeight - box.height - GAP)),
    });
  }, [at.x, at.y, percent, profile]);

  // Focus lands on the one control that does anything, so the card is usable
  // from the keyboard the moment it opens rather than after several tabs from
  // wherever focus happened to be in the sidebar.
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

  /**
   * Open the direct message with this person.
   *
   * The card closes only on success. A failure leaves it up with the reason on
   * it, because a card that vanished would take the explanation with it and
   * the press would read as a button that does nothing.
   */
  function message() {
    setOpening(true);
    setProblem(null);
    directRoom(userId)
      .then((roomId) => {
        onOpenRoom(roomId);
        onClose();
      })
      .catch((raw: unknown) => {
        setOpening(false);
        setProblem(asCommandError(raw).message);
      });
  }

  const states = callStates(person);

  return (
    <div
      ref={menu}
      className="person-menu"
      role="dialog"
      aria-label={person.name}
      style={{ left: placement.left, top: placement.top }}
    >
      <div className="person-menu__header">
        <RoomAvatar
          roomId={roomId}
          userId={person.id}
          name={person.name}
          className="person-menu__face"
        />
        <div className="person-menu__identity">
          <p className="person-menu__who">{person.name}</p>
          {/*
            Under the display name rather than instead of it. Two people in a
            room can carry the same display name, and the user ID is the only
            thing on this card that tells them apart.
          */}
          <p className="person-menu__id">{person.id}</p>
        </div>
      </div>

      <dl className="person-menu__facts">
        <div className="person-menu__fact">
          <dt className="person-menu__label">Status</dt>
          <dd className="person-menu__detail">
            {profile === null ? (
              "Checking…"
            ) : (
              <>
                <span
                  className="person-menu__dot"
                  data-presence={profile.presence}
                  aria-hidden="true"
                />
                {presenceLabel(profile.presence)}
              </>
            )}
          </dd>
        </div>
        {profile?.status != null && profile.status !== "" && (
          <div className="person-menu__fact">
            <dt className="person-menu__label">Says</dt>
            <dd className="person-menu__detail">{profile.status}</dd>
          </div>
        )}
        {person.since !== undefined && (
          <div className="person-menu__fact">
            <dt className="person-menu__label">In call</dt>
            <dd className="person-menu__detail">
              {elapsedLabel(person.since, Date.now())}
            </dd>
          </div>
        )}
        {states.length > 0 && (
          <div className="person-menu__fact">
            <dt className="person-menu__label">Right now</dt>
            <dd className="person-menu__detail">{states.join(", ")}</dd>
          </div>
        )}
      </dl>

      <button
        type="button"
        className="person-menu__action"
        disabled={person.id === selfId || opening}
        onClick={message}
      >
        Message
      </button>

      <hr className="person-menu__rule" />

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
            max={LOUDEST}
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

/**
 * The call state the row draws as icons, written out.
 *
 * Not a precedence, unlike the row. There the three flags compete for one
 * slot, so the strongest claim wins and the others are dropped; here there is
 * room to say all of them, and "deafened and away" is a different fact from
 * either one alone.
 */
function callStates(person: Participant): string[] {
  const states: string[] = [];
  if (person.deafened === true) states.push("Deafened");
  if (person.away === true) states.push("Away");
  if (person.muted === true) states.push("Muted");
  if (person.camera === true) states.push("Camera on");
  return states;
}
