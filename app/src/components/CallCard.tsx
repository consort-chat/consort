import {
  useEffect,
  useLayoutEffect,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";

import { NOBODY, type Call, type Participant } from "../lib/api";
import { callLabel } from "../lib/labels";
import { useDraggable } from "../lib/useDraggable";
import { CallFace } from "./CallFace";
import { PersonMenu } from "./PersonMenu";
import "./CallCard.css";

/**
 * Arrows into the corners, or back out of them.
 *
 * One glyph that turns around rather than two controls, because expanding and
 * collapsing are one state with two values and a pair of buttons would leave
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
  /** Show a room, by ID. Passed to a person's card for its Message button. */
  onOpenRoom: (roomId: string) => void;
}

/**
 * The call you are in, floating over whatever you are reading.
 *
 * A div in the one window rather than a window of its own. The capability set
 * grants `core:default` for `main` alone, so a second window is a permission
 * this frontend does not have and a second React root to keep in step, and the
 * thing being asked for is an overlay beside a sidebar that is still visible.
 *
 * It says who is here and nothing else. Everything that acts on the call, the
 * microphone, the headphones, and hanging up, stays in `CallPanel`, which is
 * what makes closing this safe: there is no way to lose the call by shutting
 * the card, and no control with two homes to keep in agreement.
 */
export function CallCard({
  call,
  channelName,
  speaking = NOBODY,
  selfId,
  onOpenRoom,
}: Props) {
  const drag = useDraggable();
  const [expanded, setExpanded] = useState(false);
  const [hidden, setHidden] = useState(false);
  // Which face was clicked, and where to draw the card about them. One at a
  // time, for the reason the channel list keeps one.
  const [opened, setOpened] = useState<{
    person: Participant;
    at: { x: number; y: number };
  } | null>(null);

  /*
    A card that was closed is closed for this call and not for good. Reset as
    the next join goes out rather than when it lands, because that is the
    moment somebody asked for a call and so the moment they would expect the
    card back.
  */
  useEffect(() => {
    if (call.state === "connecting") setHidden(false);
  }, [call.state]);

  /*
    Expanding makes it wider, and a card parked against the right edge grows
    straight off it. The hook follows a window that shrank; this is the other
    half of the same problem and nothing else would ever bring it back.

    Before paint, so it is never drawn in the place it cannot stay.
  */
  useLayoutEffect(() => {
    drag.keepInView();
    // The function and not the hook's whole answer, which is a fresh object
    // every render and would make this run after every one of them.
  }, [expanded, drag.keepInView]);

  // The same condition the call panel draws itself on. A second rule for when
  // there is a call is a second thing to keep in step with the first.
  if (call.state === "disconnected" || call.state === "failed") return null;
  if (hidden) return null;

  const where = channelName ?? "Voice channel";
  // A join in flight has no roster yet. The card is drawn anyway, because the
  // thing it is answering is "am I in this channel", and that is true from the
  // moment the join goes out.
  const people = call.state === "connected" ? call.participants : [];

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
    setExpanded((current) => !current);
  }

  return (
    <section
      className="call-card"
      ref={drag.ref}
      data-state={call.state}
      data-expanded={expanded}
      aria-label={`Call in ${where}`}
      onDoubleClick={toggle}
      style={
        drag.at === null
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
          {...drag.handle}
        >
          <span className="call-card__where">{where}</span>
        </button>
        {/*
          The same thing a double-click on the card does, as something that can
          be pressed. A size that can only be changed by a gesture is a size
          some people cannot change, and the gesture is not discoverable
          either: nothing on a card announces that it can be double-clicked.

          `aria-pressed` rather than a label that changes, the way the call
          panel's buttons do it: a button whose name changes under the cursor
          is announced as a new button.
        */}
        <button
          type="button"
          className="call-card__control"
          aria-pressed={expanded}
          aria-label="Expand the call card"
          title={expanded ? "Collapse" : "Expand"}
          onClick={() => setExpanded((current) => !current)}
        >
          <ExpandIcon expanded={expanded} />
        </button>
        {/*
          Hiding, not leaving. A card that covers the conversation and cannot
          be put away is worse than no card, and the sidebar still says you are
          in a call either way.
        */}
        <button
          type="button"
          className="call-card__control"
          aria-label="Hide the call card"
          title="Hide"
          onClick={() => setHidden(true)}
        >
          &times;
        </button>
      </header>

      {call.state === "connecting" ? (
        <p className="call-card__waiting">{callLabel(call)}</p>
      ) : (
        <ul className="call-card__people" aria-label={`People in ${where}`}>
          {people.map((participant) => (
            <CallFace
              key={participant.id}
              person={participant}
              roomId={call.roomId}
              speaking={speaking.has(participant.id)}
              /*
                Always, unlike the sidebar. This card is only ever about the
                call this session is in, so its roster is the live one and a
                camera that is off is a reading rather than a silence.
              */
              live
              layout="tile"
              onOpen={(at) => setOpened({ person: participant, at })}
            />
          ))}
        </ul>
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
