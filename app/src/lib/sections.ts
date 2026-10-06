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
