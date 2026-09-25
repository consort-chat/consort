import { useState } from "react";

import type { Channel, Space } from "../lib/api";
import { matchingChannels } from "../lib/channelSearch";
import { channelHeading, joinLabel } from "../lib/labels";
import { RoomAvatar } from "./RoomAvatar";
import { SidebarToggle } from "./SidebarToggle";
import type { Joining } from "./ChannelList";
import "./SpacePane.css";

/** How the lede counts what is here, and how much of it is still on offer. */
function countLine(channels: Channel[]): string {
  const total = channels.length;
  const unjoined = channels.filter((channel) => !channel.joined).length;
  const counted = `${total} ${total === 1 ? "channel" : "channels"}`;

  if (unjoined === 0) return `${counted} in here.`;
  return `${counted} in here, ${unjoined} you have not joined.`;
}

/** One channel, as something to press. */
function Row({
  channel,
  joining,
  onOpen,
  onJoin,
}: {
  channel: Channel;
  /** This session's join, when it is about this row. Null when it is not. */
  joining: Joining | null;
  onOpen: (roomId: string) => void;
  onJoin: (roomId: string) => void;
}) {
  const underway = joining !== null && joining.problem === null;

  return (
    <li>
      <button
        type="button"
        className="space__room"
        data-kind={channel.kind}
        /*
          The same rule the channel list follows: only this session's own join
          closes the row, and only while it is out. A refusal leaves it live,
          because a channel that was invite only this morning is one somebody
          may have been asked into since.
        */
        disabled={underway}
        /*
          Named explicitly, because the avatar beside the name is deliberately
          hidden from the accessible name: see [`RoomAvatar`].
        */
        aria-label={
          channel.joined
            ? channelHeading(channel)
            : joinLabel(channel, underway)
        }
        onClick={() =>
          channel.joined ? onOpen(channel.id) : onJoin(channel.id)
        }
      >
        <RoomAvatar
          roomId={channel.id}
          name={channelHeading(channel)}
          avatar={channel.avatar}
          className="space__avatar"
        />
        {/*
          The name and the topic stack, so a channel that set no topic is a row
          the height of its name rather than one with a gap under it. Most rows
          have no topic: a room nobody has joined has none to read, because the
          hierarchy response carries a name and a picture and nothing else.
        */}
        <span className="space__text">
          <span className="space__room-name">{channelHeading(channel)}</span>
          {channel.topic !== undefined && (
            <span className="space__room-topic">{channel.topic}</span>
          )}
        </span>
        {!channel.joined && (
          <span className="space__join" aria-hidden="true">
            {underway ? "Joining" : "Join"}
          </span>
        )}
      </button>
      {/*
        Beside the row it was about rather than at the top of the pane, the
        way the channel list says the same thing: a space with four unjoined
        channels has four reasons a join can fail and one of them belongs here.
      */}
      {joining?.problem != null && (
        <p className="space__problem" role="alert">
          {joining.problem}
        </p>
      )}
    </li>
  );
}

/**
 * The screen a space shows when no channel in it is selected.
 *
 * What was here was the opening screen, which is about everywhere: the rooms
 * this account was last in and the calls happening now, across every rail
 * entry. Clicking a space and being shown the same thing as clicking Home is
 * the half of #128 that says "right now we show nothing".
 *
 * So this pane is about one space and says the two things the column beside it
 * cannot. Which channels are here that this account is not in, which is what
 * makes them findable at all. And which channel is which, by its topic, at a
 * width that can hold one.
 *
 * Nothing here is asked of a homeserver. A space's children, unjoined ones
 * included, are already in the room list the shell holds: `hierarchy.rs` names
 * them once per space and folds the answer into the snapshot. The search is a
 * filter over a prop.
 */
export function SpacePane({
  space,
  joining = null,
  onOpen,
  onJoin,
  onUnfold,
}: {
  space: Space;
  /** The join this session has out, or the one that was refused. */
  joining?: Joining | null;
  /** Show a channel this account is in. Connects, if it is a voice one. */
  onOpen: (roomId: string) => void;
  /** Ask to be let into a channel this account is not in. */
  onJoin: (roomId: string) => void;
  /**
   * Bring the folded channel list back, when it is folded.
   *
   * This pane has no header to put the control in, the same way the room does,
   * so without it folding the list before picking a channel is a one-way door.
   */
  onUnfold?: () => void;
}) {
  /*
    Not focused on mount, deliberately. This pane is drawn on every rail click
    and every time a channel is deselected, and a field that took focus each
    time would take it off whatever somebody was actually using.
  */
  const [query, setQuery] = useState("");
  const shown = matchingChannels(space.channels, query);
  const searching = query.trim() !== "";

  return (
    <div className="space">
      {onUnfold !== undefined && (
        <div className="space__unfold">
          <SidebarToggle folded onToggle={onUnfold} />
        </div>
      )}

      <div className="space__sheet">
        <header className="space__lede">
          <h1 className="space__name">{space.name}</h1>
          {space.channels.length > 0 && (
            <p className="space__count">{countLine(space.channels)}</p>
          )}
        </header>

        {space.channels.length === 0 ? (
          <p className="space__none">Nothing in here yet.</p>
        ) : (
          <>
            <input
              type="search"
              className="space__search"
              aria-label={`Search the channels in ${space.name}`}
              placeholder="Search channels"
              spellCheck={false}
              autoComplete="off"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />

            <section className="space__section">
              <h2 className="space__heading">
                Channels
                {/*
                  How much of the list is on screen, which only needs saying
                  once some of it is not: a search that hid nine of twelve rows
                  would otherwise read as a space with three channels.
                */}
                <span className="space__tally">
                  {searching
                    ? `${shown.length} of ${space.channels.length}`
                    : space.channels.length}
                </span>
              </h2>
              {shown.length > 0 ? (
                <ul className="space__rooms" aria-label="Channels">
                  {shown.map((channel) => (
                    <Row
                      key={channel.id}
                      channel={channel}
                      joining={
                        joining?.roomId === channel.id ? joining : null
                      }
                      onOpen={onOpen}
                      onJoin={onJoin}
                    />
                  ))}
                </ul>
              ) : (
                <p className="space__none">
                  Nothing here matches &quot;{query.trim()}&quot;.
                </p>
              )}
            </section>
          </>
        )}
      </div>
    </div>
  );
}
