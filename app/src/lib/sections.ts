import type { Channel, CustomSection } from "./api";

/**
 * The order the sidebar's sections are drawn in.
 *
 * Keys rather than the sections themselves, so that what is stored stays
 * independent of what a build happens to draw. See `SidebarSettings`.
 */

/**
 * The sections to draw, top first, given the order somebody dragged them into.
 *
 * A stable sort, which is what puts a section the stored order has never heard
 * of after the ones it has rather than at a place it was never given.
 */
export function arrange<T extends { key: string }>(
  sections: readonly T[],
  order: readonly string[],
): T[] {
  const place = (section: T) => {
    const at = order.indexOf(section.key);
    return at < 0 ? order.length : at;
  };
  return [...sections].sort((one, other) => place(one) - place(other));
}

/**
 * `keys` with `from` taken out and put back where `to` was.
 *
 * Unchanged when either key is missing. A drag can arrive from outside the
 * application carrying any text at all, and this is where that stops.
 */
export function moved(
  keys: readonly string[],
  from: string,
  to: string,
): string[] {
  const target = keys.indexOf(to);
  if (!keys.includes(from) || target < 0) return [...keys];

  const rest = keys.filter((key) => key !== from);
  return [...rest.slice(0, target), from, ...rest.slice(target)];
}

/**
 * The sections the sidebar draws before anybody makes one.
 *
 * Derived from `m.room.type` rather than from a choice, which is why these two
 * cannot be renamed or deleted: Matrix decides what is in them. They are also
 * where a room with no section of its own is drawn, so between them they hold
 * whatever is left over.
 */
export const BUILT_IN: readonly { key: string; label: string; kind: Channel["kind"] }[] =
  [
    { key: "text", label: "Text", kind: "text" },
    { key: "voice", label: "Voice", kind: "voice" },
  ];

/** One section, with the channels in it, ready to draw. */
export interface Drawn {
  key: string;
  label: string;
  channels: Channel[];
  /** Whether somebody made this one, so it can be renamed and deleted. */
  custom: boolean;
}

/**
 * The sections to draw for one space, in the order somebody arranged them.
 *
 * `customs` is the sections made under this space, which the caller filters:
 * a section made in one space has no business being drawn in another.
 *
 * A room is in one section at a time, so a room a custom section claims is
 * taken out of Text or Voice. An empty built-in section is dropped and an
 * empty custom one is not: the first reads as a list that failed to load, and
 * the second is the only place anything can be put into it.
 */
export function sectionsOf(
  channels: readonly Channel[],
  customs: readonly CustomSection[],
  order: readonly string[],
): Drawn[] {
  const claimed = new Set(customs.flatMap((section) => section.rooms));

  const built = BUILT_IN.map((section) => ({
    key: section.key,
    label: section.label,
    custom: false,
    channels: channels.filter(
      (channel) => channel.kind === section.kind && !claimed.has(channel.id),
    ),
  })).filter((section) => section.channels.length > 0);

  // Filtered rather than read off `rooms`, so a section's channels sit in the
  // order Rust sorted them into and agree with every other section about it.
  const mine = customs.map((section) => ({
    key: section.key,
    label: section.name,
    custom: true,
    channels: channels.filter((channel) => section.rooms.includes(channel.id)),
  }));

  return arrange([...built, ...mine], order);
}

/**
 * The longest a section's name may be, in characters.
 *
 * Mirrors `crate::settings::MAX_SECTION_NAME`, which refuses a longer one. The
 * box stops at it rather than letting somebody type a name that is then
 * rejected.
 */
export const MAX_SECTION_NAME = 40;
