import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { EmojiPicker } from "./EmojiPicker";
import { emojiSettings, emojiUsed, setEmojiTone } from "../lib/api";
import type { EmojiSet } from "../lib/emoji";

/** A set small enough that nothing here waits on eight hundred kilobytes. */
const SMALL: EmojiSet = {
  groups: [
    {
      name: "Smileys & Emotion",
      slug: "smileys-emotion",
      emoji: [
        { key: "😀", name: "grinning face", terms: ["grinning"], skins: [] },
      ],
    },
    {
      name: "People & Body",
      slug: "people-body",
      emoji: [
        {
          key: "👋",
          name: "waving hand",
          terms: ["waving", "wave"],
          skins: ["👋🏻", "👋🏼", "👋🏽", "👋🏾", "👋🏿"],
        },
      ],
    },
  ],
  tones: [
    { name: "light skin tone", swatch: "🏻" },
    { name: "medium-light skin tone", swatch: "🏼" },
    { name: "medium skin tone", swatch: "🏽" },
    { name: "medium-dark skin tone", swatch: "🏾" },
    { name: "dark skin tone", swatch: "🏿" },
  ],
};

vi.mock("../lib/emoji", async (importOriginal) => ({
  // `matching` and `inTone` stay real: the picker is drawn out of what they
  // answer, and stubbing them would test nothing.
  ...(await importOriginal<typeof import("../lib/emoji")>()),
  loadEmoji: vi.fn(),
}));

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  emojiSettings: vi.fn(),
  emojiUsed: vi.fn(),
  setEmojiTone: vi.fn(),
}));

const { loadEmoji } = await import("../lib/emoji");

beforeEach(() => {
  vi.mocked(loadEmoji).mockResolvedValue(SMALL);
  vi.mocked(emojiSettings).mockResolvedValue({ recent: ["🎉"], tone: 0 });
  vi.mocked(emojiUsed).mockResolvedValue({ recent: ["😀", "🎉"], tone: 0 });
  vi.mocked(setEmojiTone).mockResolvedValue(undefined);
});

function draw(props: Partial<Parameters<typeof EmojiPicker>[0]> = {}) {
  const onPick = vi.fn();
  const onClose = vi.fn();
  render(
    <EmojiPicker
      action="React with"
      onPick={onPick}
      onClose={onClose}
      {...props}
    />,
  );
  return { onPick, onClose, user: userEvent.setup() };
}

/** Wait for the dataset to arrive and the grid to be drawn. */
const drawn = () =>
  screen.findByRole("button", { name: "React with grinning face" });

describe("opening the picker", () => {
  it("says it is fetching before the data is there", () => {
    // Most of a megabyte, read off disk on the first open of a session. A
    // panel that drew nothing until it arrived would look broken.
    vi.mocked(loadEmoji).mockReturnValue(new Promise(() => {}));
    draw();

    expect(screen.getByRole("status")).toHaveTextContent(/emoji/i);
  });

  it("draws the grid once it has arrived", async () => {
    draw();

    expect(await drawn()).toBeVisible();
  });

  it("puts the row it remembers above the categories", async () => {
    draw();
    await drawn();

    const recent = screen.getByRole("group", { name: /recent/i });
    expect(within(recent).getByRole("button", { name: "React with 🎉" }))
      .toBeVisible();
  });

  it("carries on with an empty row when the settings cannot be read", async () => {
    // Failing to remember what somebody reacted with last week is not a
    // reason to refuse to let them react now.
    vi.mocked(emojiSettings).mockRejectedValue(new Error("no file"));
    vi.spyOn(console, "error").mockImplementation(() => {});
    draw();

    expect(await drawn()).toBeVisible();
    expect(screen.queryByRole("group", { name: /recent/i })).toBeNull();
  });
});

describe("what it does with a key", () => {
  it("hands it to the caller and remembers it", async () => {
    const { onPick, user } = draw();
    await drawn();

    await user.click(screen.getByRole("button", { name: "React with grinning face" }));

    expect(onPick).toHaveBeenCalledWith("😀");
    expect(emojiUsed).toHaveBeenCalledWith("😀");
  });

  it("redraws the row from what was written rather than guessing", async () => {
    const { user } = draw();
    await drawn();

    await user.click(screen.getByRole("button", { name: "React with grinning face" }));

    const recent = screen.getByRole("group", { name: /recent/i });
    await waitFor(() => {
      expect(
        within(recent)
          .getAllByRole("button")
          .map((one) => one.textContent),
      ).toEqual(["😀", "🎉"]);
    });
  });

  it("still tells the caller when remembering it fails", async () => {
    // The reaction has been sent by then. Losing the row entry is a smaller
    // problem than a press that appears to do nothing.
    vi.mocked(emojiUsed).mockRejectedValue(new Error("read only"));
    vi.spyOn(console, "error").mockImplementation(() => {});
    const { onPick, user } = draw();
    await drawn();

    await user.click(screen.getByRole("button", { name: "React with grinning face" }));

    expect(onPick).toHaveBeenCalledWith("😀");
  });
});

describe("the skin tone it remembers", () => {
  it("applies the one that was stored", async () => {
    vi.mocked(emojiSettings).mockResolvedValue({ recent: [], tone: 3 });
    const { onPick, user } = draw();
    await drawn();

    await user.click(screen.getByRole("button", { name: "People & Body" }));
    await user.click(
      screen.getByRole("button", {
        name: "React with waving hand, medium skin tone",
      }),
    );

    expect(onPick).toHaveBeenCalledWith("👋🏽");
  });

  it("writes a new one down and uses it straight away", async () => {
    const { onPick, user } = draw();
    await drawn();

    await user.click(screen.getByRole("button", { name: "dark skin tone" }));
    await user.click(screen.getByRole("button", { name: "People & Body" }));
    await user.click(
      screen.getByRole("button", {
        name: "React with waving hand, dark skin tone",
      }),
    );

    expect(setEmojiTone).toHaveBeenCalledWith(5);
    expect(onPick).toHaveBeenCalledWith("👋🏿");
  });
});

describe("shutting it", () => {
  it("closes on Escape, and keeps the press to itself", async () => {
    /*
      One press, one thing. Without stopping it here, Escape would also shut
      the room panel behind this, which listens on the window: the event
      reaches the document first, so stopping it there is what keeps it from
      going any further.
    */
    const behind = vi.fn();
    window.addEventListener("keydown", behind);
    const { onClose, user } = draw();
    await drawn();

    await user.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalled();
    expect(behind).not.toHaveBeenCalled();
    window.removeEventListener("keydown", behind);
  });

  it("closes when the pointer goes down outside it", async () => {
    const { onClose, user } = draw();
    await drawn();

    await user.click(document.body);

    expect(onClose).toHaveBeenCalled();
  });

  it("stays open when the pointer goes down inside it", async () => {
    const { onClose, user } = draw();
    await drawn();

    await user.click(screen.getByRole("searchbox", { name: /search/i }));

    expect(onClose).not.toHaveBeenCalled();
  });
});
