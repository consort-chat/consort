import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const usePicture = vi.hoisted(() => vi.fn());
vi.mock("../lib/usePicture", () => ({ usePicture }));

import { ScreenTile } from "./ScreenTile";

const PICTURE = "data:image/jpeg;base64,aaaa";

/** One tile in a list, because that is where the card puts it. */
function draw(overrides: Partial<Parameters<typeof ScreenTile>[0]> = {}) {
  const onPick = vi.fn();
  render(
    <ul aria-label="Other screens shared in Lounge">
      <ScreenTile
        label="DP-0 (2560x1440)"
        mine
        onPick={onPick}
        {...overrides}
      />
    </ul>,
  );
  return onPick;
}

beforeEach(() => {
  usePicture.mockReset().mockReturnValue(PICTURE);
});

describe("a shared screen waiting in the strip", () => {
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

    expect(screen.getByRole("button")).toHaveTextContent("Ada's screen");
    expect(screen.queryByRole("img", { name: "Your screen" })).toBeNull();
    expect(usePicture).not.toHaveBeenCalled();
  });

  it("says what pressing it does, not only whose screen it is", () => {
    // The tile moved from filling the window to taking the stage. A name that
    // is only the label leaves the one control that rearranges the card
    // announced as a caption.
    draw({ label: "Ada's screen", mine: false });

    expect(screen.getByRole("button")).toHaveAccessibleName(
      "Put Ada's screen on the stage",
    );
    expect(screen.getByRole("button")).toHaveAttribute(
      "title",
      "Put it on the stage",
    );
  });

  it("takes the stage when clicked", async () => {
    const onPick = draw();

    await userEvent.click(screen.getByRole("button"));

    expect(onPick).toHaveBeenCalledTimes(1);
  });

  it("takes the stage from the keyboard", async () => {
    // Promoting a tile is an interaction now, so it has to be reachable
    // without a pointer.
    const onPick = draw();

    await userEvent.tab();
    await userEvent.keyboard("{Enter}");

    expect(onPick).toHaveBeenCalledTimes(1);
  });
});
