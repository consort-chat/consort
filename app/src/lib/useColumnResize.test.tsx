/*
  The edge of a resizable column, as a gesture rather than as a panel.

  Both sidebars use this, and they face opposite ways: the thread panel is on
  the right of the pane and widens as the pointer goes left, the channel list
  is on the left and widens as it goes right. The sign is the only difference
  between them, so it is the thing most worth a test of its own.
*/
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { STEP, clampTo, useColumnResize } from "./useColumnResize";

const BOUNDS = { min: 100, max: 600 };

function Edge({
  width,
  onResize,
  widens,
}: {
  width: number;
  onResize: (width: number) => void;
  widens: "left" | "right";
}) {
  const grip = useColumnResize({ width, onResize, widens, bounds: () => BOUNDS });
  return <div role="separator" aria-label="Resize" tabIndex={0} {...grip} />;
}

function drawn(widens: "left" | "right", width = 300) {
  const onResize = vi.fn();
  render(<Edge width={width} onResize={onResize} widens={widens} />);
  return { onResize, grip: screen.getByRole("separator", { name: "Resize" }) };
}

describe("clamping a width to its bounds", () => {
  it("keeps a width that is already inside them", () => {
    expect(clampTo(BOUNDS, 300)).toBe(300);
  });

  it("refuses to go below the minimum", () => {
    expect(clampTo(BOUNDS, 10)).toBe(100);
  });

  it("refuses to go above the maximum", () => {
    expect(clampTo(BOUNDS, 10_000)).toBe(600);
  });

  it("leaves a usable column when the window is too small for both", () => {
    // The minimum last, so a squeezed window gives a readable column rather
    // than a sliver. A maximum below the minimum is the shape that tests it.
    expect(clampTo({ min: 100, max: 40 }, 300)).toBe(100);
  });
});

describe("dragging the edge of a column on the right", () => {
  it("widens as the pointer moves towards the pane", () => {
    const { onResize, grip } = drawn("left");

    fireEvent.pointerDown(grip, { clientX: 900 });
    fireEvent.pointerMove(window, { clientX: 800 });

    expect(onResize).toHaveBeenLastCalledWith(400);
  });

  it("narrows as the pointer moves away from it", () => {
    const { onResize, grip } = drawn("left");

    fireEvent.pointerDown(grip, { clientX: 900 });
    fireEvent.pointerMove(window, { clientX: 1_000 });

    expect(onResize).toHaveBeenLastCalledWith(200);
  });
});

describe("dragging the edge of a column on the left", () => {
  it("widens as the pointer moves towards the pane", () => {
    const { onResize, grip } = drawn("right");

    fireEvent.pointerDown(grip, { clientX: 300 });
    fireEvent.pointerMove(window, { clientX: 400 });

    expect(onResize).toHaveBeenLastCalledWith(400);
  });

  it("narrows as the pointer moves away from it", () => {
    const { onResize, grip } = drawn("right");

    fireEvent.pointerDown(grip, { clientX: 300 });
    fireEvent.pointerMove(window, { clientX: 200 });

    expect(onResize).toHaveBeenLastCalledWith(200);
  });
});

describe("a drag that is over", () => {
  it("stops listening once the pointer is let go", () => {
    const { onResize, grip } = drawn("left");

    fireEvent.pointerDown(grip, { clientX: 900 });
    fireEvent.pointerUp(window, { clientX: 900 });
    onResize.mockClear();
    fireEvent.pointerMove(window, { clientX: 700 });

    expect(onResize).not.toHaveBeenCalled();
  });

  it("stops listening when the system takes the pointer back", () => {
    // The one ending that never sends a `pointerup`. Without it the edge goes
    // on following a finger that has gone.
    const { onResize, grip } = drawn("left");

    fireEvent.pointerDown(grip, { clientX: 900 });
    fireEvent.pointerCancel(window, { clientX: 900 });
    onResize.mockClear();
    fireEvent.pointerMove(window, { clientX: 700 });

    expect(onResize).not.toHaveBeenCalled();
  });

  it("stops listening when the column it belonged to goes", () => {
    // A thread can be shut with the pointer still down on its grip.
    const onResize = vi.fn();
    const { unmount } = render(
      <Edge width={300} onResize={onResize} widens="left" />,
    );

    fireEvent.pointerDown(
      screen.getByRole("separator", { name: "Resize" }),
      { clientX: 900 },
    );
    unmount();
    onResize.mockClear();
    fireEvent.pointerMove(window, { clientX: 700 });

    expect(onResize).not.toHaveBeenCalled();
  });
});

describe("the arrow keys, so a mouse is not the only way", () => {
  it("widens a column on the right with the left arrow", async () => {
    const { onResize, grip } = drawn("left");
    grip.focus();

    await userEvent.keyboard("{ArrowLeft}");
    expect(onResize).toHaveBeenLastCalledWith(300 + STEP);

    await userEvent.keyboard("{ArrowRight}");
    expect(onResize).toHaveBeenLastCalledWith(300 - STEP);
  });

  it("widens a column on the left with the right arrow", async () => {
    const { onResize, grip } = drawn("right");
    grip.focus();

    await userEvent.keyboard("{ArrowRight}");
    expect(onResize).toHaveBeenLastCalledWith(300 + STEP);

    await userEvent.keyboard("{ArrowLeft}");
    expect(onResize).toHaveBeenLastCalledWith(300 - STEP);
  });

  it("says nothing about a key that is not an arrow", async () => {
    const { onResize, grip } = drawn("left");
    grip.focus();

    await userEvent.keyboard("{Enter}");

    expect(onResize).not.toHaveBeenCalled();
  });

  it("holds a nudge inside the bounds", async () => {
    const { onResize, grip } = drawn("left", BOUNDS.min);
    grip.focus();

    await userEvent.keyboard("{ArrowRight}");

    expect(onResize).toHaveBeenLastCalledWith(BOUNDS.min);
  });
});

describe("what the edge says about itself", () => {
  it("reports the width and the bounds it may be dragged between", () => {
    const { grip } = drawn("left", 320);

    expect(grip).toHaveAttribute("aria-valuenow", "320");
    expect(grip).toHaveAttribute("aria-valuemin", String(BOUNDS.min));
    expect(grip).toHaveAttribute("aria-valuemax", String(BOUNDS.max));
    expect(grip).toHaveAttribute("aria-orientation", "vertical");
  });
});
