import { useEffect, useRef, useState } from "react";

import {
  NOBODY,
  asCommandError,
  createSection,
  deleteSection,
  renameSection,
  setRoomSection,
  setSectionFolded,
  setSectionOrder,
  sidebarSettings,
  type Call,
  type CustomSection,
  type Participant,
  type Space,
} from "../lib/api";
import { BUILT_IN, arrange, moved, sectionsOf } from "../lib/sections";
import { NewSection } from "./NewSection";
import { ChannelGroup } from "./ChannelGroup";
import type { Joining } from "./ChannelRow";
import { PersonMenu } from "./PersonMenu";
import { SidebarToggle } from "./SidebarToggle";
import "./ChannelList.css";

export type { Joining } from "./ChannelRow";

/** Nothing folded, which is how a fresh account finds the sidebar. */
const NO_SECTIONS: ReadonlySet<string> = new Set();

/** Nothing dragged, which means the order `BUILT_IN` is written in. */
const NO_ORDER: readonly string[] = [];

/** No sections of anybody's own, which is how a fresh account starts. */
const NO_MADE: readonly CustomSection[] = [];

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
 * The channels of one rail entry, in Text, Voice, and whatever sections
 * somebody made.
 *
 * Filtered rather than re-sorted. The order comes from Rust, which follows
 * MSC1772, and filtering preserves it: a channel keeps its place relative to
 * its neighbours whichever section it moves into. See [`sectionsOf`].
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
  // `BUILT_IN` is written in.
  const [order, setOrder] = useState<readonly string[]>(NO_ORDER);
  // The sections somebody made, across every space. Filtered to this one on
  // the way to `sectionsOf`: a section made in one space has no business being
  // drawn in another.
  const [made, setMade] = useState<readonly CustomSection[]>(NO_MADE);
  // What a refused create or rename said, for the one place a person is
  // waiting on an answer. A fold or a drag only logs; see `report`.
  const [problem, setProblem] = useState<string | null>(null);
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

  const mine = made.filter((section) => section.space === space.id);
  const drawn = sectionsOf(space.channels, mine, order);
  /*
    Every section's key, drawn or not, which is what gets written down. A
    section this space has nothing in is still one somebody arranged, so
    leaving its key out of the order would move it to the end the first time
    anything else was dragged.
  */
  const every = arrange(
    [...BUILT_IN, ...mine.map((section) => ({ key: section.key }))],
    order,
  ).map((section) => section.key);

  useEffect(() => {
    sidebarSettings().then(
      (settings) => {
        setFolded(new Set(settings.folded));
        setOrder(settings.order);
        setMade(settings.sections);
      },
      (raw: unknown) => {
        // Drawn open and in the order `BUILT_IN` is written in. Failing to
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
    const next = moved(every, key, onto);
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

  /** What a write nobody is waiting on does when it fails. */
  function report(what: string) {
    return (raw: unknown) => {
      console.error(what, asCommandError(raw).detail);
    };
  }

  /**
   * Make a section named `name` under this space.
   *
   * The one write here that is not optimistic: the key comes from the file, so
   * there is nothing to draw until it answers.
   */
  function create(name: string) {
    setProblem(null);
    createSection(space.id, name).then(
      (key) => {
        setMade((was) => [
          ...was,
          { key, name: name.trim(), space: space.id, rooms: [] },
        ]);
      },
      (raw: unknown) => setProblem(asCommandError(raw).message),
    );
  }

  /** Give the section keyed `key` a new name, and write that down. */
  function rename(key: string, name: string) {
    setProblem(null);
    setMade((was) =>
      was.map((one) => (one.key === key ? { ...one, name: name.trim() } : one)),
    );
    renameSection(key, name).catch((raw: unknown) =>
      setProblem(asCommandError(raw).message),
    );
  }

  /** Forget a section. Its channels are drawn under Text and Voice again. */
  function remove(key: string) {
    setMade((was) => was.filter((one) => one.key !== key));
    deleteSection(key).catch(report("could not delete the section"));
  }

  /**
   * Put `roomId` in the section keyed `key`, or in none when `key` is null.
   *
   * Taken out of every other section here as well as in Rust, because a room
   * is in one at a time and this side has to agree with the file about which.
   */
  function assign(roomId: string, key: string | null) {
    setMade((was) =>
      was.map((one) => ({
        ...one,
        rooms:
          one.key === key
            ? [...one.rooms.filter((id) => id !== roomId), roomId]
            : one.rooms.filter((id) => id !== roomId),
      })),
    );
    setRoomSection(roomId, key).catch(report("could not move the channel"));
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

      {space.channels.length === 0 && drawn.length === 0 ? (
        <p className="channels__empty">Nothing in here yet.</p>
      ) : (
        drawn.map((section) => (
          <ChannelGroup
            key={section.key}
            sectionKey={section.key}
            label={section.label}
            channels={section.channels}
            custom={section.custom}
            inSpace={space.channels}
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
            onRename={(name) => rename(section.key, name)}
            onDelete={() => remove(section.key)}
            onChoose={(roomId, inside) =>
              assign(roomId, inside ? section.key : null)
            }
            /*
              Dropped on Text or Voice means out of every section somebody
              made, because those two hold whatever is left over.
            */
            onTakeRoom={(roomId) =>
              assign(roomId, section.custom ? section.key : null)
            }
            onSelect={onSelect}
            onJoin={onJoin}
            onOpenChat={onOpenRoom}
            onOpenPerson={open}
          />
        ))
      )}

      <NewSection onCreate={create} />
      {/*
        The one place somebody is waiting on an answer, so the one write here
        that says so rather than only logging. A fold and a drag have already
        moved the list; see `report`.
      */}
      {problem !== null && (
        <p className="channels__problem" role="alert">
          {problem}
        </p>
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
