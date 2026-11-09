import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const usePicture = vi.hoisted(() => vi.fn());
vi.mock("../lib/usePicture", () => ({ usePicture }));

import { SelfPicture } from "./SelfPicture";

const PICTURE = "data:image/jpeg;base64,aaaa";

beforeEach(() => {
  usePicture.mockReset().mockReturnValue(null);
});

describe("a still of what this session is sending", () => {
  it("draws nothing before the first frame arrives", () => {
    // Between a device opening and its first frame. An empty box here would
    // flash every time somebody switched a camera on.
    render(<SelfPicture of="camera" bound={320} />);

    expect(screen.queryByRole("img")).toBeNull();
  });

  it("draws the camera under its own name", () => {
    usePicture.mockReturnValue(PICTURE);

    render(<SelfPicture of="camera" bound={320} />);

    expect(screen.getByRole("img", { name: "Your camera" })).toHaveAttribute(
      "src",
      PICTURE,
    );
  });

  it("draws a shared screen under a different name", () => {
    // Both can be going out at once, so "Your camera" on both would leave
    // somebody unable to tell which square is which.
    usePicture.mockReturnValue(PICTURE);

    render(<SelfPicture of="screen" bound={320} />);

    expect(screen.getByRole("img", { name: "Your screen" })).toBeVisible();
  });

  it("asks for the thing it was told to draw", () => {
    // One component for both, so a camera and a shared screen cannot drift
    // into two ways of drawing the same still.
    render(<SelfPicture of="screen" bound={320} />);

    expect(usePicture).toHaveBeenCalledWith("screen", 320);
  });

  it("asks for the box it is drawn into, not a square's worth", () => {
    // Issue #194. The stage is up to the whole window and a tile is seventy
    // pixels, and a picture made for the tile is what the sender was stuck
    // with everywhere.
    usePicture.mockReturnValue(PICTURE);

    render(<SelfPicture of="screen" bound={1920} />);

    expect(usePicture).toHaveBeenCalledWith("screen", 1920);
    expect(screen.getByRole("img")).toHaveAttribute("width", "1920");
  });
});
