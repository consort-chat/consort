import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ChatEffect } from "./ChatEffect";
import { PARTICLES, playsFor } from "../lib/chatEffects";

/** Say what `prefers-reduced-motion` answers. */
function reducedMotion(reduce: boolean) {
  window.matchMedia = vi.fn().mockImplementation((media: string) => ({
    matches: reduce && media.includes("prefers-reduced-motion"),
    media,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}

/** Every glyph currently on screen. */
function glyphs(): Element[] {
  return [...document.querySelectorAll(".effect__glyph")];
}

beforeEach(() => {
  vi.useFakeTimers();
  reducedMotion(false);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("a chat effect playing", () => {
  it("throws its glyphs across the room", () => {
    render(<ChatEffect effect="confetti" onDone={vi.fn()} />);

    expect(glyphs()).toHaveLength(PARTICLES);
  });

  it("gives each one its own place and its own timing", () => {
    render(<ChatEffect effect="snowfall" onDone={vi.fn()} />);

    const [first] = glyphs();
    expect(first).toHaveStyle({ animationDuration: expect.anything() });
    expect((first as HTMLElement).style.left).not.toBe("");
    expect((first as HTMLElement).style.animationDelay).not.toBe("");
  });

  it("says which way they travel, which is what the keyframes are chosen by", () => {
    const { rerender } = render(
      <ChatEffect effect="hearts" onDone={vi.fn()} />,
    );
    expect(document.querySelector(".effect")).toHaveAttribute(
      "data-drift",
      "rise",
    );

    rerender(<ChatEffect effect="rainfall" onDone={vi.fn()} />);
    expect(document.querySelector(".effect")).toHaveAttribute(
      "data-drift",
      "fall",
    );
  });

  /*
    The message that asked for this is in the room with its words on it, so
    the animation is for the eye. Fifty party poppers read out one after
    another is the one reading of it that is worse than silence.
  */
  it("is not read out", () => {
    render(<ChatEffect effect="fireworks" onDone={vi.fn()} />);

    expect(document.querySelector(".effect")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    // Nothing an accessible query can reach, which is the point of the
    // attribute above rather than a second way of saying it.
    expect(
      screen.queryAllByText("🎆", { ignore: "[aria-hidden='true'] *" }),
    ).toHaveLength(0);
  });

  it("says when the last glyph has gone, so the next one can start", () => {
    const done = vi.fn();
    render(<ChatEffect effect="confetti" onDone={done} />);

    expect(done).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(playsFor("confetti"));
    });

    expect(done).toHaveBeenCalledTimes(1);
  });

  it("says nothing after it has been taken off screen", () => {
    const done = vi.fn();
    const { unmount } = render(
      <ChatEffect effect="confetti" onDone={done} />,
    );

    unmount();
    act(() => {
      vi.advanceTimersByTime(playsFor("confetti"));
    });

    expect(done).not.toHaveBeenCalled();
  });
});

describe("a desktop that asked for less motion", () => {
  /*
    Nothing drawn, rather than drawn and left still. An overlay that only
    stopped animating would leave fifty glyphs standing across the room, which
    is worse than either playing it or not.
  */
  it("gets no glyphs at all", () => {
    reducedMotion(true);

    render(<ChatEffect effect="confetti" onDone={vi.fn()} />);

    expect(glyphs()).toHaveLength(0);
    expect(document.querySelector(".effect")).not.toBeInTheDocument();
  });

  it("is told immediately that it is over", () => {
    reducedMotion(true);
    const done = vi.fn();

    render(<ChatEffect effect="confetti" onDone={done} />);

    expect(done).toHaveBeenCalledTimes(1);
  });
});
