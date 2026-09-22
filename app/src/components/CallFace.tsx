import type { Participant } from "../lib/api";
import { RoomAvatar } from "./RoomAvatar";
import "./CallFace.css";

/**
 * A struck-through microphone, next to somebody who has muted themselves.
 *
 * Smaller and thinner than the one in the call panel. That one is a control
 * somebody presses; this is a fact about a name in a list, and drawing them at
 * the same weight would make the list look like a row of buttons.
 */
function MutedIcon({ "aria-label": label }: { "aria-label": string }) {
  return (
    <svg
      className="call-face__flag"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      role="img"
      aria-label={label}
    >
      <rect x="9" y="2" width="6" height="11" rx="3" />
      <path d="M5 10a7 7 0 0 0 14 0" />
      <path d="M12 17v4" />
      <path d="M3 3l18 18" />
    </svg>
  );
}

/**
 * Struck-through headphones, next to somebody who has stopped listening.
 *
 * Drawn instead of the microphone rather than beside it. Deafening mutes, so
 * both are true of the same person, and showing two icons would spend twice
 * the width saying one thing. The headphones are the stronger claim: somebody
 * muted might still be listening, somebody deafened is not.
 */
function DeafenedIcon({ "aria-label": label }: { "aria-label": string }) {
  return (
    <svg
      className="call-face__flag"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      role="img"
      aria-label={label}
    >
      <path d="M4 14v-2a8 8 0 0 1 16 0v2" />
      <path d="M4 14h3v6H5.5A1.5 1.5 0 0 1 4 18.5z" />
      <path d="M20 14h-3v6h1.5a1.5 1.5 0 0 0 1.5-1.5z" />
      <path d="M3 3l18 18" />
    </svg>
  );
}

/**
 * A clock, next to somebody who is not at their computer.
 *
 * The one icon here that is not a struck-through anything, deliberately. The
 * other two say what somebody switched off; this one says they are not there,
 * which is a different kind of fact and should not look like a fault.
 */
function AwayIcon({ "aria-label": label }: { "aria-label": string }) {
  return (
    <svg
      className="call-face__flag"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      role="img"
      aria-label={label}
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </svg>
  );
}

/**
 * A camera, struck through when nobody can see them.
 *
 * Always drawn for the call this session is in, which is the difference
 * between this and the three glyphs above. Those say somebody chose something,
 * so their absence means "nothing to report"; this one answers a question that
 * always has an answer, and an icon that only appeared when a camera came on
 * would leave "off" and "we have not looked" drawn identically.
 *
 * Which is exactly why it is never drawn for a channel this session is not in.
 * Room state carries nothing about cameras, so a cross there would be an
 * invention rather than a reading.
 */
function CameraIcon({
  on,
  "aria-label": label,
}: {
  on: boolean;
  "aria-label": string;
}) {
  return (
    <svg
      className="call-face__camera"
      data-on={on}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      role="img"
      aria-label={label}
    >
      <path d="M3 7.5h11v9H3z" />
      <path d="m14 12 6-3.5v7z" />
      {!on && <path d="m3.5 3.5 17 17" />}
    </svg>
  );
}

export interface CallFaceProps {
  /**
   * Who this is, straight off the roster.
   *
   * The whole participant rather than a name and a set of booleans, because
   * every field on it is drawn here and taking them apart at the call site
   * would put the same four conditions in two places.
   */
  person: Participant;
  /** The channel they are in, which is half of the key their avatar takes. */
  roomId: string;
  /** Whether they are audible right now. See the ring in the stylesheet. */
  speaking: boolean;
  /**
   * Whether this came from the live call roster rather than from room state.
   *
   * The camera is only drawn when it did. See [`CameraIcon`].
   */
  live: boolean;
  /**
   * A row in a list, or a tile in a grid.
   *
   * Two arrangements of one face rather than two components, because they are
   * the same subject and #69 is about to put video in both. Size is not part
   * of this: that comes from `--avatar-size` at the call site, which is what
   * that variable is for.
   */
  layout?: "row" | "tile";
  /** Open the card about this person, at the pointer. */
  onOpen: (at: { x: number; y: number }) => void;
}

/**
 * One person in a voice call.
 *
 * The sidebar's list and the floating card draw the same thing, so they draw
 * it from here. Two copies would be two definitions of what a person in a call
 * looks like, to be kept in step at exactly the moment video is about to
 * change both.
 */
export function CallFace({
  person,
  roomId,
  speaking,
  live,
  layout = "row",
  onOpen,
}: CallFaceProps) {
  return (
    /*
      `data-speaking` sits on the row rather than on the avatar, because
      `RoomAvatar` takes the props it knows about and drops the rest. The ring
      is drawn on the face from here, which is where every other client puts it
      and where somebody scanning a list of faces is already looking.
    */
    <li
      className="call-face"
      data-layout={layout}
      data-muted={person.muted === true}
      data-speaking={speaking}
    >
      {/*
        A button rather than a row with a click handler on it. What it opens is
        a menu, and a menu that can only be reached with a mouse is a menu half
        the people here cannot reach at all: this way the keyboard gets it for
        free, along with focus, Enter and Space.

        Both buttons open the same thing. Right-click is where anybody looks
        for a card about a person; left-click is what makes the row look like
        the control it now is, and is the only one a touchpad without a second
        button has. Splitting them would mean two panels about one person, and
        a person is one subject.
      */}
      <button
        type="button"
        className="call-face__button"
        aria-haspopup="dialog"
        onClick={(event) => onOpen({ x: event.clientX, y: event.clientY })}
        onContextMenu={(event) => {
          event.preventDefault();
          onOpen({ x: event.clientX, y: event.clientY });
        }}
      >
        <RoomAvatar
          roomId={roomId}
          userId={person.id}
          name={person.name}
          className="call-face__avatar"
        />
        <span className="call-face__who">{person.name}</span>
        {/*
          Drawn rather than only dimmed, and with a name on it. Somebody
          scanning this list for who to talk to is reading names, not noticing
          that one of them is a shade lighter, and a colour with no glyph
          beside it is nothing at all to a screen reader.

          One icon, never two. All three flags can be set on one person at
          once, because each of the stronger ones implies the microphone is
          off, so this is a precedence rather than a set of conditions.

          Deafened first: it is the only one that says talking to them will not
          reach them at all. Then away, which says they are not there to
          answer. Muted last, because it is the weakest claim of the three and
          the only one that leaves somebody listening and present.
        */}
        {person.deafened === true ? (
          <DeafenedIcon aria-label={`${person.name} is deafened`} />
        ) : person.away === true ? (
          <AwayIcon aria-label={`${person.name} is away`} />
        ) : (
          person.muted === true && (
            <MutedIcon aria-label={`${person.name} is muted`} />
          )
        )}
        {/*
          Beside the precedence above rather than inside it, because it is a
          different question. Somebody muted with their camera on is ordinary,
          and so is the reverse, so these are two facts about one person rather
          than two candidates for one slot.
        */}
        {live && (
          <CameraIcon
            on={person.camera === true}
            aria-label={
              person.camera === true
                ? `${person.name} has their camera on`
                : `${person.name} has their camera off`
            }
          />
        )}
      </button>
    </li>
  );
}
