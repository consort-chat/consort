import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { useDraggable } from "./useDraggable";

/**
 * A card with a handle, which is the only shape this hook is used in.
 *
 * jsdom has no layout, so the box it would measure is stubbed rather than
 * left at the zeros `getBoundingClientRect` answers with. A card whose width
 * is zero is never clamped by anything, so every test here would pass for the
 * wrong reason without this.
 */
function Card() {
  const drag = useDraggable<HTMLDivElement>();
  // What the last press turned out to be, the way the card itself asks: in a
  // click handler, after the gesture is over.
  const [dragged, setDragged] = useState<boolean | null>(null);

  return (
    <div
      ref={drag.ref}
      data-testid="card"
      {...(dragged === null ? {} : { "data-dragged": String(dragged) })}
      onClick={() => setDragged(drag.dragged())}
      style={
        drag.at === null
          ? { position: "fixed", left: 100, top: 100 }
          : { position: "fixed", left: drag.at.left, top: drag.at.top }
      }
    >
      <div data-testid="handle" tabIndex={0} {...drag.handle} />
    </div>
  );
}

/** What the card would measure, if jsdom measured anything. */
function stubLayout(box: { left: number; top: number; width: number; height: number }) {
  const card = screen.getByTestId("card");
  card.getBoundingClientRect = () =>
    ({
      left: box.left,
      top: box.top,
      width: box.width,
      height: box.height,
      right: box.left + box.width,
      bottom: box.top + box.height,
      x: box.left,
      y: box.top,
      toJSON: () => ({}),
    }) as DOMRect;
}

function resizeTo(width: number, height: number) {
  Object.defineProperty(window, "innerWidth", { value: width, configurable: true });
  Object.defineProperty(window, "innerHeight", { value: height, configurable: true });
  fireEvent(window, new Event("resize"));
}

afterEach(() => {
  Object.defineProperty(window, "innerWidth", { value: 1024, configurable: true });
  Object.defineProperty(window, "innerHeight", { value: 768, configurable: true });
});

