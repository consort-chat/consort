import { useState } from "react";

import {
  NOBODY,
  callRoomId,
  type Call,
  type Channel,
  type Participant,
  type Space,
} from "../lib/api";
import { channelLabel } from "../lib/labels";
import { PersonMenu } from "./PersonMenu";
import { RoomAvatar } from "./RoomAvatar";
import "./ChannelList.css";

/**
 * A speaker, for voice channels.
 *
 * A glyph rather than a letter, because this is the one distinction the next
 * milestone depends on being visible: a voice channel has to be identifiable
 * as one before anybody clicks it.
 */
function VoiceIcon() {
  return (
    <svg
      className="channels__glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M11 5 6.5 9H3v6h3.5L11 19z" />
      <path d="M15.5 8.5a5 5 0 0 1 0 7" />
      <path d="M18.5 5.5a9 9 0 0 1 0 13" />
    </svg>
  );
}

/**
 * Who is in a voice channel, under it.
 *
 * Drawn without anybody clicking the channel and without connecting to
 * anything: Element Call announces a connection by writing room state, so this
 * is a read of something the account already has.
 *
 * Omitted entirely when the channel is empty, the same way a group with no
 * channels is omitted, so a quiet voice channel keeps exactly the shape it had
 * before this existed.
 */
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
      className="channels__muted"
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
      className="channels__muted"
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
      className="channels__muted"
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
      className="channels__camera"
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

