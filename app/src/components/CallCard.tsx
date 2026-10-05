import {
  useEffect,
  useLayoutEffect,
  useState,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";

import { NOBODY, type Call, type Participant } from "../lib/api";
import { callLabel } from "../lib/labels";
import { useDraggable } from "../lib/useDraggable";
import { CallFace } from "./CallFace";
import { PersonMenu } from "./PersonMenu";
import { ScreenStage } from "./ScreenStage";
import { ScreenTile } from "./ScreenTile";
import { SelfPicture } from "./SelfPicture";
import { TheirPicture } from "./TheirPicture";
import "./CallCard.css";

/**
 * How much of the window the card is taking.
 *
 * One value rather than two flags, which would have a fourth state meaning
 * nothing. `full` is where clicking a shared screen goes.
 */
type Size = "card" | "expanded" | "full";

/**
 * How many faces peek out from behind a shared screen before the rest become a
 * count. What the card is wide enough for at each size it floats at.
 */
const PEEKING: Record<"card" | "expanded", number> = { card: 4, expanded: 7 };

/**
 * How many pixels to ask a remote picture for, by where it is being drawn.
 *
 * Twice the width the box is drawn at while the card floats, so a display at
 * two device pixels per CSS pixel gets a true pixel for each one, and the
 * ceiling once it fills the window, where twice would be past it. The squares
 * are all one size and the stage is three, which is why the stage's are named
 * after the card's.
 *
 * The table and the ceiling above it:
 * `docs/adr/0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md`. Issue
 * #167 replaces this with a measured box and a cap somebody chose.
 */
const BOUND: Record<Size | "tile", number> = {
  tile: 320,
  card: 480,
  expanded: 960,
  full: 1920,
};

/**
 * Arrows into the corners, or back out of them.
 *
 * One glyph that turns around rather than two controls, because growing and
 * shrinking are one state with three values and a pair of buttons would leave
 * whichever one is not available to press looking broken.
 */
function ExpandIcon({ expanded }: { expanded: boolean }) {
  return (
    <svg
      className="call-card__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {expanded ? (
        <>
          <path d="M4 14h6v6" />
          <path d="M20 10h-6V4" />
          <path d="m14 10 7-7" />
          <path d="m10 14-7 7" />
        </>
      ) : (
        <>
          <path d="M15 3h6v6" />
          <path d="M9 21H3v-6" />
          <path d="m21 3-7 7" />
          <path d="m3 21 7-7" />
        </>
      )}
    </svg>
  );
}

interface Props {
  /**
   * What this session's call is doing.
   *
   * Passed whole rather than reduced to a roster, because whether there is a
   * card at all is the same question as whether there is a call, and the two
   * answers drifting apart is how a card outlives the thing it describes.
   */
  call: Call;
  /** What the channel is called, or null while the room list has not said. */
  channelName: string | null;
  /**
   * Who is talking, by Matrix user ID.
   *
   * Handed down from the one subscription the screen has. This changes several
   * times a second, so a listener of its own here would be that many renders
   * of everything else on the card.
   */
  speaking?: ReadonlySet<string>;
  /** Whoever is signed in, so a person's card can tell when it is about them. */
  selfId: string;
  /**
   * Whether to draw it at all.
   *
   * Owned by the shell rather than held here, because the control that brings
   * it back is the call panel's: a card that has drawn nothing has no button
   * left to press, so the state has to outlive the markup.
   */
  shown: boolean;
  /** Put it away. The shell decides what that means and when it ends. */
  onHide: () => void;
  /** Show a room, by ID. Passed to a person's card for its Message button. */
  onOpenRoom: (roomId: string) => void;
  /**
   * Whether this session's camera is reaching the call.
   *
   * Taken from the self-video channel rather than asked for here, so the picture
   * and the camera button cannot disagree about whether a camera is on.
   */
  cameraOn?: boolean;
  /**
   * What this session is putting on the call from its screen, or null.
   *
   * The name rather than a flag, because it is what the tile is captioned
   * with, and from the screen channel for the reason `cameraOn` comes from the
   * video one: a share also stops for reasons nobody clicked.
   */
  sharing?: string | null;
}

