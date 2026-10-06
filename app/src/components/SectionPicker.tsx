/**
 * Which of a space's channels are in one section somebody made.
 *
 * The route that needs no pointing device, and the only route into a section
 * with nothing in it yet. A checkbox per channel rather than a list of the
 * ones left over, because unchecking is how a channel leaves again.
 */
import { useId, useState } from "react";

import type { Channel } from "../lib/api";
import { channelLabel } from "../lib/labels";

export function SectionPicker({
  label,
  channels,
  held,
  onChoose,
}: {
  /** The section's name, for the controls that have to say which section. */
  label: string;
  /** Every channel in the space, whichever section it is in now. */
  channels: Channel[];
  /** The IDs this section holds, which is what the boxes are ticked from. */
  held: ReadonlySet<string>;
  /** Put a channel in this section, or take it back out. */
  onChoose: (roomId: string, inside: boolean) => void;
}) {
  const [open, setOpen] = useState(false);
  const listId = useId();

  if (channels.length === 0) return null;

  return (
    <div className="channels__picker">
      <button
        type="button"
        className="channels__choose"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen(!open)}
      >
        {open ? "Done" : `Choose channels for ${label}`}
      </button>
      {/*
        Hidden rather than unmounted, so the control's `aria-controls` points
        at something that exists in both states. The same way a section folds.
      */}
      <ul className="channels__choices" id={listId} hidden={!open}>
        {channels.map((channel) => (
          <li key={channel.id}>
            <label className="channels__choice">
              <input
                type="checkbox"
                checked={held.has(channel.id)}
                onChange={(event) => onChoose(channel.id, event.target.checked)}
              />
              {channelLabel(channel)}
            </label>
          </li>
        ))}
      </ul>
    </div>
  );
}
