import type { Channel, Space } from "../lib/api";
import { channelLabel } from "../lib/labels";
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

function ChannelRow({
  channel,
  selected,
  onSelect,
}: {
  channel: Channel;
  selected: boolean;
  onSelect: () => void;
}) {
  const voice = channel.kind === "voice";

  return (
    <li>
      <button
        type="button"
        className="channels__entry"
        data-selected={selected}
        data-kind={channel.kind}
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
    </li>
  );
}

function Group({
  label,
  channels,
  selectedId,
  onSelect,
}: {
  label: string;
  channels: Channel[];
  selectedId: string | null;
  onSelect: (id: string) => void;
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
            onSelect={() => onSelect(channel.id)}
          />
        ))}
      </ul>
    </section>
  );
}

interface Props {
  space: Space;
  selectedId: string | null;
  onSelect: (id: string) => void;
}

/**
 * The channels of one rail entry, split into text and voice.
 *
 * Filtered rather than re-sorted. The order comes from Rust, which follows
 * MSC1772, and filtering preserves it: a channel keeps its place relative to
 * its neighbours even when it moves between the two groups.
 */
export function ChannelList({ space, selectedId, onSelect }: Props) {
  const text = space.channels.filter((channel) => channel.kind === "text");
  const voice = space.channels.filter((channel) => channel.kind === "voice");

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
            onSelect={onSelect}
          />
          <Group
            label="Voice"
            channels={voice}
            selectedId={selectedId}
            onSelect={onSelect}
          />
        </>
      )}
    </div>
  );
}
