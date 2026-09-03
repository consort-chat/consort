/**
 * A scrolling box that reports numbers, in a DOM that does no layout.
 *
 * jsdom lays nothing out, so `scrollHeight` and `clientHeight` are zero for
 * every element and `scrollTop` never keeps what is written to it. Both the
 * room and the thread panel decide whether to follow a conversation from
 * exactly those three, so without this there is nothing about following to
 * assert.
 *
 * Patched on the prototype rather than on the element, because the box does
 * not exist until the first render and the layout effect that reads it runs
 * inside that same commit. There is no moment in between to reach it.
 *
 * Importing this file registers the cleanup that puts the prototype back.
 */
import { afterEach } from "vitest";

const tops = new WeakMap<Element, number>();

/** The descriptors as they were, so a file using this does not leak into the next. */
const original = new Map<string, PropertyDescriptor | undefined>();

function fake(name: string, descriptor: PropertyDescriptor) {
  if (!original.has(name)) {
    original.set(name, Object.getOwnPropertyDescriptor(Element.prototype, name));
  }
  Object.defineProperty(Element.prototype, name, {
    configurable: true,
    ...descriptor,
  });
}

/** How tall the content is, and how much of it is on screen. */
export interface FakeLayout {
  scrollHeight: number;
  clientHeight: number;
}

/**
 * Say how tall the content is and how much of it is on screen.
 *
 * Call it before the box is drawn. The returned record is writable, because a
 * page of history landing on the front is a box that got taller, and holding
 * a reader's place through that is the thing worth testing.
 *
 * `scrollTop` keeps what is written to it, clamped the way a browser clamps
 * it. The clamp is not decoration: `scrollTop = scrollHeight` is how both
 * panes follow a conversation, and without it the bottom of a box would be a
 * position no real one can be in.
 */
export function fakeScrolling(
  scrollHeight: number,
  clientHeight: number,
): FakeLayout {
  const layout: FakeLayout = { scrollHeight, clientHeight };

  fake("scrollHeight", { get: () => layout.scrollHeight });
  fake("clientHeight", { get: () => layout.clientHeight });
  fake("scrollTop", {
    get(this: Element) {
      return tops.get(this) ?? 0;
    },
    set(this: Element, top: number) {
      const furthest = Math.max(0, layout.scrollHeight - layout.clientHeight);
      tops.set(this, Math.max(0, Math.min(top, furthest)));
    },
  });

  return layout;
}

afterEach(() => {
  for (const [name, descriptor] of original) {
    if (descriptor === undefined) {
      delete (Element.prototype as unknown as Record<string, unknown>)[name];
    } else {
      Object.defineProperty(Element.prototype, name, descriptor);
    }
  }
  original.clear();
});
