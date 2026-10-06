/**
 * One channel in the sidebar, and who is in it when it is a voice one.
 *
 * Split out of `ChannelList` when #170 added sections of somebody's own: the
 * list had outgrown the file-size limit and the row is the half of it that is
 * purely about drawing one channel.
 */
import {
  callRoomId,
  type Call,
  type Channel,
  type Participant,
} from "../lib/api";
import { channelLabel, joinLabel } from "../lib/labels";
import { CallFace } from "./CallFace";

/**
 * What a drag of a channel carries.
 *
 * A type of our own, on `SECTION`'s terms: a file dragged into the window
 * cannot be read as a channel and a channel cannot be dropped into a text box.
 */
export const ROOM = "application/x-consort-room";

/**
 * The grip that takes hold of a section or a channel.
 *
 * Two short bars rather than the usual six dots, because at this size dots
 * come out as a smudge and the lettering beside them is already detail.
 */
export function GripGlyph() {
  return (
    <svg
      className="channels__grip-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <path d="M5 9h14" />
      <path d="M5 15h14" />
    </svg>
  );
}

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
 * A speech bubble, for the control that reads a voice channel.
 *
 * Drawn from the same 24px grid as the speaker beside it, because the two are
 * on one row and an icon set that does not agree with itself looks like a
 * mistake at this size.
 */
