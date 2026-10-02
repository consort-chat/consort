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
    render(<SelfPicture of="camera" />);

    expect(screen.queryByRole("img")).toBeNull();
  });

  it("draws the camera under its own name", () => {
    usePicture.mockReturnValue(PICTURE);

    render(<SelfPicture of="camera" />);

    expect(screen.getByRole("img", { name: "Your camera" })).toHaveAttribute(
      "src",
      PICTURE,
    );
  });

  it("draws a shared screen under a different name", () => {
    // Both can be going out at once, so "Your camera" on both would leave
    // somebody unable to tell which square is which.
    usePicture.mockReturnValue(PICTURE);

    render(<SelfPicture of="screen" />);

    expect(screen.getByRole("img", { name: "Your screen" })).toBeVisible();
  });

  it("asks for the thing it was told to draw", () => {
    // One component for both, so a camera and a shared screen cannot drift
    // into two ways of drawing the same still.
    render(<SelfPicture of="screen" />);

    expect(usePicture).toHaveBeenCalledWith("screen");
  });
});
