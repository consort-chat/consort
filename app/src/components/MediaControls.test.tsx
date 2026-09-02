import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { MediaControls, clock } from "./MediaControls";

/**
 * A video element the bar can drive.
 *
 * jsdom implements none of the media element's behaviour: `play` and `pause`
 * throw, `duration` is `NaN` and nothing changes `paused`. So the parts the
 * bar reads and writes are defined here, and the events it listens for are
 * fired by hand, which is what the browser would do.
 */
function element(): HTMLVideoElement {
  const video = document.createElement("video");
  let paused = true;

  Object.defineProperty(video, "paused", {
    get: () => paused,
    configurable: true,
  });
  Object.defineProperty(video, "duration", { value: 90, configurable: true });
  video.play = vi.fn().mockImplementation(() => {
    paused = false;
    video.dispatchEvent(new Event("play"));
    return Promise.resolve();
  });
  video.pause = vi.fn().mockImplementation(() => {
    paused = true;
    video.dispatchEvent(new Event("pause"));
  });

  return video;
}

let video: HTMLVideoElement;

beforeEach(() => {
  video = element();
});

describe("clock", () => {
  it("writes a running time the way a player does", () => {
    expect(clock(0)).toBe("0:00");
    expect(clock(9)).toBe("0:09");
    expect(clock(90)).toBe("1:30");
    expect(clock(3661)).toBe("1:01:01");
  });

  it("drops the hour for anything under one", () => {
    // A chat room's clips are seconds long. An hour column on every one of
    // them is a column of zeroes.
    expect(clock(59)).toBe("0:59");
  });

  it("says zero for a length nobody knows yet", () => {
    // `duration` is NaN until enough of the file has arrived, and drawing
    // "NaN:NaN" for the second it takes is worse than drawing nothing.
    expect(clock(Number.NaN)).toBe("0:00");
    expect(clock(Number.POSITIVE_INFINITY)).toBe("0:00");
    expect(clock(-1)).toBe("0:00");
  });
});

describe("MediaControls", () => {
  it("plays and pauses the element it was given", async () => {
    render(<MediaControls media={video} label="clip.mp4" />);

    await userEvent.click(screen.getByRole("button", { name: "Play clip.mp4" }));
    expect(video.play).toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: "Pause clip.mp4" }));
    expect(video.pause).toHaveBeenCalled();
  });

  it("follows the element rather than remembering what it was told", () => {
    // A clip is driven by things other than these buttons: the keyboard, the
    // end of the file, a page that lost focus. State kept here would drift out
    // of step with the picture beside it.
    render(<MediaControls media={video} label="clip.mp4" />);

    // Started from somewhere else entirely: the keyboard, or the `autoplay`
    // attribute the player carries.
    Object.defineProperty(video, "paused", { value: false, configurable: true });
    fireEvent(video, new Event("play"));

    expect(screen.getByRole("button", { name: "Pause clip.mp4" })).toBeVisible();
  });

  it("draws where it is and how long it is", () => {
    render(<MediaControls media={video} label="clip.mp4" />);

    Object.defineProperty(video, "currentTime", { value: 30, configurable: true });
    fireEvent(video, new Event("timeupdate"));

    expect(screen.getByText("0:30")).toBeVisible();
    expect(screen.getByText("1:30")).toBeVisible();
  });

  it("seeks to wherever the scrub bar is dragged", () => {
    render(<MediaControls media={video} label="clip.mp4" />);

    fireEvent.change(screen.getByRole("slider"), { target: { value: "45" } });

    expect(video.currentTime).toBe(45);
  });

  it("leaves the scrub bar alone until a length is known", () => {
    // `duration` is NaN while the first bytes are on their way, and a slider
    // that can be dragged to nowhere is a control that does nothing.
    const unknown = document.createElement("video");
    render(<MediaControls media={unknown} label="clip.mp4" />);

    expect(screen.getByRole("slider")).toBeDisabled();
  });

  it("mutes and unmutes", async () => {
    render(<MediaControls media={video} label="clip.mp4" />);

    await userEvent.click(screen.getByRole("button", { name: "Mute clip.mp4" }));

    expect(video.muted).toBe(true);
    expect(screen.getByRole("button", { name: "Unmute clip.mp4" })).toBeVisible();
  });

  it("asks for full screen", async () => {
    video.requestFullscreen = vi.fn().mockResolvedValue(undefined);
    render(<MediaControls media={video} label="clip.mp4" />);

    await userEvent.click(
      screen.getByRole("button", { name: "Show clip.mp4 full screen" }),
    );

    expect(video.requestFullscreen).toHaveBeenCalled();
  });

  it("draws without complaint before the player exists", () => {
    // The bar is rendered in the same pass as the element it drives, so it
    // sees null once, every time.
    render(<MediaControls media={null} label="clip.mp4" />);

    expect(screen.getByRole("button", { name: "Play clip.mp4" })).toBeVisible();
  });

  it("presses nothing when there is nothing to press", async () => {
    render(<MediaControls media={null} label="clip.mp4" />);

    await userEvent.click(screen.getByRole("button", { name: "Play clip.mp4" }));

    expect(screen.getByRole("button", { name: "Play clip.mp4" })).toBeVisible();
  });

  it("names what it is driving, so a room of clips has distinguishable controls", () => {
    render(<MediaControls media={video} label="holiday.mp4" />);

    expect(screen.getByRole("slider", { name: "Position in holiday.mp4" })).toBeVisible();
  });
});
