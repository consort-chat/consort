import { useEffect, useState, type FormEvent } from "react";

import {
  asCommandError,
  roomCanInvite,
  roomCopyLink,
  roomInvite,
  roomLeave,
  roomMembers,
  type Channel,
  type Members,
  type Participant,
  type Roster,
} from "../lib/api";
import { channelHeading } from "../lib/labels";
import { Confirm } from "./Confirm";
import { PersonMenu } from "./PersonMenu";
import { RoomAvatar } from "./RoomAvatar";
import { COPIED_FOR } from "./RoomTimeline";
import "./RoomInfoPanel.css";

/**
 * Whether this room will take an invitation from this account, and why not.
 *
 * Three states rather than a boolean, because "not yet known" is a real one:
 * the answer costs a command, and a control drawn as ready before it lands
 * would be answering a question that has not come back. Written this way so
 * that "not allowed" cannot exist without a sentence saying why, which is the
 * whole difference between this and simply leaving the control out.
 */
type Invites = { allowed: true } | { allowed: false; why: string };

/** Why a room refuses an invitation from somebody with too little power. */
const NO_PERMISSION = "You do not have permission to invite people here.";

/**
 * What a room is, beside the room.
 *
 * Opened from the heading, which until #85 was a line of text that did
 * nothing. Almost everything here is already in the snapshot the shell holds,
 * so nothing is fetched and nothing can fail to arrive: the panel is the same
 * facts the header has room for one line of, drawn at a size that can hold all
 * of them.
 *
 * A column beside the room rather than a dialog over it, for the reason the
 * thread panel is one: what somebody is reading when they ask what a room is
 * about is the room, and a panel that covered it would hide the answer to the
 * question behind it.
 *
 * ## The one thing it does ask for
 *
 * Who is in the room. It is a command rather than a field on the room list,
 * because the list is re-sent in full whenever anything in it changes and a
 * member list per room would multiply a payload that is a few kilobytes today
 * by every room on the account.
 *
 * What comes back is a snapshot of the moment the panel was pointed at this
 * room, and it stays that way until it is pointed at another one. Somebody
 * joining while it is open does not appear, which is the price of the
 * alternative being worse: a list that changed under the pointer would move
 * the row somebody was about to press, and the machinery to do it at all
 * would be a subscription per room feeding exactly the payload this command
 * exists to avoid.
 */
