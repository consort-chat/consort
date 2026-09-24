import { useEffect, useState } from "react";

import {
  asCommandError,
  callRoomId,
  recentRooms,
  type Call,
  type Channel,
  type Participant,
  type Rooms,
} from "../lib/api";
import { channelHeading } from "../lib/labels";
import { ExternalLink } from "./ExternalLink";
import { RoomAvatar } from "./RoomAvatar";
import { SidebarToggle } from "./SidebarToggle";
import "./OpeningPane.css";

/** Where the source is, for anybody who wants to read it or file something. */
const REPOSITORY = "https://github.com/consort-chat/consort";

/**
 * How many recent rooms are drawn.
 *
 * Fewer than are remembered, which is the point of remembering more: rooms
 * that have since been left resolve against nothing and drop out silently, so
 * the list stays this long rather than shortening every time somebody leaves
 * somewhere. Long enough to be a shortcut, short enough to be read at a glance
 * rather than scanned, which is what would make it a second channel list.
 */
const SHOWN = 6;

/** A room, and the rail entry it hangs under. */
interface Placed {
  spaceId: string;
  spaceName: string;
  channel: Channel;
}

/** Where a room is, or null for one this account is no longer in. */
function placed(rooms: Rooms, roomId: string): Placed | null {
  for (const space of rooms.spaces) {
    const channel = space.channels.find((candidate) => candidate.id === roomId);
    if (channel !== undefined) {
      return { spaceId: space.id, spaceName: space.name, channel };
    }
  }
  return null;
}

/**
 * The voice channels somebody is sitting in, across every rail entry.
 *
 * Every one of them, rather than the selected space's, which is the whole
 * reason this is worth drawing: the sidebar shows one space at a time, so a
 * call in any other is a call nobody sees until they go looking.
 *
 * Read straight off the room list. Element Call announces a connection by
 * writing room state, so this is a reading of something the account already
 * has rather than anything asked of a homeserver.
 *
 * In the order the room list is in, which is name order within a space and
 * does not move between renders. A list that sorted itself by how many people
 * were in each channel would reorder under the pointer as somebody joined.
 */
function liveNow(rooms: Rooms, call: Call): Placed[] {
  // The channel this session is sitting in is left out. The call panel and
  // the call card both already say where it is, and a third row offering to
  // walk into the room somebody is standing in is a control with nothing to
  // do.
  const here = callRoomId(call);

  return rooms.spaces.flatMap((space) =>
    space.channels
      .filter(
        (channel) =>
          channel.kind === "voice" &&
          channel.participants.length > 0 &&
          channel.id !== here,
      )
      .map((channel) => ({
        spaceId: space.id,
        spaceName: space.name,
        channel,
      })),
  );
}

/**
 * Who is in a voice channel, said the way the typing line says it.
 *
 * Two names and then a count. A row here is a nudge rather than a roster, and
 * six names is wider than this pane on a laptop.
 */
function whoIsIn(people: Participant[]): string {
  const names = people.map((person) => person.name);
  // One name on its own, and two joined, both fall out of the same join.
  if (names.length <= 2) return names.join(" and ");

  const others = names.length - 2;
  return `${names[0]}, ${names[1]} and ${others} ${others === 1 ? "other" : "others"}`;
}

/** What a row hands back when it is pressed. */
type Open = (spaceId: string, channel: Channel) => void;

/**
 * One room, as something to press.
 *
 * Named explicitly rather than left to its contents, because the avatar beside
 * the name is deliberately hidden from the accessible name: see [`RoomAvatar`].
 */
function Row({
  place,
  label,
  detail,
  onOpen,
}: {
  place: Placed;
  /** What a screen reader is told the control is for. */
  label: string;
  /** The quiet second line: where the room is, or who is in it. */
  detail: string;
  onOpen: Open;
}) {
  const { spaceId, channel } = place;
  const name = channelHeading(channel);

  return (
    <button
      type="button"
      className="opening__room"
      aria-label={label}
      onClick={() => onOpen(spaceId, channel)}
    >
      <RoomAvatar
        roomId={channel.id}
        name={name}
        avatar={channel.avatar}
        className="opening__avatar"
      />
      <span className="opening__room-name">{name}</span>
      <span className="opening__room-detail">{detail}</span>
    </button>
  );
}

/** A voice channel with people in it, said as who they are. */
function LiveRow({ place, onOpen }: { place: Placed; onOpen: Open }) {
  const who = whoIsIn(place.channel.participants);
  const name = channelHeading(place.channel);

  return (
    <Row
      place={place}
      label={`${name}, with ${who}`}
      detail={who}
      onOpen={onOpen}
    />
  );
}

/** A room somebody was last in, said as the rail entry it hangs under. */
function RecentRow({ place, onOpen }: { place: Placed; onOpen: Open }) {
  const name = channelHeading(place.channel);

  return (
    <Row
      place={place}
      label={`${name}, in ${place.spaceName}`}
      detail={place.spaceName}
      onOpen={onOpen}
    />
  );
}

