import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ScreenStage } from "./ScreenStage";

/** Whatever the card put on the stage. Which screen it is is the card's call. */
const PICTURE = <img src="data:image/jpeg;base64,aaaa" alt="DP-0" />;

function draw(overrides: Partial<Parameters<typeof ScreenStage>[0]> = {}) {
  const onToggle = vi.fn();
  render(
    <ScreenStage
      label="DP-0 (2560x1440)"
      picture={PICTURE}
      full={false}
      onToggle={onToggle}
      {...overrides}
    />,
  );
  return onToggle;
}

describe("the screen on the stage", () => {
  it("says what is being shared", () => {
    draw();

    expect(screen.getByRole("button")).toHaveTextContent("DP-0 (2560x1440)");
  });

  it("draws the picture it was handed, over the glyph", () => {
    draw();

    expect(
      within(screen.getByRole("button")).getByRole("img", { name: "DP-0" }),
    ).toBeVisible();
  });

  it("is still a named stage with no picture to draw", () => {
    // Every share starts here: the stream is known before its first frame, and
    // somebody else's may never arrive. A stage that drew nothing until one
    // did would make a share look like it had not started.
    draw({ label: "Ada's screen", picture: undefined });

    expect(screen.getByRole("button")).toHaveTextContent("Ada's screen");
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("fills the window when clicked", async () => {
    const onToggle = draw();

    await userEvent.click(screen.getByRole("button"));

    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("fills the window from the keyboard", async () => {
    // The stage is the biggest control on the card. One only a mouse can
    // reach is one some people do not have.
    const onToggle = draw();

    await userEvent.tab();
    await userEvent.keyboard("{Enter}");

    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("keeps one name and says the state separately", () => {
    // A button whose name changes under the cursor is announced as a new
    // button, which is why the card's own controls do it this way.
    draw({ full: true });

    const stage = screen.getByRole("button");
    expect(stage).toHaveAccessibleName("DP-0 (2560x1440), fill the window");
    expect(stage).toHaveAttribute("aria-pressed", "true");
    expect(stage).toHaveAttribute("title", "Back to the card");
  });

  it("offers to fill the window while it is only a card", () => {
    draw();

    const stage = screen.getByRole("button");
    expect(stage).toHaveAttribute("aria-pressed", "false");
    expect(stage).toHaveAttribute("title", "Fill the window");
  });
});
