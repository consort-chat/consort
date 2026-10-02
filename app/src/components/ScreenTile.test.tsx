import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const usePicture = vi.hoisted(() => vi.fn());
vi.mock("../lib/usePicture", () => ({ usePicture }));

import { ScreenTile } from "./ScreenTile";

const PICTURE = "data:image/jpeg;base64,aaaa";

/** One tile in a list, because that is where the card puts it. */
function draw(overrides: Partial<Parameters<typeof ScreenTile>[0]> = {}) {
  const onToggle = vi.fn();
  render(
    <ul aria-label="Screens shared in Lounge">
      <ScreenTile
        label="DP-0 (2560x1440)"
        mine
        full={false}
        onToggle={onToggle}
        {...overrides}
      />
    </ul>,
  );
  return onToggle;
}

beforeEach(() => {
  usePicture.mockReset().mockReturnValue(PICTURE);
});

describe("a shared screen on the card", () => {
  it("says what is being shared", () => {
    // The whole point of #70: somebody sharing has to be able to tell their
    // terminal from their inbox.
    draw();

    expect(screen.getByRole("listitem")).toHaveTextContent("DP-0 (2560x1440)");
  });

  it("draws this session's own screen", () => {
    draw();

    const tile = screen.getByRole("button");
    expect(within(tile).getByRole("img", { name: "Your screen" })).toBeVisible();
  });

  it("names somebody else's screen and asks for no picture", () => {
    // There is one local capture and no path from anybody else's. Until there
    // is, a tile saying who is presenting is the honest half of this.
    draw({ label: "Ada's screen", mine: false });

    expect(screen.getByRole("button")).toHaveAccessibleName("Ada's screen");
    expect(screen.queryByRole("img", { name: "Your screen" })).toBeNull();
    expect(usePicture).not.toHaveBeenCalled();
  });

  it("fills the window when clicked", async () => {
    const onToggle = draw();

    await userEvent.click(screen.getByRole("button"));

    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("offers the way back once the card fills the window", async () => {
    // A view somebody cannot leave is not finished, and the control that got
    // them there is the first place they will try.
    draw({ full: true });

    expect(screen.getByRole("button")).toHaveAttribute(
      "title",
      "Back to the card",
    );
  });

  it("offers to fill the window while it is only a tile", () => {
    draw();

    expect(screen.getByRole("button")).toHaveAttribute(
      "title",
      "Fill the window",
    );
  });
});