/**
 * The screen the application opens on.
 *
 * What was here was a sentence saying there was nothing here, which is the one
 * screen every session meets and the one that said the least (#83). This is
 * the same pane doing something: where somebody was last, what is happening in
 * voice right now, and the way to the source.
 *
 * Everything on it is drawn from what is already local. The room list is a
 * prop, and the recency is a file this application wrote itself, so nothing
 * here waits on a homeserver: this pane sits between signing in and seeing
 * anything, and a request in it would be a request in front of the whole
 * application.
 *
 * Good with nothing, deliberately. An account in no rooms is a person who has
 * just signed in for the first time and is least sure this is working, so that
 * state is a screen that says where rooms come from rather than a heading over
 * an empty box.
 */
export function OpeningPane({
  rooms,
  call,
  onOpen,
  onUnfold,
}: {
  /** The room list the shell holds, which everything here is resolved against. */
  rooms: Rooms;
  /** Where this session's own call is, so the row for it can be left out. */
  call: Call;
  onOpen: (spaceId: string, channel: Channel) => void;
  /**
   * Bring the folded channel list back, when it is folded.
   *
   * This pane has no header to put the control in, the same way the room does,
   * so without it folding the list before picking a channel is a one-way door.
   */
  onUnfold?: () => void;
}) {
  /*
    Null until the answer is in, which is a file read rather than anything
    remote. Not knowing and knowing there is nothing are drawn differently on
    purpose: the line about what will fill the list would otherwise flash up
    on the screen of somebody who has six rooms to be offered.
  */
  const [recent, setRecent] = useState<string[] | null>(null);

  useEffect(() => {
    let cancelled = false;

    recentRooms()
      .then((ids) => {
        if (!cancelled) setRecent(ids);
      })
      .catch((raw: unknown) => {
        // Which rooms somebody was last in is not worth a blank screen. The
        // rest of the pane draws, and the list says what will fill it.
        console.error(
          "could not read the recent rooms",
          asCommandError(raw).detail,
        );
        if (!cancelled) setRecent([]);
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const anyChannels = rooms.spaces.some((space) => space.channels.length > 0);
  const busy = liveNow(rooms, call);
  /*
    A room with somebody in it is drawn once, above, and not again here. Two
    identical rows a few centimetres apart read as a mistake, and of the two
    the live one is the better: it says who is in there.

    Dropped before the list is cut to length rather than after, so the row it
    gives up is filled from further down. A list that came out five long
    because one room happened to be busy would be shorter for no reason a
    reader could see.
  */
  const live = new Set(busy.map((place) => place.channel.id));
  const shown = (recent ?? [])
    .map((roomId) => placed(rooms, roomId))
    .filter(
      (room): room is Placed => room !== null && !live.has(room.channel.id),
    )
    .slice(0, SHOWN);

  return (
    <div className="opening">
      {onUnfold !== undefined && (
        <div className="opening__unfold">
          <SidebarToggle folded onToggle={onUnfold} />
        </div>
      )}

      <div className="opening__sheet">
        <header className="opening__lede">
          <h1 className="opening__wordmark">Consort</h1>
          <p className="opening__tagline">
            {anyChannels
              ? "Pick a channel to read it. Clicking a voice one joins it as well."
              : "Consort draws the rooms this account has already joined. Join one from another client and it will turn up in the rail down the left, with its channels in the column beside it."}
          </p>
        </header>

        {/*
          Above the recent rooms, because it is the only thing on this screen
          that expires. A room somebody was in yesterday will still be there
          after lunch; a call happening now will not.
        */}
        {busy.length > 0 && (
          <section className="opening__section" aria-labelledby="opening-live">
            <h2
              className="opening__heading opening__heading--live"
              id="opening-live"
            >
              Live now
            </h2>
            <ul className="opening__rooms" aria-label="Live now">
              {busy.map((place) => (
                /*
                  Keyed by both, because a room can be a child of two spaces
                  and is then drawn under each of them, exactly as the rail
                  draws it twice.
                */
                <li key={`${place.spaceId}/${place.channel.id}`}>
                  <LiveRow place={place} onOpen={onOpen} />
                </li>
              ))}
            </ul>
          </section>
        )}

        {/*
          Left out entirely for an account in no rooms. A heading saying Recent
          over a line saying nothing has been opened yet is the screen this
          replaced, said twice.
        */}
        {anyChannels && (
          <section className="opening__section" aria-labelledby="opening-recent">
            <h2 className="opening__heading" id="opening-recent">
              Recent
            </h2>
            {shown.length > 0 ? (
              <ul className="opening__rooms" aria-label="Recent">
                {shown.map((place) => (
                  <li key={`${place.spaceId}/${place.channel.id}`}>
                    <RecentRow place={place} onOpen={onOpen} />
                  </li>
                ))}
              </ul>
            ) : (
              recent !== null && (
                <p className="opening__none">Channels you open show up here.</p>
              )
            )}
          </section>
        )}
      </div>

      <p className="opening__footnote">
        <ExternalLink href={REPOSITORY} className="opening__link">
          Consort on GitHub
        </ExternalLink>
      </p>
    </div>
  );
}
