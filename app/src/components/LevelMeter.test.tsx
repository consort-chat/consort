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
      <LevelMeter level={0.25} open={false} running voiceActivity />,
    );

    expect(fillOf(container).style.width).toBe("25%");
  });

  it("cannot be drawn past the end of its own track", () => {
    // The Rust side divides by 32768 rather than by i16::MAX precisely so this
    // cannot happen, but the bar is drawn from a number that crossed a process
    // boundary and nothing here can prove what is on the other side of it.
    const { container } = render(<LevelMeter level={4} open running voiceActivity />);

    expect(fillOf(container).style.width).toBe("100%");
  });

  it("cannot be drawn backwards", () => {
    const { container } = render(<LevelMeter level={-1} open={false} running voiceActivity />);

    expect(fillOf(container).style.width).toBe("0%");
  });

  it("says when it can hear you", () => {
    render(<LevelMeter level={0.6} open running voiceActivity />);

    expect(screen.getByText(/hear you/i)).toBeVisible();
  });

  it("says when it is listening and hearing nothing", () => {
    // The distinction the whole panel exists for. A bar at zero means one of
    // two very different things, and only one of them is a broken microphone.
    render(<LevelMeter level={0.01} open={false} running voiceActivity />);

    expect(screen.getByText(/listening/i)).toBeVisible();
  });

  it("says when the microphone is not open at all", () => {
    render(<LevelMeter level={0} open={false} running={false} voiceActivity />);

    expect(screen.getByText(/not running/i)).toBeVisible();
  });

  it("says that everything is going out when voice activity is off", () => {
    // "We can hear you" would be true on every frame with the gate off, and
    // would therefore say nothing. What is worth saying is that nothing is
    // being held back, because that is the thing somebody chose and the thing
    // they might have forgotten choosing.
    render(<LevelMeter level={0.6} open running voiceActivity={false} />);

    expect(screen.getByText(/everything the microphone hears/i)).toBeVisible();
  });

  it("still says nothing is running when the gate is off and the device is shut", () => {
    // The gate being off does not make a dead microphone into a live one, and
    // that is the one confusion this caption exists to prevent.
    render(
      <LevelMeter level={0} open={false} running={false} voiceActivity={false} />,
    );

    expect(screen.getByText(/not running/i)).toBeVisible();
  });

  it("marks the fill when the gate is open, so colour is not the only signal", () => {
    const { container } = render(<LevelMeter level={0.6} open running voiceActivity />);

    expect(fillOf(container)).toHaveAttribute("data-open", "true");
  });
});