function Participants({
  channel,
  people,
  speaking,
  live,
  onOpenPerson,
}: {
  channel: Channel;
  people: Participant[];
  speaking: ReadonlySet<string>;
  /**
   * Whether these came from the live call roster rather than from room state.
   *
   * The camera is only drawn when they did. See [`CameraIcon`].
   */
  live: boolean;
  onOpenPerson: (
    person: Participant,
    roomId: string,
    at: { x: number; y: number },
  ) => void;
}) {
  if (people.length === 0) return null;

  return (
    <ul
      className="channels__people"
      aria-label={`In ${channelLabel(channel)}`}
    >
      {/*
        `data-speaking` sits on the row rather than on the avatar, because
        `RoomAvatar` takes the props it knows about and drops the rest. The
        ring is drawn on the face from here, which is where every other client
        puts it and where somebody scanning a list of faces is already looking.
      */}
      {people.map((participant) => (
        <li
          key={participant.id}
          className="channels__person"
          data-muted={participant.muted === true}
          data-speaking={speaking.has(participant.id)}
        >
          {/*
            A button rather than a row with a click handler on it. What it
            opens is a menu, and a menu that can only be reached with a mouse
            is a menu half the people here cannot reach at all: this way the
            keyboard gets it for free, along with focus, Enter and Space.

            Both buttons open the same thing. Right-click is where anybody
            looks for a card about a person; left-click is what makes the row
            look like the control it now is, and is the only one a touchpad
            without a second button has. Splitting them would mean two panels
            about one person, and a person is one subject.
          */}
          <button
            type="button"
            className="channels__person-button"
            aria-haspopup="dialog"
            onClick={(event) =>
              onOpenPerson(participant, channel.id, {
                x: event.clientX,
                y: event.clientY,
              })
            }
            onContextMenu={(event) => {
              event.preventDefault();
              onOpenPerson(participant, channel.id, {
                x: event.clientX,
                y: event.clientY,
              });
            }}
          >
            <RoomAvatar
              roomId={channel.id}
              userId={participant.id}
              name={participant.name}
              className="channels__face"
            />
            <span className="channels__who">{participant.name}</span>
            {/*
              Drawn rather than only dimmed, and with a name on it. Somebody
              scanning this list for who to talk to is reading names, not
              noticing that one of them is a shade lighter, and a colour with
              no glyph beside it is nothing at all to a screen reader.

              One icon, never two. All three flags can be set on one person at
              once, because each of the stronger ones implies the microphone is
              off, so this is a precedence rather than a set of conditions.

              Deafened first: it is the only one that says talking to them will
              not reach them at all. Then away, which says they are not there
              to answer. Muted last, because it is the weakest claim of the
              three and the only one that leaves somebody listening and
              present.
            */}
            {participant.deafened === true ? (
              <DeafenedIcon aria-label={`${participant.name} is deafened`} />
            ) : participant.away === true ? (
              <AwayIcon aria-label={`${participant.name} is away`} />
            ) : (
              participant.muted === true && (
                <MutedIcon aria-label={`${participant.name} is muted`} />
              )
            )}
            {/*
              Beside the precedence above rather than inside it, because it is
              a different question. Somebody muted with their camera on is
              ordinary, and so is the reverse, so these are two facts about one
              person rather than two candidates for one slot.
            */}
            {live && (
              <CameraIcon
                on={participant.camera === true}
                aria-label={
                  participant.camera === true
                    ? `${participant.name} has their camera on`
                    : `${participant.name} has their camera off`
                }
              />
            )}
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * What this session's call has to do with this row.
 *
 * Null for every channel except the one being joined, sat in, or last failed
 * on. The call carries its own room id precisely so that a second channel
 * clicked during a slow join does not take the first one's state with it.
 */
function callStateOf(channel: Channel, call: Call): Call["state"] | null {
  return callRoomId(call) === channel.id ? call.state : null;
}

/**
 * Who to draw under a voice channel.
 *
 * Two sources, and the better one wins for the one channel it covers. Room
 * state is what every channel this session is not sitting in has to use, and it
 * is only correct in the oldest MatrixRTC generation: in the current one it
 * shows nobody at all. The channel being sat in has a live roster from the call
 * itself, which is right in every generation and knows about this session
 * before any sync does.
 *
 * Only while connected. A join in flight has no roster yet, and a failed one
 * never will, so both keep whatever room state last said rather than blanking
 * a list that was correct a second ago.
 */
function peopleIn(
  channel: Channel,
  call: Call,
): { people: Participant[]; live: boolean } {
  if (call.state === "connected" && call.roomId === channel.id) {
    return { people: call.participants, live: true };
  }
  // `live` is not decoration on the list: it is what separates a camera that
  // is off from a camera nothing has looked at. Room state carries neither
  // cameras nor mutes, so everything drawn from it is a name and an avatar.
  return { people: channel.participants, live: false };
}

function ChannelRow({
  channel,
  selected,
  call,
  speaking,
  onSelect,
  onOpenPerson,
}: {
  channel: Channel;
  selected: boolean;
  call: Call;
  speaking: ReadonlySet<string>;
  onSelect: () => void;
  onOpenPerson: (
    person: Participant,
    roomId: string,
    at: { x: number; y: number },
  ) => void;
}) {
  const voice = channel.kind === "voice";
  const callState = voice ? callStateOf(channel, call) : null;
  const roster = peopleIn(channel, call);

  return (
    <li>
      <button
        type="button"
        className="channels__entry"
        data-selected={selected}
        data-kind={channel.kind}
        /*
          The call's own state, separate from selection. A channel can be
          selected without being joined and joined without being selected, and
          collapsing the two would mean clicking away from a voice channel
          looked like leaving it.
        */
        data-call={callState ?? undefined}
        /*
          A room this account is not in cannot be opened, so the control that
          would open it is disabled rather than absent. Hiding it would make
          Consort disagree with every other client about how many channels the
          space has.
        */
        disabled={!channel.joined}
        aria-current={selected ? "true" : undefined}
        title={
          channel.joined
            ? undefined
            : "This account has not joined this channel."
        }
        onClick={onSelect}
      >
        {voice ? <VoiceIcon /> : <span className="channels__hash">#</span>}
        <span className="channels__name">{channelLabel(channel)}</span>
      </button>
      {/*
        Beside the channel that was clicked rather than in the connection
        panel, because there is no connection to put it in: a failed join
        leaves this session exactly where it was, and the only thing worth
        saying is which channel would not take it and why.
      */}
      {callState === "failed" && call.state === "failed" && (
        <p className="channels__problem" role="alert">
          {call.error}
        </p>
      )}
      {/*
        Outside the button, deliberately. A person in the channel is not part
        of the control that opens it, and nesting them would make every name a
        target that opens the room instead.
      */}
      {voice && (
        <Participants
          channel={channel}
          people={roster.people}
          live={roster.live}
          speaking={speaking}
          onOpenPerson={onOpenPerson}
        />
      )}
    </li>
  );
}

function Group({
  label,
  channels,
  selectedId,
  call,
  speaking,
  onSelect,
  onOpenPerson,
}: {
  label: string;
  channels: Channel[];
  selectedId: string | null;
  call: Call;
  speaking: ReadonlySet<string>;
  onSelect: (id: string) => void;
  onOpenPerson: (
    person: Participant,
    roomId: string,
    at: { x: number; y: number },
  ) => void;
}) {
  // An empty group is no group. A "VOICE" header over nothing reads as a
  // channel list that failed to load rather than a space with no voice rooms.
  if (channels.length === 0) return null;

  return (
    <section className="channels__group" aria-label={label}>
      <h2 className="channels__label">{label}</h2>
      <ul className="channels__list">
        {channels.map((channel) => (
          <ChannelRow
            key={channel.id}
            channel={channel}
            selected={channel.id === selectedId}
            call={call}
            speaking={speaking}
            onSelect={() => onSelect(channel.id)}
            onOpenPerson={onOpenPerson}
          />
        ))}
      </ul>
    </section>
  );
}

interface Props {
  space: Space;
  selectedId: string | null;
  /**
   * What this session's voice call is doing, whichever space it is in.
   *
   * Passed whole rather than reduced to "is this one joined", because the
   * three states this list draws differently are not a boolean: connecting,
   * connected, and a failure that names a channel and a reason.
   */
  call: Call;
  /**
   * Who in the call is talking, by Matrix user ID.
   *
   * Drilled the way `call` is rather than put in a context, because it is the
   * same journey to the same place and one of them being different would be
   * the surprising thing. Defaulted to nobody so a caller that has no call to
   * describe does not have to invent an empty set.
   */
  speaking?: ReadonlySet<string>;
  onSelect: (id: string) => void;
}

/**
 * The channels of one rail entry, split into text and voice.
 *
 * Filtered rather than re-sorted. The order comes from Rust, which follows
 * MSC1772, and filtering preserves it: a channel keeps its place relative to
 * its neighbours even when it moves between the two groups.
 */
export function ChannelList({
  space,
  selectedId,
  call,
  speaking = NOBODY,
  onSelect,
}: Props) {
  const text = space.channels.filter((channel) => channel.kind === "text");
  const voice = space.channels.filter((channel) => channel.kind === "voice");
  // Which name was clicked, and where the pointer was when it happened. One at
  // a time: two open menus about two people would be two sliders somebody has
  // to tell apart by the heading.
  const [opened, setOpened] = useState<{
    person: Participant;
    roomId: string;
    at: { x: number; y: number };
  } | null>(null);

  return (
    <div className="channels">
      <header className="channels__header">
        <h2 className="channels__space" title={space.name}>
          {space.name}
        </h2>
      </header>

      {space.channels.length === 0 ? (
        <p className="channels__empty">Nothing in here yet.</p>
      ) : (
        <>
          <Group
            label="Text"
            channels={text}
            selectedId={selectedId}
            call={call}
            speaking={speaking}
            onSelect={onSelect}
            onOpenPerson={(person, roomId, at) =>
              setOpened({ person, roomId, at })
            }
          />
          <Group
            label="Voice"
            channels={voice}
            selectedId={selectedId}
            call={call}
            speaking={speaking}
            onSelect={onSelect}
            onOpenPerson={(person, roomId, at) =>
              setOpened({ person, roomId, at })
            }
          />
        </>
      )}

      {/*
        At the root of the list rather than inside the row that opened it, so
        that one menu is open at a time and it survives the row scrolling out
        from under it. Where it actually lands in the document is the menu's
        own business: it portals itself to the body, for the reason written
        beside it there.
      */}
      {opened !== null && (
        <PersonMenu
          key={opened.person.id}
          person={opened.person}
          roomId={opened.roomId}
          at={opened.at}
          onClose={() => setOpened(null)}
        />
      )}
    </div>
  );
}