function ChatIcon() {
  return (
    <svg
      className="channels__chat-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
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
   * The camera is only drawn when they did. See [`CallFace`].
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
      {people.map((participant) => (
        <CallFace
          key={participant.id}
          person={participant}
          roomId={channel.id}
          speaking={speaking.has(participant.id)}
          live={live}
          onOpen={(at) => onOpenPerson(participant, channel.id, at)}
        />
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

/**
 * How high either badge counts before it stops.
 *
 * Past this the number stops being information and starts being width. What
 * somebody does about nine of something and about ninety is the same thing,
 * and a badge that grows to four digits pushes the channel name it belongs to
 * off the row.
 */
const MOST_SHOWN = 99;

/** What a badge draws, which stops climbing at the cap rather than growing. */
function shown(count: number): string {
  return count > MOST_SHOWN ? `${MOST_SHOWN}+` : String(count);
}

/**
 * How many times somebody said your name here.
 *
 * Gold, and first claim on the one badge slot the row has. A mention is a
 * different claim from an unread message: it is about you rather than about
 * the channel, so when there are both it is the one drawn, and the unread fact
 * is still carried by the name in white beside it.
 */
function MentionBadge({ count, channel }: { count: number; channel: Channel }) {
  if (count === 0) return null;

  return (
    <span
      className="channels__mentions"
      /*
        The label carries the channel, because the badge is read out on its
        own: a screen reader landing on "3" beside a name it has already
        passed has been told a number and not what it counts.
      */
      aria-label={`${count} ${count === 1 ? "mention" : "mentions"} in ${channelLabel(channel)}`}
    >
      {shown(count)}
    </span>
  );
}

/**
 * How many messages are waiting here.
 *
 * White and unfilled, rather than a second pill beside the gold one. That is
 * the same treatment the channel's name already carries, said as a number, and
 * it is what keeps a mention the only thing in the list with a colour of its
 * own. Two filled badges on one row would make the reader compare them, and
 * they are not comparable.
 *
 * #53 argued this number was not worth drawing: what somebody does about forty
 * unread messages and about four is open the channel. Using it said otherwise.
 * White tells a reader something is waiting and not whether it is one message
 * or two hundred, and that is the difference between opening a channel now and
 * leaving it until after lunch. See #62.
 */
function UnreadBadge({ count, channel }: { count: number; channel: Channel }) {
  if (count === 0) return null;

  return (
    <span
      className="channels__unread"
      // Labelled for the same reason the mention badge is, and in the same
      // shape: a badge is read on its own and a bare number counts nothing.
      aria-label={`${count} unread ${count === 1 ? "message" : "messages"} in ${channelLabel(channel)}`}
    >
      {shown(count)}
    </span>
  );
}

/**
 * A join in flight, or the one that did not work.
 *
 * One value rather than two, because at most one of these is interesting at a
 * time: a second join cannot start while the first is out, and a failure is
 * about the room that was last asked for. `problem` is null while the request
 * is still going and a sentence for a person once it is not.
 */
export interface Joining {
  roomId: string;
  problem: string | null;
}

export function ChannelRow({
  channel,
  selected,
  call,
  speaking,
  joining,
  onSelect,
  onJoin,
  onOpenChat,
  onOpenPerson,
}: {
  channel: Channel;
  selected: boolean;
  call: Call;
  speaking: ReadonlySet<string>;
  /** This session's join, when it is about this row. Null when it is not. */
  joining: Joining | null;
  onSelect: () => void;
  /** Ask to be let into this channel. Only a row this account is not in. */
  onJoin: () => void;
  /** Show this channel's messages without connecting to it. Voice only. */
  onOpenChat: () => void;
  onOpenPerson: (
    person: Participant,
    roomId: string,
    at: { x: number; y: number },
  ) => void;
}) {
  const voice = channel.kind === "voice";
  const callState = voice ? callStateOf(channel, call) : null;
  const roster = peopleIn(channel, call);
  const underway = joining !== null && joining.problem === null;

  return (
    <li>
      {/*
        The row is two controls, not one. Clicking a voice channel connects to
        it, which is the right thing for the row to do and the wrong thing to
        have to do to read what was said in it: the messages are a room like
        any other, and every other client lets somebody open one without
        joining the call in it.
      */}
      <div
        className="channels__row"
        /*
          Whether the control that reads a voice channel is on this row. It is
          drawn over the row rather than in it, so the mention badge beside it
          has to leave its slot alone; see the stylesheet.
        */
        data-chat={voice && channel.joined}
      >
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
            Something has been said here since this account last looked, drawn
            as the name in white. A boolean because that is all the name can
            say; how many is the badge's job, and on a channel whose badge went
            to the mentions this stays the only thing saying anything is
            waiting at all.
          */
          data-unread={channel.unread > 0}
          /*
            A room this account is not in is a room to walk into, not a row to
            be refused by. Only this session's own join disables it, and only
            while it is out: a refusal leaves the control live, because a
            channel that was invite only this morning is one somebody may have
            been asked into since.
          */
          disabled={underway}
          aria-current={selected ? "true" : undefined}
          aria-label={channel.joined ? undefined : joinLabel(channel, underway)}
          title={
            channel.joined
              ? undefined
              : "This account has not joined this channel. Click to join it."
          }
          onClick={channel.joined ? onSelect : onJoin}
        >
          {voice ? <VoiceIcon /> : <span className="channels__hash">#</span>}
          <span className="channels__name">{channelLabel(channel)}</span>
          {/*
            Inside the control rather than beside it, so the whole row is the
            one target. A second button saying Join would be a second place to
            click that did what the first one does.
          */}
          {!channel.joined && (
            <span className="channels__join" aria-hidden="true">
              {underway ? "Joining" : "Join"}
            </span>
          )}
        </button>
        {/*
          Only on a voice channel, because a text one is already what this does.
          Absent rather than disabled on a channel this account is not in: the
          row beside it is disabled and says why in its own title, and a second
          dead control saying the same thing is noise on a row that is mostly
          empty space.
        */}
        {voice && channel.joined && (
          <button
            type="button"
            className="channels__chat"
            aria-label={`Read ${channelLabel(channel)} without connecting`}
            title="Read without connecting"
            onClick={onOpenChat}
          >
            <ChatIcon />
          </button>
        )}
        {/*
          Outside the button, beside the chat control rather than inside the
          thing that opens the channel. A count is a fact about the row and not
          a second target on it, and nesting it would make the number a
          separate place to click that did the same thing.

          Drawn whether or not the channel is selected. The selected channel is
          the one being read, so its count is on its way to zero anyway, and
          taking the badge away early would make a mention arriving in the room
          somebody is looking at the one mention they are never told about.
        */}
        <MentionBadge count={channel.mentions} channel={channel} />
        {/*
          One badge slot, and the mention has first claim on it. A channel with
          both draws the gold number alone: a mention already implies unread,
          so a second pill beside it tells a reader something they have worked
          out, and two numbers on one row is the list somebody has to read
          rather than scan that #53 was right to be wary of. The unread fact is
          not lost either way, because the name is white regardless.
        */}
        {channel.mentions === 0 && (
          <UnreadBadge count={channel.unread} channel={channel} />
        )}
        {/*
          A handle of its own rather than the row being draggable, for the
          reason a section's heading has one: clicking a voice channel connects
          to it, and a press that has to be told apart from the start of a drag
          is a press that sometimes does neither.

          A span, and out of the accessibility tree. It is the pointing
          device's shortcut and not the only way in: the checklist on a section
          does the same job with a checkbox, so a second control on every row
          would put the channel's name in the tree twice for no new ability.
        */}
        <span
          className="channels__room-grip"
          draggable
          aria-hidden="true"
          title="Drag onto a section heading"
          onDragStart={(event) => {
            event.dataTransfer.setData(ROOM, channel.id);
            event.dataTransfer.effectAllowed = "move";
          }}
        >
          <GripGlyph />
        </span>
      </div>
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
        The same treatment, for the same reason: the only thing worth saying
        about a join that was refused is which channel would not take it.
      */}
      {joining?.problem != null && (
        <p className="channels__problem" role="alert">
          {joining.problem}
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
