import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { LevelMeter } from "./LevelMeter";

function fillOf(container: HTMLElement): HTMLElement {
  const fill = container.querySelector<HTMLElement>(".level-meter__fill");
  if (fill === null) throw new Error("the meter drew no fill");
  return fill;
}

describe("LevelMeter", () => {
  it("draws the fill in proportion to the level", () => {
    const { container } = render(
      <LevelMeter level={0.25} open={false} running />,
    );

    expect(fillOf(container).style.width).toBe("25%");
  });

  it("cannot be drawn past the end of its own track", () => {
    // The Rust side divides by 32768 rather than by i16::MAX precisely so this
    // cannot happen, but the bar is drawn from a number that crossed a process
    // boundary and nothing here can prove what is on the other side of it.
    const { container } = render(<LevelMeter level={4} open running />);

    expect(fillOf(container).style.width).toBe("100%");
  });

  it("cannot be drawn backwards", () => {
    const { container } = render(<LevelMeter level={-1} open={false} running />);

    expect(fillOf(container).style.width).toBe("0%");
  });

  it("says when it can hear you", () => {
    render(<LevelMeter level={0.6} open running />);

    expect(screen.getByText(/hear you/i)).toBeVisible();
  });

  it("says when it is listening and hearing nothing", () => {
    // The distinction the whole panel exists for. A bar at zero means one of
    // two very different things, and only one of them is a broken microphone.
    render(<LevelMeter level={0.01} open={false} running />);

    expect(screen.getByText(/listening/i)).toBeVisible();
  });

  it("says when the microphone is not open at all", () => {
    render(<LevelMeter level={0} open={false} running={false} />);

    expect(screen.getByText(/not running/i)).toBeVisible();
  });

  it("marks the fill when the gate is open, so colour is not the only signal", () => {
    const { container } = render(<LevelMeter level={0.6} open running />);

    expect(fillOf(container)).toHaveAttribute("data-open", "true");
  });
});
