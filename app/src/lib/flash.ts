/**
 * Going to a message and lighting it up.
 *
 * Shared because two things arrive at the same place: pressing the row above a
 * reply, and pressing a link to a message somebody pasted. Both want the same
 * scroll and the same flash, and two copies would be two animations that drift
 * apart.
 */

/**
 * How long a message stays lit after being jumped to, in milliseconds.
 *
 * Long enough to find with the eye after the scroll settles, short enough that
 * it is not still glowing when somebody starts reading the next thing.
 */
export const FLASH = 1_400;

/**
 * Scroll to a message inside `box` and light it up.
 *
 * The attribute goes on the element rather than through React state, and
 * deliberately. The row being jumped to may belong to a different component
 * than the one that was pressed, which state there could not reach; and React
 * leaves an attribute it never set alone, so a re-render does not fight it.
 *
 * False when the message is not drawn, which is a reply or a link naming
 * something older than what is loaded. The caller decides what to say about
 * that; there is nothing useful to do here.
 */
export function flashMessage(box: HTMLElement | null, eventId: string): boolean {
  const target = box?.querySelector(`[data-message-id="${CSS.escape(eventId)}"]`);
  if (!(target instanceof HTMLElement)) return false;

  target.scrollIntoView({ block: "center", behavior: "smooth" });
  target.setAttribute("data-flash", "true");
  window.setTimeout(() => target.removeAttribute("data-flash"), FLASH);
  return true;
}
