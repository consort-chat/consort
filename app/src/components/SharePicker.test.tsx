import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SharePicker } from "./SharePicker";
import { shareSources } from "../lib/api";
import type { ShareSource } from "../lib/api";

vi.mock("../lib/api", () => ({
  shareSources: vi.fn(),
  asCommandError: (error: unknown) =>
    typeof error === "object" && error !== null && "message" in error
      ? (error as { message: string })
      : null,
}));

const listed = vi.mocked(shareSources);

function screenOf(connector: string, width = 2560, height = 1440): ShareSource {
  return {
    id: `screen:${connector}`,
    title: `${connector} (${width}x${height})`,
    kind: "screen",
    fullscreen: false,
    width,
    height,
  };
}

function windowOf(id: number, title: string, fullscreen = false): ShareSource {
  return {
    id: `window:${id}`,
    title,
    kind: "window",
    fullscreen,
    width: 1280,
    height: 720,
  };
}

/** Draw the picker and wait for the list it asks for on mount. */
async function open(sources: ShareSource[], onPick = vi.fn()) {
  listed.mockResolvedValue(sources);
  const onClose = vi.fn();
  render(<SharePicker onPick={onPick} onClose={onClose} />);
  await waitFor(() => expect(listed).toHaveBeenCalled());
  return { onPick, onClose };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("what it offers", () => {
  it("opens on the applications tab, not on the whole screen", async () => {
    // The safer of the two defaults. A whole screen shows everything on it,
    // including whatever notification arrives next, and a person who came to
    // share one window should not have to navigate away from the option that
    // shows more than they meant.
    await open([screenOf("DP-0"), windowOf(1, "Firefox")]);

    expect(
      screen.getByRole("tab", { name: /applications/i }),
    ).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("button", { name: /Firefox/ })).toBeInTheDocument();
  });

  it("keeps windows and screens on their own tabs", async () => {
    await open([screenOf("DP-0"), windowOf(1, "Firefox")]);

    expect(
      screen.queryByRole("button", { name: /DP-0/ }),
    ).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("tab", { name: /screens/i }));

    expect(screen.getByRole("button", { name: /DP-0/ })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Firefox/ }),
    ).not.toBeInTheDocument();
  });

  it("offers every screen when there is more than one monitor", async () => {
    await open([screenOf("DP-0"), screenOf("HDMI-0", 1920, 1080)]);

    await userEvent.click(screen.getByRole("tab", { name: /screens/i }));

    expect(screen.getByRole("button", { name: /DP-0/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /HDMI-0/ })).toBeInTheDocument();
  });

  it("offers the windows in the order the backend gave them", async () => {
    // The ordering is decided in Rust, where the stacking order and the
    // fullscreen flag are. Re-sorting here would be a second answer to drift
    // from the first.
    await open([
      windowOf(1, "Game", true),
      windowOf(2, "Browser"),
      windowOf(3, "Notes"),
    ]);

    const names = screen
      .getAllByRole("button", { name: /Game|Browser|Notes/ })
      .map((button) => button.textContent);

    expect(names[0]).toContain("Game");
    expect(names[1]).toContain("Browser");
    expect(names[2]).toContain("Notes");
  });

  it("says which entry is filling a screen", async () => {
    // The acceptance criteria's game. Rust finds it; this is what makes it
    // visible, so somebody scanning the list can tell the game from a window
    // that happens to be named like one.
    await open([windowOf(1, "Helldivers 2", true), windowOf(2, "Browser")]);

    const game = screen.getByRole("button", { name: /Helldivers 2/ });

    expect(game).toHaveAttribute("data-fullscreen", "true");
    expect(
      screen.getByRole("button", { name: /Browser/ }),
    ).toHaveAttribute("data-fullscreen", "false");
  });
});

describe("choosing something", () => {
  it("hands back the id rather than the title", async () => {
    // The title is for a person and can repeat: two terminals are both
    // "Terminal". The id is what names one drawable.
    const { onPick } = await open([windowOf(7, "Terminal")]);

    await userEvent.click(screen.getByRole("button", { name: /Terminal/ }));

    expect(onPick).toHaveBeenCalledWith("window:7");
  });

  it("does not share anything until something is chosen", async () => {
    // Opening a picker is not consent. Nothing may be captured by the act of
    // looking at the list.
    const { onPick } = await open([screenOf("DP-0"), windowOf(1, "Firefox")]);

    await userEvent.click(screen.getByRole("tab", { name: /screens/i }));

    expect(onPick).not.toHaveBeenCalled();
  });

  it("closes without sharing when Escape is pressed", async () => {
    const { onPick, onClose } = await open([windowOf(1, "Firefox")]);

    await userEvent.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalled();
    expect(onPick).not.toHaveBeenCalled();
  });

  it("closes without sharing when the cancel control is pressed", async () => {
    const { onPick, onClose } = await open([windowOf(1, "Firefox")]);

    await userEvent.click(screen.getByRole("button", { name: /cancel/i }));

    expect(onClose).toHaveBeenCalled();
    expect(onPick).not.toHaveBeenCalled();
  });
});

describe("when there is nothing to offer", () => {
  it("says so rather than drawing an empty card", async () => {
    await open([]);

    expect(screen.getByRole("status")).toHaveTextContent(/nothing/i);
  });

  it("says why when the display server cannot be read", async () => {
    // The Wayland case from #70. Somebody there needs to know this is a path
    // Consort has not built rather than something broken on their machine.
    listed.mockRejectedValue({
      message: "sharing a screen needs an X11 session",
      detail: "listing what can be shared: no display",
    });
    render(<SharePicker onPick={vi.fn()} onClose={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/X11/),
    );
  });

  it("offers no way to share when the list could not be read", async () => {
    // A refusal must not leave a stale list behind to click.
    listed.mockRejectedValue({ message: "no display", detail: "no display" });
    const onPick = vi.fn();
    render(<SharePicker onPick={onPick} onClose={vi.fn()} />);

    await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());

    expect(
      screen.queryAllByRole("button", { name: /window:|screen:/ }),
    ).toHaveLength(0);
  });
});

describe("reaching it without a mouse", () => {
  it("names the tabs and the list for a screen reader", async () => {
    await open([windowOf(1, "Firefox")]);

    const tabs = screen.getByRole("tablist");

    expect(within(tabs).getAllByRole("tab")).toHaveLength(2);
    expect(screen.getByRole("tabpanel")).toBeInTheDocument();
  });

  it("puts focus inside the card so Escape and Tab reach it", async () => {
    await open([windowOf(1, "Firefox")]);

    await waitFor(() =>
      expect(
        screen.getByRole("tab", { name: /applications/i }),
      ).toHaveFocus(),
    );
  });
});