export function RoomInfoPanel({
  channel,
  selfId,
  onClose,
  onOpenRoom,
}: {
  /** The room this is about, straight off the snapshot the shell holds. */
  channel: Channel;
  /**
   * Whoever is signed in, so a person's card can tell when it is about them.
   *
   * Your own name is one of the names in a member list, and without this the
   * card's Message button would offer to make a note-to-self room by accident.
   */
  selfId: string;
  onClose: () => void;
  /** Show a room, by ID. Handed to a person's card for its Message button. */
  onOpenRoom: (roomId: string) => void;
}) {
  const [copied, setCopied] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [members, setMembers] = useState<Members | null>(null);
  const [membersProblem, setMembersProblem] = useState<string | null>(null);
  // Which name was pressed, and where to draw the card about them. One at a
  // time, as in the channel list: two cards about two people would be two
  // sliders to tell apart by the heading.
  const [opened, setOpened] = useState<{
    person: Participant;
    at: { x: number; y: number };
  } | null>(null);
  /* Null while the answer is still coming. See [`Invites`]. */
  const [invites, setInvites] = useState<Invites | null>(null);
  const [invitee, setInvitee] = useState("");
  /* Who the last invitation went to, so the control can say it did. */
  const [invited, setInvited] = useState<string | null>(null);
  const [inviteProblem, setInviteProblem] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [leaveProblem, setLeaveProblem] = useState<string | null>(null);
  const roomId = channel.id;

  /*
    Escape shuts the panel, which is what it does to everything else here that
    can be dismissed.

    On `window` for the reason the thread panel's is: the picture viewer, the
    settings dialog, the reaction picker and a person's card all catch the key
    at the document and stop it there, one step nearer the press. Listening
    further out is what leaves them the press while any of them is open over
    this, and what leaves the composer the press while it has a reply or an
    attachment to clear. One press, one thing.

    Nothing guards on the panel being open because the shell only mounts this
    while it is, so the listener goes with it.
  */
  useEffect(() => {
    function shut(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      onClose();
    }

    window.addEventListener("keydown", shut);
    return () => {
      window.removeEventListener("keydown", shut);
    };
  }, [onClose]);

  // The tick on the copy control, put back after a moment. The same length the
  // one on a message holds for, because two ticks on one screen that fade at
  // different speeds is a difference nobody could see the reason for.
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), COPIED_FOR);
    return () => window.clearTimeout(timer);
  }, [copied]);

  /*
    Who is here, asked for once per room the panel is pointed at.

    `moved` is not an unmount guard. The panel follows the selected room, so a
    slow homeserver can answer about the room before this one, and drawing
    those people under this room's name would be a list that is wrong in the
    one way nobody would think to check.
  */
  useEffect(() => {
    let moved = false;
    setMembers(null);
    setMembersProblem(null);
    setOpened(null);

    void roomMembers(roomId)
      .then((who) => {
        if (moved) return;
        setMembers(who);
      })
      .catch((raw: unknown) => {
        if (moved) return;
        setMembersProblem(asCommandError(raw).message);
      });

    return () => {
      moved = true;
    };
  }, [roomId]);

  /*
    Whether this room will take an invitation, asked once per room.

    `moved` is not an unmount guard. The panel follows the selected room, so a
    slow homeserver can answer about the room before this one, and drawing that
    answer under this room's name would enable a control the room will refuse.

    Everything else about the invitation goes with it: what was typed, who the
    last one went to, and why the last one failed are all about the room that
    was on screen when they happened.
  */
  useEffect(() => {
    let moved = false;
    setInvites(null);
    setInvitee("");
    setInvited(null);
    setInviteProblem(null);

    void roomCanInvite(roomId)
      .then((allowed) => {
        if (moved) return;
        setInvites(allowed ? { allowed } : { allowed, why: NO_PERMISSION });
      })
      .catch((raw: unknown) => {
        // Not the permission sentence. Reaching this means the room is not one
        // this account is in, which is a different answer and deserves the one
        // Rust wrote for it.
        if (moved) return;
        setInvites({ allowed: false, why: asCommandError(raw).message });
      });

    return () => {
      moved = true;
    };
  }, [roomId]);

  // The question is about this room, so the answer to it goes when the room
  // does. Without this, pointing the panel at another room leaves a
  // confirmation open asking about the one before it.
  useEffect(() => {
    setConfirming(false);
    setLeaveProblem(null);
  }, [roomId]);

  const name = channelHeading(channel);

  function copyLink() {
    setProblem(null);
    void roomCopyLink(channel.id)
      .then(() => setCopied(true))
      .catch((raw: unknown) => {
        setProblem(asCommandError(raw).message);
      });
  }

  /*
    Nothing is done here about what to select next. The shell derives its
    selection from the room list every render, so the room going out of that
    list is what deselects it and falls the pane back to the empty state. A
    panel that also asked for a selection would be a second opinion about a
    question already answered, and the two would disagree the moment either
    changed.
  */
  function leaveRoom() {
    setLeaveProblem(null);
    void roomLeave(roomId).catch((raw: unknown) => {
      setLeaveProblem(asCommandError(raw).message);
    });
  }

  function sendInvite(event: FormEvent) {
    event.preventDefault();
    const who = invitee.trim();
    // Nothing typed is not a request. The homeserver would refuse it and the
    // sentence it came back with would be about a user ID nobody entered.
    if (who === "") return;

    setInviteProblem(null);
    setInvited(null);
    void roomInvite(roomId, who)
      .then(() => {
        setInvited(who);
        // Emptied only on the way through. A refused invitation keeps what was
        // typed, because a user ID is a long thing to type twice and the
        // likeliest fix is one character of it.
        setInvitee("");
      })
      .catch((raw: unknown) => {
        setInviteProblem(asCommandError(raw).message);
      });
  }

  return (
    <aside className="info" aria-label={`About ${name}`}>
      <div className="info__head">
        <h2 className="info__title">Room info</h2>
        <button
          type="button"
          className="info__close"
          aria-label="Close room info"
          onClick={onClose}
        >
          &times;
        </button>
      </div>

      <div className="info__scroll">
        <div className="info__identity">
          <RoomAvatar
            roomId={channel.id}
            name={name}
            avatar={channel.avatar}
            className="info__avatar"
          />
          <p className="info__name">{name}</p>
          {/*
            The address the room published, under the name it goes by. Absent
            for most private rooms, and absent is drawn as nothing: a room with
            no published address has not failed to have one.
          */}
          {channel.alias !== undefined && (
            <p className="info__alias">{channel.alias}</p>
          )}
        </div>

        {/*
          The whole topic, wrapped, which is the thing the header cannot do.
          `pre-wrap` because a topic is free text and the line breaks somebody
          put in one are part of what they wrote.
        */}
        <section className="info__section" aria-labelledby="info-topic">
          <h3 className="info__heading" id="info-topic">
            Topic
          </h3>
          {channel.topic === undefined ? (
            <p className="info__none">This room has not set a topic.</p>
          ) : (
            <p className="info__topic">{channel.topic}</p>
          )}
        </section>

        {/*
          Above the people rather than under them, though the people are the
          longer read. A hundred rows between somebody and the one control on
          this panel would make copying a link a scroll.
        */}
        <button type="button" className="info__copy" onClick={copyLink}>
          {copied ? "Link copied" : "Copy room link"}
        </button>

        {problem !== null && (
          <p className="info__problem" role="alert">
            {problem}
          </p>
        )}

        <section className="info__section" aria-labelledby="info-people">
          <h3 className="info__heading" id="info-people">
            People{" "}
            {members !== null && (
              <span className="info__count">{members.joined.count}</span>
            )}
          </h3>

          {membersProblem !== null && (
            <p className="info__problem" role="alert">
              {membersProblem}
            </p>
          )}
          {members === null && membersProblem === null && (
            <p className="info__none">Reading who is here…</p>
          )}
          {members !== null && (
            <>
              <Rows
                roomId={roomId}
                roster={members.joined}
                onOpen={(person, at) => setOpened({ person, at })}
              />
              {/*
                Their own group rather than a mark on a row, because the
                difference is not decoration: somebody invited cannot read
                what is being said here yet, and that is the fact that matters
                when the next thing anybody does is paste something into the
                room.
              */}
              {members.invited.count > 0 && (
                <div className="info__invited">
                  <h4 className="info__subheading">
                    Invited{" "}
                    <span className="info__count">{members.invited.count}</span>
                  </h4>
                  <p className="info__note">
                    They have not joined, so they cannot read what is said here
                    yet.
                  </p>
                  <Rows
                    roomId={roomId}
                    roster={members.invited}
                    onOpen={(person, at) => setOpened({ person, at })}
                  />
                </div>
              )}
            </>
          )}
        </section>
      </div>

      {/*
        The two actions, outside the scroll rather than at the bottom of it.

        Pinned because of what goes above them: the panel's scrolling half is
        the room's own facts and grows with the room, and a destructive control
        underneath a hundred names would be a control people scroll past to
        reach. Pinned also gives the confirmation somewhere to open. A panel
        hanging off a button inside an `overflow-y: auto` column is a panel
        that column clips.
      */}
      <div className="info__actions">
        <section className="info__section" aria-labelledby="info-invite">
          <h3 className="info__heading" id="info-invite">
            Invite
          </h3>
          {/*
            A form rather than a box and a button that happen to sit together,
            so Enter sends. A user ID is typed rather than picked, and the
            hand that typed one is already at the keyboard.
          */}
          <form className="info__invite" onSubmit={sendInvite}>
            <input
              type="text"
              className="info__who-box"
              aria-label="Matrix user ID"
              placeholder="@name:example.org"
              value={invitee}
              spellCheck={false}
              autoComplete="off"
              disabled={invites?.allowed !== true}
              onChange={(event) => {
                setInvitee(event.target.value);
                // Both answers are about what was in the box a moment ago, so
                // both go as soon as it changes.
                setInviteProblem(null);
                setInvited(null);
              }}
            />
            <button
              type="submit"
              className="info__invite-go"
              disabled={invites?.allowed !== true}
            >
              Invite
            </button>
          </form>

          {/*
            Why the control is off, when it is. Drawn rather than left out for
            the reason the control itself is: a missing control reads as
            something Consort cannot do, and this is something this room will
            not let this account do.
          */}
          {invites?.allowed === false && (
            <p className="info__note">{invites.why}</p>
          )}
          {invited !== null && (
            <p className="info__sent" role="status">
              Invited {invited}. They have to accept before they can read
              anything here.
            </p>
          )}
          {inviteProblem !== null && (
            <p className="info__problem" role="alert">
              {inviteProblem}
            </p>
          )}
        </section>

        {/*
          Last, alone, and in the one colour nothing else in this panel uses.
          It is the only control here that cannot be undone by pressing it
          again.
        */}
        <div className="info__anchor">
          <button
            type="button"
            className="info__leave"
            aria-haspopup="dialog"
            aria-expanded={confirming}
            onClick={() => {
              setLeaveProblem(null);
              setConfirming((open) => !open);
            }}
          >
            Leave room
          </button>
          {confirming && (
            <Confirm
              question={`Leave ${name}?`}
              detail="You stop receiving messages from this room, and any call you are in here ends. If the room is invite only you need an invitation to come back."
              go="Leave"
              onConfirm={() => {
                setConfirming(false);
                leaveRoom();
              }}
              onCancel={() => setConfirming(false)}
            />
          )}
        </div>

        {leaveProblem !== null && (
          <p className="info__problem" role="alert">
            {leaveProblem}
          </p>
        )}
      </div>

      {/*
        At the root of the panel rather than inside the row that opened it, so
        one card is open at a time and it survives the row scrolling out from
        under it.
      */}
      {opened !== null && (
        <PersonMenu
          key={opened.person.id}
          person={opened.person}
          roomId={roomId}
          selfId={selfId}
          at={opened.at}
          onClose={() => setOpened(null)}
          onOpenRoom={onOpenRoom}
        />
      )}
    </aside>
  );
}