describe("useDraggable", () => {
  it("leaves the element where it is until something moves it", () => {
    render(<Card />);

    expect(screen.getByTestId("card")).toHaveStyle({ left: "100px" });
  });

  it("focuses the handle it was taken hold of by", () => {
    // Preventing the default on a pointer press is what stops the drag
    // smearing a selection across the card, and it is also what stops the
    // press focusing anything. The arrow keys are no use to somebody who has
    // to tab back to a handle they have just clicked.
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });

    expect(screen.getByTestId("handle")).toHaveFocus();
  });

  it("follows the pointer", () => {
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: 190, clientY: 160 });
    fireEvent.pointerUp(window);

    expect(screen.getByTestId("card")).toHaveStyle({
      left: "140px",
      top: "150px",
    });
  });

  it("stops following once the pointer is let go", () => {
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: 160, clientY: 120 });
    fireEvent.pointerUp(window);
    fireEvent.pointerMove(window, { clientX: 600, clientY: 600 });

    expect(screen.getByTestId("card")).toHaveStyle({ left: "110px" });
  });

  it("keeps the element inside the window", () => {
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: 5000, clientY: 5000 });

    // The far edge, less the card and the gap every floating thing here keeps.
    expect(screen.getByTestId("card")).toHaveStyle({
      left: `${1024 - 200 - 8}px`,
      top: `${768 - 120 - 8}px`,
    });
  });

  it("keeps the element off the near edges too", () => {
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: -5000, clientY: -5000 });

    expect(screen.getByTestId("card")).toHaveStyle({ left: "8px", top: "8px" });
  });

  it("brings a dragged element back when the window shrinks under it", () => {
    // The bug this exists for: a card parked against the right edge of a wide
    // window is off screen entirely once the window is half the size.
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: 900, clientY: 110 });
    fireEvent.pointerUp(window);
    resizeTo(500, 400);

    expect(screen.getByTestId("card")).toHaveStyle({
      left: `${500 - 200 - 8}px`,
    });
  });

  it("leaves an element nobody has dragged where the stylesheet put it", () => {
    // Nothing to correct. Its place is whatever the stylesheet says, which
    // follows the window on its own.
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    resizeTo(500, 400);

    expect(screen.getByTestId("card")).toHaveStyle({ left: "100px" });
  });

  it("moves with the arrow keys", () => {
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.keyDown(screen.getByTestId("handle"), { key: "ArrowRight" });
    fireEvent.keyDown(screen.getByTestId("handle"), { key: "ArrowDown" });

    expect(screen.getByTestId("card")).toHaveStyle({
      left: "116px",
      top: "116px",
    });
  });

  it("moves the other way too", () => {
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.keyDown(screen.getByTestId("handle"), { key: "ArrowLeft" });
    fireEvent.keyDown(screen.getByTestId("handle"), { key: "ArrowUp" });

    expect(screen.getByTestId("card")).toHaveStyle({
      left: "84px",
      top: "84px",
    });
  });

  it("stops at the edge for a keyboard as well", () => {
    render(<Card />);
    stubLayout({ left: 10, top: 10, width: 200, height: 120 });

    fireEvent.keyDown(screen.getByTestId("handle"), { key: "ArrowLeft" });

    expect(screen.getByTestId("card")).toHaveStyle({ left: "8px" });
  });

  it("ignores every other key", () => {
    // Typing is not moving. A handle that swallowed Tab would be a keyboard
    // trap, which is worse than one that cannot be moved at all.
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.keyDown(screen.getByTestId("handle"), { key: "Tab" });
    fireEvent.keyDown(screen.getByTestId("handle"), { key: "a" });

    expect(screen.getByTestId("card")).toHaveStyle({ left: "100px" });
  });

  it("reports a gesture that travelled", () => {
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: 250, clientY: 110 });
    fireEvent.pointerUp(window);
    fireEvent.click(screen.getByTestId("card"));

    expect(screen.getByTestId("card")).toHaveAttribute("data-dragged", "true");
  });

  it("reports a gesture that went out and came back as a drag", () => {
    // Not a click. Somebody who moved the card and put it back has dragged it,
    // and treating that as a click would expand the card under their hand.
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: 250, clientY: 110 });
    fireEvent.pointerMove(window, { clientX: 150, clientY: 110 });
    fireEvent.pointerUp(window);
    fireEvent.click(screen.getByTestId("card"));

    expect(screen.getByTestId("card")).toHaveAttribute("data-dragged", "true");
  });

  it("forgets a drag as soon as the next press lands anywhere", () => {
    // The verdict is about the press that just ended, not about the last one
    // that happened to be a drag. Without this a card dragged once refuses
    // every double-click it is given afterwards.
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: 250, clientY: 110 });
    fireEvent.pointerUp(window);
    fireEvent.pointerDown(screen.getByTestId("card"));
    fireEvent.click(screen.getByTestId("card"));

    expect(screen.getByTestId("card")).toHaveAttribute("data-dragged", "false");
  });

  it("lets go when the system takes the pointer back", () => {
    // A cancelled touch is the one ending that never sends a `pointerup`.
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: 160, clientY: 120 });
    fireEvent.pointerCancel(window);
    fireEvent.pointerMove(window, { clientX: 600, clientY: 600 });

    expect(screen.getByTestId("card")).toHaveStyle({ left: "110px" });
  });

  it("does not report a press that stayed still", () => {
    render(<Card />);
    stubLayout({ left: 100, top: 100, width: 200, height: 120 });

    fireEvent.pointerDown(screen.getByTestId("handle"), {
      clientX: 150,
      clientY: 110,
    });
    fireEvent.pointerMove(window, { clientX: 151, clientY: 110 });
    fireEvent.pointerUp(window);
    fireEvent.click(screen.getByTestId("card"));

    expect(screen.getByTestId("card")).toHaveAttribute("data-dragged", "false");
  });
});
