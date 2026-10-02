import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const usePicture = vi.hoisted(() => vi.fn());
vi.mock("../lib/usePicture", () => ({ usePicture }));

import { ScreenStage } from "./ScreenStage";

const PICTURE = "data:image/jpeg;base64,aaaa";

function draw(overrides: Partial<Parameters<typeof ScreenStage>[0]> = {}) {
  const onToggle = vi.fn();
  render(
    <ScreenStage
      label="DP-0 (2560x1440)"
      mine
      full={false}
      onToggle={onToggle}
      {...overrides}
    />,
  );
  return onToggle;
}

beforeEach(() => {
  usePicture.mockReset().mockReturnValue(PICTURE);
});

describe("the screen on the stage", () => {
  it("says what is being shared", () => {
    draw();

    expect(screen.getByRole("button")).toHaveTextContent("DP-0 (2560x1440)");
  });

  it("draws this session's own screen", () => {
    draw();

    expect(
      within(screen.getByRole("button")).getByRole("img", {
        name: "Your screen",
      }),
    ).toBeVisible();
  });

  it("names somebody else's screen and asks for no picture", () => {
    // One local capture, and no path from anybody else's into this window.
    draw({ label: "Ada's screen", mine: false });

    expect(screen.getByRole("button")).toHaveTextContent("Ada's screen");
    expect(screen.queryByRole("img", { name: "Your screen" })).toBeNull();
    expect(usePicture).not.toHaveBeenCalled();
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