/**
 * The call you are in, floating over whatever you are reading.
 *
 * A div in the one window rather than a window of its own. The capability set
 * grants `core:default` for `main` alone, so a second window is a permission
 * this frontend does not have and a second React root to keep in step, and the
 * thing being asked for is an overlay beside a sidebar that is still visible.
 *
 * It says what the call is carrying and nothing else: a square per person, a
 * square per screen somebody is sharing. Everything that acts on the call, the
 * microphone, the headphones, and hanging up, stays in `CallPanel`, which is
 * what makes closing this safe: there is no way to lose the call by shutting
 * the card, and no control with two homes to keep in agreement.
 */
export function CallCard({
  call,
  channelName,
  speaking = NOBODY,
  selfId,
  shown,
  onHide,
  onOpenRoom,
  cameraOn = false,
  sharing = null,
}: Props) {
  const drag = useDraggable();
  const [size, setSize] = useState<Size>("card");
  // Which face was clicked, and where to draw the card about them. One at a
  // time, for the reason the channel list keeps one.
  const [opened, setOpened] = useState<{
    person: Participant;
    at: { x: number; y: number };
  } | null>(null);

  // The same condition the call panel draws itself on. A second rule for when
  // there is a call is a second thing to keep in step with the first.
  const gone = call.state === "disconnected" || call.state === "failed";
  const drawn = shown && !gone;
  const full = size === "full";

  const where = channelName ?? "Voice channel";
  // A join in flight has no roster yet. The card is drawn anyway, because the
  // thing it is answering is "am I in this channel", and that is true from the
  // moment the join goes out.
  const people = call.state === "connected" ? call.participants : [];

  /*
    Every screen the call is carrying, this session's own first.

    Ours comes from the screen channel rather than from the roster: the channel
    says so the moment the capture starts and names what is going out, where
    the roster only agrees once the SFU has reflected it back. Which is why we
    are filtered out of the rest, or one share would be two squares.
  */
  const screens = [
    ...(sharing === null
      ? []
      : [
          {
            key: selfId,
            label: sharing,
            picture: () => <SelfPicture of="screen" />,
          },
        ]),
    ...people
      .filter((person) => person.id !== selfId && person.screen === true)
      .map((person) => ({
        key: person.id,
        label: `${person.name}'s screen`,
        /*
          A function of the size, because the same share is drawn at two of
          them: across the stage and as a square in the strip. Ours takes no
          size, being one local capture bounded where it is sampled.
        */
        picture: (bound: number) => (
          <TheirPicture
            userId={person.id}
            name={person.name}
            of="screen"
            bound={bound}
          />
        ),
      })),
  ];

  /*
    Behind the screen rather than beside it: a card 240px across has room for
    one of the two, and the screen is what the call is about while one is going
    out. Nothing to tuck behind, or nobody to tuck, means neither happens.
  */
  const peeking = !full && screens.length > 0 && people.length > 0;
  const peekers = people.slice(
    0,
    size === "expanded" ? PEEKING.expanded : PEEKING.card,
  );
  const leftOut = people.length - peekers.length;

  /*
    Which screen the stage is showing, and which are left waiting under it.

    Ours by default, which ADR-0009 chose because it was the only share that
    could draw a picture. That reason has expired with #70 and the rule has not
    changed: the first screen listed still wins, and picking the newest instead
    needs the arrival order the roster does not carry. A tile that was clicked
    wins until that share stops, and `find` is what hands the stage back when
    it does.
  */
  const [picked, setPicked] = useState<string | null>(null);
  const staged = screens.find((one) => one.key === picked) ?? screens[0] ?? null;
  const waiting = screens.filter((one) => one !== staged);

  /*
    Expanding makes it wider, and a card parked against the right edge grows
    straight off it. The hook follows a window that shrank; this is the other
    half of the same problem and nothing else would ever bring it back.

    Before paint, so it is never drawn in the place it cannot stay.
  */
  useLayoutEffect(() => {
    // Nothing floats while it fills the window, and measuring it then would
    // clamp a card that had been dragged into the corner for its way back.
    if (full) return;
    drag.keepInView();
    // The function and not the hook's whole answer, which is a fresh object
    // every render and would make this run after every one of them.
    //
    // How many screens there are is in here because a row of them changes the
    // card's height, and a card against the bottom edge grows straight off it.
  }, [size, full, shown, screens.length, drag.keepInView]);

  /*
    Filling the window is about one call, the way putting the card away is.
    The size somebody chose for the card itself outlives one, and did before
    any of this, so only the window-filling view is given back.
  */
  useEffect(() => {
    if (gone) setSize((current) => (current === "full" ? "card" : current));
  }, [gone]);

  /*
    Escape, out of the full-screen view and nowhere else. A floating card has
    nothing Escape should take away, and one that closed on it would vanish
    every time somebody dismissed something else.
  */
  useEffect(() => {
    if (!drawn || !full) return;

    function leave(event: KeyboardEvent) {
      if (event.key === "Escape") setSize("card");
    }

    window.addEventListener("keydown", leave);
    return () => window.removeEventListener("keydown", leave);
  }, [drawn, full]);

  if (!drawn) return null;

  // Taken off the narrowed `call` here because `face` below is a closure, and
  // the narrowing the guard above does cannot reach inside one.
  const { roomId } = call;

  /**
   * One person, drawn the way the card is drawing people at the moment.
   *
   * One call site for both arrangements, because a square and a face peeking
   * out from behind a screen differ in nothing but the word.
   */
  function face(participant: Participant, layout: "tile" | "peek") {
    return (
      <CallFace
        key={participant.id}
        person={participant}
        roomId={roomId}
        speaking={speaking.has(participant.id)}
        /*
          Always, unlike the sidebar. This card is only ever about the call this
          session is in, so its roster is the live one and a camera that is off
          is a reading rather than a silence.
        */
        live
        layout={layout}
        /*
          The camera in the square the avatar is in. Passed as a node rather
          than a URL so that a frame arriving redraws it alone, which is the
          trap #142 found by holding the picture here.
        */
        picture={camera(participant)}
        onOpen={(at) => setOpened({ person: participant, at })}
      />
    );
  }

  /**
   * Whichever camera belongs in this person's square, if any.
   *
   * Ours comes from the local capture and everybody else's from the call,
   * which is the whole of #69 from out here. Theirs is drawn on the roster's
   * word: `camera` is only ever true of somebody publishing one unmuted, and
   * the picture answers nothing until its first frame either way.
   */
  function camera(participant: Participant): ReactNode {
    if (participant.id === selfId) {
      return cameraOn ? <SelfPicture of="camera" /> : undefined;
    }

    return participant.camera === true ? (
      <TheirPicture
        userId={participant.id}
        name={participant.name}
        of="camera"
        bound={BOUND.tile}
      />
    ) : undefined;
  }

  /** Back to the floating card, or out to the size beyond it. */
  function resize() {
    setSize((current) => (current === "card" ? "expanded" : "card"));
  }

  /**
   * Bigger, or back to the size it was.
   *
   * A double-click on a control is that control's business: a face opens the
   * card about a person, and resizing the thing that was just clicked out from
   * under the pointer is not what anybody asked for.
   *
   * A drag is not a click either, even one that ended where it started. The
   * hand that moved the card and put it back has dragged it.
   */
  function toggle(event: ReactMouseEvent<HTMLElement>) {
    if (
      event.target instanceof Element &&
      event.target.closest("button") !== null
    ) {
      return;
    }
    if (drag.dragged()) return;
    resize();
  }

  return (
    <section
      className="call-card"
      ref={drag.ref}
      data-state={call.state}
      data-size={size}
      data-peeking={peeking}
      aria-label={`Call in ${where}`}
      onDoubleClick={toggle}
      style={
        drag.at === null || full
          ? undefined
          : { left: drag.at.left, top: drag.at.top, right: "auto" }
      }
    >
      <header className="call-card__bar">
        {/*
          A button, so focus, the role and the announcement all come free. What
          it does is the arrow keys rather than Enter: there is nothing for a
          press on its own to mean, and a handle only a mouse can reach is a
          handle some people do not have.
        */}
        <button
          type="button"
          className="call-card__grip"
          /*
            The channel's name is in the label as well as under it, because a
            name that is only drawn is a name speech input cannot be told to
            press. The arrow keys are in it because they are the half of this
            control nothing else announces.
          */
          aria-label={`Move the ${where} call card with the arrow keys`}
          /*
            Nothing to move while it fills the window, and a drag there would
            leave the card clamped to the corner for when it comes back.
          */
          disabled={full}
          {...drag.handle}
        >
          <span className="call-card__where">{where}</span>
        </button>
        {/*
          The same thing a double-click on the card does, as something that can
          be pressed. A size that can only be changed by a gesture is a size
          some people cannot change, and the gesture is not discoverable
          either: nothing on a card announces that it can be double-clicked.

          One press from the full-screen view goes all the way back, because
          "back to the floating card" is what was asked for.

          `aria-pressed` rather than a label that changes, the way the call
          panel's buttons do it: a button whose name changes under the cursor
          is announced as a new button.
        */}
        <button
          type="button"
          className="call-card__control"
          aria-pressed={size !== "card"}
          aria-label="Expand the call card"
          title={size === "card" ? "Expand" : "Collapse"}
          onClick={resize}
        >
          <ExpandIcon expanded={size !== "card"} />
        </button>
        {/*
          Hiding, not leaving. A card that covers the conversation and cannot
          be put away is worse than no card, and the sidebar still says you are
          in a call either way.

          Which is also where it comes back from: the call panel's state line
          is the other end of this, so putting the card away is a thing with
          two directions rather than a door that only shuts.
        */}
        <button
          type="button"
          className="call-card__control"
          aria-label="Hide the call card"
          title="Hide"
          onClick={onHide}
        >
          &times;
        </button>
      </header>

      {/*
        Before the screens in the markup as well as above them on the card,
        because that is what lets a screen paint over the faces behind it. The
        same list under the same name as the squares, not a second roster.
      */}
      {peeking && (
        <ul
          className="call-card__peek"
          aria-label={
            leftOut > 0
              ? `People in ${where}, and ${leftOut} more`
              : `People in ${where}`
          }
        >
          {peekers.map((participant) => face(participant, "peek"))}
          {/*
            Hidden from a screen reader because the list's own name already
            carries the number, the way the read receipts under a message do
            it. The sidebar's roster is the one that leaves nobody out.
          */}
          {leftOut > 0 && (
            <li className="call-card__more" aria-hidden="true">
              +{leftOut}
            </li>
          )}
        </ul>
      )}

      {/*
        Above the faces, because a screen somebody is presenting is what the
        call is about and the people in it are who is present.
      */}
      {staged !== null && (
        <ScreenStage
          label={staged.label}
          picture={staged.picture(BOUND[size])}
          full={full}
          onToggle={() =>
            setSize((current) => (current === "full" ? "card" : "full"))
          }
        />
      )}

      {waiting.length > 0 && (
        <ul
          className="call-card__screens"
          aria-label={`Other screens shared in ${where}`}
        >
          {waiting.map((shared) => (
            <ScreenTile
              key={shared.key}
              label={shared.label}
              picture={shared.picture(BOUND.tile)}
              onPick={() => setPicked(shared.key)}
            />
          ))}
        </ul>
      )}

      {call.state === "connecting" ? (
        <p className="call-card__waiting">{callLabel(call)}</p>
      ) : (
        !peeking && (
          <ul className="call-card__people" aria-label={`People in ${where}`}>
            {people.map((participant) => face(participant, "tile"))}
          </ul>
        )
      )}

      {/*
        Inside the card rather than beside it. The channel list moves its own
        copy out of the column because WebKitGTK draws a scroll container's bar
        over everything in it; nothing here scrolls and nothing here clips, so
        a menu nested in the card paints over the card and can be taller than
        it.
      */}
      {opened !== null && (
        <PersonMenu
          key={opened.person.id}
          person={opened.person}
          roomId={call.roomId}
          selfId={selfId}
          at={opened.at}
          onClose={() => setOpened(null)}
          onOpenRoom={onOpenRoom}
        />
      )}
    </section>
  );
}
