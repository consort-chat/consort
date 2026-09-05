import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { FLASH, flashMessage } from "./flash";

/**
 * A box holding one message per ID given.
 *
 * Attached to the document, because the thing under test looks a message up
 * inside a box somebody scrolls rather than in the document at large.
 */
function box(...ids: string[]): HTMLElement {
  const element = document.createElement("div");
  for (const id of ids) {
    const message = document.createElement("div");
    message.setAttribute("data-message-id", id);
    element.append(message);
  }
  document.body.append(element);
  return element;
}

beforeEach(() => {
  vi.useFakeTimers();
  // jsdom lays nothing out and so implements none of this.
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(() => {
  vi.useRealTimers();
  document.body.replaceChildren();
});

describe("going to a message", () => {
  it("lights up the row carrying that ID", () => {
    const room = box("$1", "$2");

    expect(flashMessage(room, "$2")).toBe(true);

    expect(room.children[1]).toHaveAttribute("data-flash", "true");
    expect(room.children[0]).not.toHaveAttribute("data-flash");
  });

  it("scrolls it to the middle rather than to an edge", () => {
    // Landing against the top or the bottom of the box puts the message being
    // jumped to at the edge of what can be read, with the answer it belongs
    // to off screen.
    const room = box("$1");

    flashMessage(room, "$1");

    expect(Element.prototype.scrollIntoView).toHaveBeenCalledWith({
      block: "center",
      behavior: "smooth",
    });
  });

  it("stops lighting it up again afterwards", () => {
    // The attribute is what the animation hangs off, so leaving it on would
    // be a message that never stops being the one somebody jumped to.
    const room = box("$1");
    flashMessage(room, "$1");

    vi.advanceTimersByTime(FLASH);

    expect(room.firstElementChild).not.toHaveAttribute("data-flash");
  });

  it("says so when the message is not drawn", () => {
    // A reply or a link can name something older than the history loaded.
    // The caller decides what to say about that.
    expect(flashMessage(box("$1"), "$missing")).toBe(false);
  });

  it("says so when there is no box to look in", () => {
    expect(flashMessage(null, "$1")).toBe(false);
  });

  it("looks only inside the box it was given", () => {
    // The thread panel draws the same component beside the room, and a root
    // message is in both. An unscoped search would light up whichever copy
    // the document happened to hold first.
    const room = box("$1");
    const panel = box("$2");

    expect(flashMessage(panel, "$1")).toBe(false);
    expect(room.firstElementChild).not.toHaveAttribute("data-flash");
  });

  it("finds an ID that would otherwise break the selector", () => {
    // Matrix event IDs start with a dollar, which is a combinator in CSS.
    const room = box("$a.b:example.org");

    expect(flashMessage(room, "$a.b:example.org")).toBe(true);
  });
});
