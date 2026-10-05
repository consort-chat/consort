import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const useTheirPicture = vi.hoisted(() => vi.fn());
vi.mock("../lib/usePicture", () => ({ useTheirPicture }));

import { TheirPicture } from "./TheirPicture";

const PICTURE = "data:image/jpeg;base64,aaaa";
const ADA = "@ada:example.org";

beforeEach(() => {
  useTheirPicture.mockReset().mockReturnValue(null);
});

describe("a still of what somebody else is sending", () => {
  it("draws nothing before their first frame arrives", () => {
    // A membership is known before its track is subscribed, so every square
    // starts here. An empty box would flash on every arrival.
    render(<TheirPicture userId={ADA} name="Ada" of="camera" bound={320} />);

    expect(screen.queryByRole("img")).toBeNull();
  });

  it("draws their camera under their own name", () => {
    useTheirPicture.mockReturnValue(PICTURE);

    render(<TheirPicture userId={ADA} name="Ada" of="camera" bound={320} />);

    expect(screen.getByRole("img", { name: "Ada's camera" })).toHaveAttribute(
      "src",
      PICTURE,
    );
  });

  it("names a shared screen as a screen rather than a face", () => {
    // Both can be arriving from one person at once, and they are drawn in
    // different squares. One name for both would leave somebody who cannot see
    // them unable to tell which is which.
    useTheirPicture.mockReturnValue(PICTURE);

    render(<TheirPicture userId={ADA} name="Ada" of="screen" bound={960} />);

    expect(screen.getByRole("img", { name: "Ada's screen" })).toBeVisible();
  });

  it("asks for the person, the stream and the size it was told", () => {
    // The size is the call site's to decide: it is the one thing that knows
    // how big the box is.
    render(<TheirPicture userId={ADA} name="Ada" of="screen" bound={1920} />);

    expect(useTheirPicture).toHaveBeenCalledWith(ADA, "screen", 1920);
  });
});
