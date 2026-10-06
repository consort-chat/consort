/**
 * One section of the sidebar, with its heading and the channels under it.
 *
 * Split out of `ChannelList` alongside `ChannelRow`, for the same reason.
 */
import { useId, useState, type DragEvent as ReactDragEvent } from "react";

import type { Call, Channel, Participant } from "../lib/api";
import { MAX_SECTION_NAME } from "../lib/sections";
import {
  ChannelRow,
  GripGlyph,
  ROOM,
  type Joining,
} from "./ChannelRow";
import { SectionPicker } from "./SectionPicker";

/**
 * A pencil, for the control that renames a section.
 *
 * Drawn from the same 24px grid as the chevron and the grip beside it, because
 * the three are on one row.
 */
function PencilGlyph() {
  return (
    <svg
      className="channels__manage-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" />
    </svg>
  );
}

/** A bin, for the control that deletes a section. See [`PencilGlyph`]. */
function BinGlyph() {
  return (
    <svg
      className="channels__manage-glyph"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M4 7h16" />
      <path d="M10 11v6M14 11v6" />
      <path d="M6 7l1 13h10l1-13" />
      <path d="M9 7V4h6v3" />
    </svg>
  );
}

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
export const SECTION = "application/x-consort-section";

export function ChannelGroup({
  sectionKey,
  label,
  channels,
  custom,
  inSpace,
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
  onRename,
  onDelete,
  onChoose,
  onTakeRoom,
  onSelect,
  onJoin,
  onOpenChat,
  onOpenPerson,
}: {
  /** What the settings file calls this section. See [`BUILT_IN`]. */
  sectionKey: string;
  label: string;
  channels: Channel[];
  /**
   * Whether somebody made this one.
   *
   * Text and Voice follow `m.room.type` rather than a choice, so they have no
   * name to change and nothing to delete. A section of somebody's own has
   * both, and is the only one that takes a channel dropped on it.
   */
  custom: boolean;
  /** Every channel in the space, for the checklist. Only a custom section. */
  inSpace: Channel[];
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
  /** Give this section a new name. Only a custom one draws the control. */
  onRename: (name: string) => void;
  /** Forget this section. Its channels go back to Text or Voice. */
  onDelete: () => void;
  /** Put a channel in this section, or take it back out. */
  onChoose: (roomId: string, inside: boolean) => void;
  /** A channel was dropped on this heading. */
  onTakeRoom: (roomId: string) => void;
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
  // The name being typed, or null when the heading is a heading. Here rather
  // than in the list, so one section being renamed leaves the others alone.
  const [draft, setDraft] = useState<string | null>(null);
  const held = new Set(channels.map((channel) => channel.id));

  /** What a drag over this section is carrying, when it is one of ours. */
  function ours(event: ReactDragEvent): "section" | "room" | null {
    if (event.dataTransfer.types.includes(SECTION)) return "section";
    if (event.dataTransfer.types.includes(ROOM)) return "room";
    return null;
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
        if (ours(event) === null) return;
        event.preventDefault();
        onOver(true);
      }}
      onDragLeave={() => onOver(false)}
      onDrop={(event) => {
        const carrying = ours(event);
        if (carrying === null) return;
        event.preventDefault();
        onOver(false);
        if (carrying === "section") {
          onDrop(event.dataTransfer.getData(SECTION));
        } else {
          onTakeRoom(event.dataTransfer.getData(ROOM));
        }
      }}
    >
      <h2 className="channels__label">
        {draft !== null ? (
          /*
            A form, so Enter saves and Escape is the only key that needs
            handling. The old name is what it opens on: a rename is usually a
            correction rather than a fresh start.
          */
          <form
            className="channels__rename"
            onSubmit={(event) => {
              event.preventDefault();
              onRename(draft);
              setDraft(null);
            }}
          >
            <input
              className="channels__name-box"
              aria-label={`Rename ${label}`}
              value={draft}
              maxLength={MAX_SECTION_NAME}
              autoFocus
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") setDraft(null);
              }}
            />
            <button
              type="submit"
              className="channels__save"
              disabled={draft.trim() === ""}
            >
              Save
            </button>
            <button
              type="button"
              className="channels__cancel"
              onClick={() => setDraft(null)}
            >
              Cancel
            </button>
          </form>
        ) : (
          <>
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
        {/*
          Only a section somebody made. Text and Voice follow `m.room.type`,
          so there is no name of theirs to change and nothing to delete.

          Faded until the section is hovered or something in it has focus, and
          never hidden from the keyboard: the stylesheet moves the opacity and
          leaves the tab order alone.
        */}
        {custom && (
          <>
            <button
              type="button"
              className="channels__manage"
              aria-label={`Rename ${label}`}
              title="Rename"
              onClick={() => setDraft(label)}
            >
              <PencilGlyph />
            </button>
            <button
              type="button"
              className="channels__manage"
              aria-label={`Delete ${label}`}
              /*
                No confirmation, because nothing is lost: a deleted section's
                channels are drawn under Text and Voice again, which is where
                they were before anybody made it.
              */
              title="Delete. The channels in it go back to Text and Voice."
              onClick={onDelete}
            >
              <BinGlyph />
            </button>
          </>
        )}
          </>
        )}
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
      {/*
        Under the channels rather than in the heading, because it is a panel
        and not a control: the heading already holds four. Only unfolded, so
        putting a section away puts the whole of it away.
      */}
      {custom && !folded && (
        <SectionPicker
          label={label}
          channels={inSpace}
          held={held}
          onChoose={onChoose}
        />
      )}
    </section>
  );
}
