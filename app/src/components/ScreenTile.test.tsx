import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ScreenTile } from "./ScreenTile";

/** Whatever the card put in the square. */
const PICTURE = <img src="data:image/jpeg;base64,aaaa" alt="DP-0" />;

/** One tile in a list, because that is where the card puts it. */
function draw(overrides: Partial<Parameters<typeof ScreenTile>[0]> = {}) {
  const onPick = vi.fn();
  render(
    <ul aria-label="Other screens shared in Lounge">
      <ScreenTile
        label="DP-0 (2560x1440)"
        picture={PICTURE}
        onPick={onPick}
        {...overrides}
      />
    </ul>,
  );
  return onPick;
}

describe("a shared screen waiting in the strip", () => {
  it("says what is being shared", () => {
    // The whole point of #70: somebody sharing has to be able to tell their
    // terminal from their inbox.
    draw();

    expect(screen.getByRole("listitem")).toHaveTextContent("DP-0 (2560x1440)");
  });

  it("draws the picture it was handed, over the glyph", () => {
    draw();

    const tile = screen.getByRole("button");
    expect(within(tile).getByRole("img", { name: "DP-0" })).toBeVisible();
  });

  it("is still a named square with no picture to draw", () => {
    // A share is listed before its first frame arrives, and somebody else's
    // may never arrive at all.
    draw({ label: "Ada's screen", picture: undefined });

    expect(screen.getByRole("button")).toHaveTextContent("Ada's screen");
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("says what pressing it does, not only whose screen it is", () => {
    // The tile moved from filling the window to taking the stage. A name that
    // is only the label leaves the one control that rearranges the card
    // announced as a caption.
    draw({ label: "Ada's screen" });

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
