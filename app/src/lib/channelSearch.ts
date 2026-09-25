/**
 * Finding a channel in a space by typing part of its name.
 *
 * Its own file rather than a closure in the pane, because it is the whole of
 * what the search does and a filter that quietly ignores its input passes any
 * test written against the pane's happy path.
 */
import type { Channel } from "./api";
import { channelLabel } from "./labels";

/**
 * The channels whose name holds `query`, in the order they were given.
 *
 * Substring rather than prefix, and case-insensitively: channel names are
 * compounds far more often than single words, so `crit` is how somebody looks
 * for `design-crit`.
 *
 * Not re-ordered by how well each one matched. The order comes from Rust,
 * which follows MSC1772, and a list that ranked itself would move under the
 * pointer as somebody typed.
 *
 * Everything back for a query that is empty or only spaces. A field somebody
 * tabbed through is a field with no question in it, and answering one with
 * nothing reads as a space that has lost its channels.
 *
 * Matched against what the row draws rather than against `name`, so a channel
 * nobody has joined yet is searchable as the "Unknown channel" it is drawn as
 * rather than as a null nobody can see.
 */
export function matchingChannels(
  channels: Channel[],
  query: string,
): Channel[] {
  const wanted = query.trim().toLowerCase();
  if (wanted === "") return channels;

  return channels.filter((channel) =>
    channelLabel(channel).toLowerCase().includes(wanted),
  );
}