/**
 * One group of people, and what it could not fit.
 *
 * The order is the order it arrived in. It was decided once, in Rust, and it
 * is total; sorting again here would be a second opinion free to disagree with
 * the first, which is exactly the thing a stable order is for.
 */
function Rows({
  roomId,
  roster,
  onOpen,
}: {
  roomId: string;
  roster: Roster;
  onOpen: (person: Participant, at: { x: number; y: number }) => void;
}) {
  const hidden = roster.count - roster.shown.length;

  return (
    <>
      <ul className="info__people">
        {roster.shown.map((member) => (
          <li key={member.person.id}>
            <button
              type="button"
              className="info__person"
              /*
                Set only when the name on the row is a user ID rather than a
                name somebody chose, which the stylesheet draws as the
                identifier it is instead of as a name nobody picked well.
              */
              data-naming={member.naming}
              onClick={(event) =>
                onOpen(member.person, { x: event.clientX, y: event.clientY })
              }
            >
              <RoomAvatar
                roomId={roomId}
                userId={member.person.id}
                name={member.person.name}
                className="info__face"
              />
              <span className="info__names">
                <span className="info__who">{member.person.name}</span>
                {/*
                  Said in words rather than marked with a glyph. The user ID is
                  already inside the name by the time it reaches here, and a
                  reader who does not know why it is there has no reason to
                  look at it, which is the whole of how somebody gets taken in
                  by a copied display name.
                */}
                {member.naming === "shared" && (
                  <span className="info__shared">
                    Someone else here uses this name
                  </span>
                )}
              </span>
            </button>
          </li>
        ))}
      </ul>
      {/*
        What the cap left out. The count above is of the room and this is of
        the list, and a list that quietly stopped at a hundred would make a
        room of two thousand look like a room of a hundred.
      */}
      {hidden > 0 && <p className="info__more">and {hidden} more</p>}
    </>
  );
}
