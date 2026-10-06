import {
  useEffect,
  useId,
  useRef,
  useState,
  type DragEvent as ReactDragEvent,
} from "react";

import {
  NOBODY,
  asCommandError,
  callRoomId,
  setSectionFolded,
  setSectionOrder,
  sidebarSettings,
  type Call,
  type Channel,
  type Participant,
  type Space,
} from "../lib/api";
import { channelLabel, joinLabel } from "../lib/labels";
import { arrange, moved } from "../lib/sections";
import { CallFace } from "./CallFace";
import { PersonMenu } from "./PersonMenu";
import { SidebarToggle } from "./SidebarToggle";
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

/** Nothing folded, which is how a fresh account finds the sidebar. */
const NO_SECTIONS: ReadonlySet<string> = new Set();

/** Nothing dragged, which means the order `SECTIONS` is written in. */
const NO_ORDER: readonly string[] = [];

/**
 * A chevron, pointing the way the section will move.
 *
 * The same device the sidebar's own fold uses, turned a quarter: down means
 * the channels are under it, right means they are put away.
 */
function FoldGlyph({ folded }: { folded: boolean }) {
  return (
    <svg
      className="channels__fold-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={folded ? "M9 6l6 6-6 6" : "M6 9l6 6 6-6"} />
    </svg>
  );
}

/**
 * What a drag of a section carries.
 *
 * A type of our own rather than `text/plain`, so that a drag from anywhere
 * else cannot be read as a section and dropping a section somewhere else
 * cannot paste a key into it.
 */
const SECTION = "application/x-consort-section";

/**
 * The grip on a section's heading, which is what takes hold of it.
 *
 * Two short bars rather than the usual six dots, because at this size dots
 * come out as a smudge and the heading beside them is already lettering.
 */
function GripGlyph() {
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

function ChannelRow({
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

function Group({
  sectionKey,
  label,
  channels,
  folded,
  dragged,
  over,
  selectedId,
  call,
  speaking,
  joining,
  onToggleFold,
  onTake,
  onDrop,
  onOver,
  onMove,
  onSelect,
  onJoin,
  onOpenChat,
  onOpenPerson,
}: {
  /** What the settings file calls this section. See [`SECTIONS`]. */
  sectionKey: string;
  label: string;
  channels: Channel[];
  /** Whether the channels under the heading are put away right now. */
  folded: boolean;
  /** Whether this is the section being dragged right now. */
  dragged: boolean;
  /** Whether a dragged section is over this one, so it is where it would land. */
  over: boolean;
  selectedId: string | null;
  call: Call;
  speaking: ReadonlySet<string>;
  joining: Joining | null;
  /** Fold this section away, or bring it back. */
  onToggleFold: () => void;
  /** Take hold of this section, or let go of it. */
  onTake: (dragging: boolean) => void;
  /** Put the section keyed `key` where this one is. */
  onDrop: (key: string) => void;
  /** A dragged section is over this one, or has left it. */
  onOver: (over: boolean) => void;
  /** Move this section one place up (-1) or down (1). */
  onMove: (by: number) => void;
  onSelect: (id: string) => void;
  /** Ask to be let into a channel this account is not in. */
  onJoin: (id: string) => void;
  /** Show a channel without connecting. Only a voice row draws the control. */
  onOpenChat: (id: string) => void;
  onOpenPerson: (
    person: Participant,
    roomId: string,
    at: { x: number; y: number },
  ) => void;
}) {
  const listId = useId();

  /** Whether a drag over this section is one of ours. See [`SECTION`]. */
  function ours(event: ReactDragEvent): boolean {
    return event.dataTransfer.types.includes(SECTION);
  }

  return (
    <section
      className="channels__group"
      aria-label={label}
      data-dragged={dragged}
      data-over={over}
      /*
        `preventDefault` on a drag over is what offers the drop at all, so it
        is deliberately conditional: without the check a file dragged into the
        window would be told the sidebar will take it.
      */
      onDragOver={(event) => {
        if (!ours(event)) return;
        event.preventDefault();
        onOver(true);
      }}
      onDragLeave={() => onOver(false)}
      onDrop={(event) => {
        if (!ours(event)) return;
        event.preventDefault();
        onDrop(event.dataTransfer.getData(SECTION));
      }}
    >
      <h2 className="channels__label">
        {/*
          A real disclosure control inside the heading, rather than a clickable
          heading. `aria-expanded` carries the state and the name stays put
          across a press, the way the call panel does it: a button renamed
          under the cursor is announced as a different button each time. The
          tooltip is where the wording is allowed to follow the state.
        */}
        <button
          type="button"
          className="channels__fold"
          aria-expanded={!folded}
          aria-controls={listId}
          title={
            folded ? `Show the ${label} channels` : `Hide the ${label} channels`
          }
          onClick={onToggleFold}
        >
          <FoldGlyph folded={folded} />
          {label}
        </button>
        {/*
          A handle of its own rather than the heading being draggable. The
          heading is already the control that folds the section, and a press
          that has to be told apart from the start of a drag is a press that
          sometimes does neither.

          The arrow keys do the same job, because a drag is a route some people
          do not have. Said in the tooltip, which an `aria-label` beside it
          turns into the control's description rather than a second name.
        */}
        <button
          type="button"
          className="channels__grip"
          draggable
          aria-label={`Move ${label}`}
          title="Drag to move, or press the up and down arrows"
          onDragStart={(event) => {
            event.dataTransfer.setData(SECTION, sectionKey);
            event.dataTransfer.effectAllowed = "move";
            onTake(true);
          }}
          onDragEnd={() => onTake(false)}
          onKeyDown={(event) => {
            const by = { ArrowUp: -1, ArrowDown: 1 }[event.key];
            if (by === undefined) return;
            event.preventDefault();
            onMove(by);
          }}
        >
          <GripGlyph />
        </button>
      </h2>
      {/*
        Hidden rather than unmounted, so the heading's `aria-controls` points
        at something that exists in both states.
      */}
      <ul className="channels__list" id={listId} hidden={folded}>
        {channels.map((channel) => (
          <ChannelRow
            key={channel.id}
            channel={channel}
            selected={channel.id === selectedId}
            call={call}
            speaking={speaking}
            joining={joining?.roomId === channel.id ? joining : null}
            onSelect={() => onSelect(channel.id)}
            onJoin={() => onJoin(channel.id)}
            onOpenChat={() => onOpenChat(channel.id)}
            onOpenPerson={onOpenPerson}
          />
        ))}
      </ul>
    </section>
  );
}

/**
 * The sections the sidebar draws, in the order they are drawn.
 *
 * One list rather than two calls, so that reordering them and #170 adding to
 * them are changes to data rather than to markup. The key is what the settings
 * file holds, so it is not the label: a label is wording and can be reworded.
 *
 * The order here is only the one nobody has changed. See [`arrange`].
 */
const SECTIONS: { key: string; label: string; kind: Channel["kind"] }[] = [
  { key: "text", label: "Text", kind: "text" },
  { key: "voice", label: "Voice", kind: "voice" },
];

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
  /** Whoever is signed in, so a person's card can tell when it is about them. */
  selfId: string;
  /**
   * The join this session has out, or the one that was refused.
   *
   * Held by the shell rather than here, on `callRefused`'s terms: it outlives
   * the request that made it, and a component that owned it would clear it on
   * every re-render caused by anything else in the list.
   *
   * Absent when nothing has been asked for, which is almost always.
   */
  joining?: Joining | null;
  onSelect: (id: string) => void;
  /** Ask to be let into a channel this account is not in. */
  onJoin: (id: string) => void;
  /** Show a room, by ID. Passed to a person's card for its Message button. */
  onOpenRoom: (roomId: string) => void;
  /** Fold this column away. The control that brings it back is elsewhere. */
  onFold: () => void;
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
  selfId,
  joining = null,
  onSelect,
  onJoin,
  onOpenRoom,
  onFold,
}: Props) {
  // Which sections are put away. Read from the settings file rather than kept
  // in the webview's storage, which is where every other preference here
  // lives; see `SidebarSettings`.
  const [folded, setFolded] = useState<ReadonlySet<string>>(NO_SECTIONS);
  // The keys somebody dragged these into, or none, which means the order
  // `SECTIONS` is written in.
  const [order, setOrder] = useState<readonly string[]>(NO_ORDER);
  // The section being dragged and the one it is over, for the stylesheet.
  const [dragged, setDragged] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);
  // Which name was clicked, and where to draw the card about them. One at a
  // time: two open menus about two people would be two sliders somebody has to
  // tell apart by the heading.
  const [opened, setOpened] = useState<{
    person: Participant;
    roomId: string;
    at: { x: number; y: number };
  } | null>(null);
  const list = useRef<HTMLDivElement | null>(null);

  const arranged = arrange(SECTIONS, order);
  /*
    The sections with something in them, which is all that is drawn. An empty
    group is no group: a "VOICE" header over nothing reads as a channel list
    that failed to load rather than a space with no voice rooms.

    Separate from `arranged` because a move is between neighbours somebody can
    see, while what gets written down is the whole order.
  */
  const drawn = arranged
    .map((section) => ({
      ...section,
      channels: space.channels.filter(
        (channel) => channel.kind === section.kind,
      ),
    }))
    .filter((section) => section.channels.length > 0);

  useEffect(() => {
    sidebarSettings().then(
      (settings) => {
        setFolded(new Set(settings.folded));
        setOrder(settings.order);
      },
      (raw: unknown) => {
        // Drawn open and in the order `SECTIONS` is written in. Failing to
        // remember a fold is not a reason to draw a sidebar with nothing in
        // it.
        console.error(
          "could not read which sections are folded",
          asCommandError(raw).detail,
        );
      },
    );
  }, []);

  /** Put a section away, or bring it back, and write that down. */
  function toggleFold(key: string) {
    const away = !folded.has(key);
    const next = new Set(folded);
    if (away) next.add(key);
    else next.delete(key);
    setFolded(next);

    // The list has already moved, and it stays moved if the write fails:
    // putting it back because the file would not take the fold would be a
    // press that undoes itself.
    setSectionFolded(key, away).catch((raw: unknown) => {
      console.error(
        "could not remember the folded sections",
        asCommandError(raw).detail,
      );
    });
  }

  /** Put `key` where the section keyed `onto` is, and write that down. */
  function rearrange(key: string, onto: string) {
    const next = moved(
      arranged.map((section) => section.key),
      key,
      onto,
    );
    setOrder(next);

    // The list has already moved, on the fold's terms: putting it back
    // because the file would not take the order would be a drag that undoes
    // itself.
    setSectionOrder(next).catch((raw: unknown) => {
      console.error(
        "could not remember the order of the sections",
        asCommandError(raw).detail,
      );
    });
  }

  /** Move a section one place up or down the list, past the drawn ones only. */
  function shift(key: string, by: number) {
    const at = drawn.findIndex((section) => section.key === key);
    const neighbour = drawn[at + by];
    if (neighbour === undefined) return;
    rearrange(key, neighbour.key);
  }

  /*
    Beside the sidebar rather than under the pointer, which is where this used
    to open and where it cannot stay.

    The sidebar scrolls, and WebKitGTK draws the scrollbar of a scroll container
    on top of the page rather than inside it. It is not in anybody's stacking
    order, so no `z-index` reaches over it and moving the card out of the
    subtree does not either: a card overlapping this column comes out with a
    grey bar down the middle of it whatever the DOM says. The only fix that
    holds is to not overlap the column.

    It is also the better place for it. A card opened at the pointer covers the
    list somebody is reading, and every other client puts the person beside the
    names rather than over them.
  */
  function open(
    person: Participant,
    roomId: string,
    at: { x: number; y: number },
  ) {
    const column = list.current?.getBoundingClientRect();
    setOpened({ person, roomId, at: { x: column?.right ?? at.x, y: at.y } });
  }

  return (
    <div className="channels" ref={list}>
      <header className="channels__header">
        <h2 className="channels__space" title={space.name}>
          {space.name}
        </h2>
        <SidebarToggle folded={false} onToggle={onFold} />
      </header>

      {space.channels.length === 0 ? (
        <p className="channels__empty">Nothing in here yet.</p>
      ) : (
        drawn.map((section) => (
          <Group
            key={section.key}
            sectionKey={section.key}
            label={section.label}
            channels={section.channels}
            folded={folded.has(section.key)}
            dragged={dragged === section.key}
            over={over === section.key && dragged !== section.key}
            selectedId={selectedId}
            call={call}
            speaking={speaking}
            joining={joining}
            onToggleFold={() => toggleFold(section.key)}
            onTake={(taken) => {
              setDragged(taken ? section.key : null);
              if (!taken) setOver(null);
            }}
            onOver={(on) => setOver(on ? section.key : null)}
            onDrop={(key) => {
              setOver(null);
              rearrange(key, section.key);
            }}
            onMove={(by) => shift(section.key, by)}
            onSelect={onSelect}
            onJoin={onJoin}
            onOpenChat={onOpenRoom}
            onOpenPerson={open}
          />
        ))
      )}

      {/*
        At the root of the list rather than inside the row that opened it, so
        that one menu is open at a time and it survives the row scrolling out
        from under it.
      */}
      {opened !== null && (
        <PersonMenu
          key={opened.person.id}
          person={opened.person}
          roomId={opened.roomId}
          selfId={selfId}
          at={opened.at}
          onClose={() => setOpened(null)}
          onOpenRoom={onOpenRoom}
        />
      )}
    </div>
  );
}
