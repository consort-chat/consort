import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ACROSS, EmojiGrid } from "./EmojiGrid";
import type { Emoji, EmojiSet } from "../lib/emoji";

function key(
  key: string,
  name: string,
  terms: string[],
  skins: string[] = [],
): Emoji {
  return { key, name, terms, skins };
}

/**
 * Enough emoji to fill more than one row.
 *
 * The keyboard rules are about rows, so a category has to be wider than
 * [`ACROSS`] for a press of the down arrow to have anywhere to go.
 */
const FACES = Array.from({ length: ACROSS + 3 }, (_, index) =>
  key(`f${index}`, `face ${index}`, [`face`, `f${index}`]),
);

const SMALL: EmojiSet = {
  groups: [
    { name: "Smileys & Emotion", slug: "smileys-emotion", emoji: FACES },
    {
      name: "People & Body",
      slug: "people-body",
      emoji: [
        key("👍", "thumbs up", ["thumbs", "up", "thumbsup"]),
        key(
          "👋",
          "waving hand",
          ["waving", "hand", "wave"],
          ["👋🏻", "👋🏼", "👋🏽", "👋🏾", "👋🏿"],
        ),
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

function draw(props: Partial<Parameters<typeof EmojiGrid>[0]> = {}) {
  const onPick = vi.fn();
  const onTone = vi.fn();
  render(
    <EmojiGrid
      set={SMALL}
      action="React with"
      recent={[]}
      tone={0}
      onTone={onTone}
      onPick={onPick}
      {...props}
    />,
  );
  return { onPick, onTone, user: userEvent.setup() };
}

/** The box everything here is typed into. */
const searchBox = () => screen.getByRole("searchbox", { name: /search/i });

/** What the live region is currently saying. */
const announcement = () => screen.getByRole("status").textContent;

describe("finding an emoji without a mouse", () => {
  it("opens with the search box focused, which is where typing goes", () => {
    draw();

    expect(searchBox()).toHaveFocus();
  });

  it("says how many matched, in a region that announces itself", async () => {
    const { user } = draw();

    await user.keyboard("thumb");

    expect(announcement()).toBe("1 emoji matches");
  });

  it("says so when nothing matched, rather than going quiet", async () => {
    const { user } = draw();

    await user.keyboard("xyzzy");

    expect(announcement()).toBe("No emoji match");
    expect(screen.queryByRole("button", { name: /thumbs up/ })).toBeNull();
  });

  it("counts across every category, not just the one on show", async () => {
    const { user } = draw();

    // "face" is every emoji in the first category and none in the second,
    // "wave" is one in the second and none in the first.
    await user.keyboard("wave");

    expect(announcement()).toBe("1 emoji matches");
    expect(screen.getByRole("button", { name: "React with waving hand" }))
      .toBeVisible();
  });

  it("moves down out of the search box into the first key", async () => {
    const { user } = draw();

    await user.keyboard("{ArrowDown}");

    expect(screen.getByRole("button", { name: "React with face 0" }))
      .toHaveFocus();
  });

  it("moves along the row and down to the row under it", async () => {
    const { user } = draw();

    await user.keyboard("{ArrowDown}{ArrowRight}");
    expect(screen.getByRole("button", { name: "React with face 1" }))
      .toHaveFocus();

    await user.keyboard("{ArrowDown}");
    expect(
      screen.getByRole("button", { name: `React with face ${ACROSS + 1}` }),
    ).toHaveFocus();

    await user.keyboard("{ArrowLeft}{ArrowUp}");
    expect(screen.getByRole("button", { name: "React with face 0" }))
      .toHaveFocus();
  });

  it("goes back up into the search box from the top row", async () => {
    // Otherwise the only way back to the box is the mouse, which is the thing
    // this whole path is for.
    const { user } = draw();

    await user.keyboard("{ArrowDown}{ArrowUp}");

    expect(searchBox()).toHaveFocus();
  });

  it("jumps to the first and last key", async () => {
    const { user } = draw();

    await user.keyboard("{ArrowDown}{End}");
    expect(
      screen.getByRole("button", { name: `React with face ${ACROSS + 2}` }),
    ).toHaveFocus();

    await user.keyboard("{Home}");
    expect(screen.getByRole("button", { name: "React with face 0" }))
      .toHaveFocus();
  });

  it("will not walk off the end of the last row", async () => {
    const { user } = draw();

    await user.keyboard("{ArrowDown}{End}{ArrowDown}{ArrowRight}");

    const last = screen.getByRole("button", {
      name: `React with face ${ACROSS + 2}`,
    });
    expect(last).toHaveFocus();
    /*
      And the tab stop with it. A cursor that ran past the end leaves focus
      where it was, which looks right, but the stop goes with the cursor: the
      grid would then have no key in the tab order at all and Tab out of the
      search box would skip it entirely.
    */
    expect(last.tabIndex).toBe(0);
  });

  it("takes the first match on Enter, without leaving the box", async () => {
    const { onPick, user } = draw();

    await user.keyboard("thumb{Enter}");

    expect(onPick).toHaveBeenCalledWith("👍");
  });

  it("takes it in the chosen tone, the same as pressing it would", async () => {
    // Enter is a second way to press the first key, so it has to be the same
    // press. Sending the plain one from here and the toned one from a click
    // is two pills for what somebody meant as one reaction.
    const { onPick, user } = draw({ tone: 3 });

    await user.keyboard("wave{Enter}");

    expect(onPick).toHaveBeenCalledWith("👋🏽");
  });

  it("does nothing on Enter when nothing matched", async () => {
    const { onPick, user } = draw();

    await user.keyboard("xyzzy{Enter}");

    expect(onPick).not.toHaveBeenCalled();
  });

  it("holds only one key in the tab order, so Tab leaves the grid", async () => {
    const { user } = draw();

    await user.keyboard("{ArrowDown}{ArrowRight}");

    const keys = screen.getAllByRole("button", { name: /^React with face/ });
    expect(keys.filter((one) => one.tabIndex === 0)).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "React with face 1" }).tabIndex,
    ).toBe(0);
  });
});

describe("the categories", () => {
  it("draws the first one, and says which one is on show", () => {
    draw();

    const tabs = screen.getByRole("group", { name: /categor/i });
    expect(
      within(tabs).getByRole("button", { name: "Smileys & Emotion" }),
    ).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("button", { name: "React with face 0" }))
      .toBeVisible();
    expect(screen.queryByRole("button", { name: "React with thumbs up" }))
      .toBeNull();
  });

  it("shows another one when it is pressed", async () => {
    const { user } = draw();

    await user.click(screen.getByRole("button", { name: "People & Body" }));

    expect(screen.getByRole("button", { name: "React with thumbs up" }))
      .toBeVisible();
    expect(screen.queryByRole("button", { name: "React with face 0" }))
      .toBeNull();
  });

  it("names the grid after what is in it, so it is not just a pile of keys", async () => {
    const { user } = draw();

    expect(screen.getByRole("group", { name: "Smileys & Emotion" }))
      .toBeVisible();

    await user.keyboard("thumb");

    expect(screen.getByRole("group", { name: /matches/i })).toBeVisible();
  });
});

describe("what pressing a key does", () => {
  it("hands back the key itself", async () => {
    const { onPick, user } = draw();

    await user.click(screen.getByRole("button", { name: "People & Body" }));
    await user.click(screen.getByRole("button", { name: "React with thumbs up" }));

    expect(onPick).toHaveBeenCalledWith("👍");
  });

  it("says what pressing it will do, which is the only thing that differs", async () => {
    const { user } = draw({ action: "Insert" });

    await user.click(screen.getByRole("button", { name: "People & Body" }));

    expect(screen.getByRole("button", { name: "Insert thumbs up" }))
      .toBeVisible();
  });

  it("draws a key this session has already used as pressed", async () => {
    const { user } = draw({ chosen: new Set(["👍"]) });

    await user.click(screen.getByRole("button", { name: "People & Body" }));

    expect(
      screen.getByRole("button", { name: "React with thumbs up" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("button", { name: "React with waving hand" }),
    ).toHaveAttribute("aria-pressed", "false");
  });
});

describe("the recently used row", () => {
  it("draws what it was given, above the categories", async () => {
    const { onPick, user } = draw({ recent: ["👍", "f2"] });

    const recent = screen.getByRole("group", { name: /recent/i });
    const keys = within(recent).getAllByRole("button");
    expect(keys.map((one) => one.textContent)).toEqual(["👍", "f2"]);

    await user.click(keys[0]!);
    expect(onPick).toHaveBeenCalledWith("👍");
  });

  it("is not drawn at all when there is nothing in it", () => {
    draw({ recent: [] });

    expect(screen.queryByRole("group", { name: /recent/i })).toBeNull();
  });

  it("goes away while a search is running, which is not what it is for", async () => {
    const { user } = draw({ recent: ["👍"] });

    await user.keyboard("thumb");

    expect(screen.queryByRole("group", { name: /recent/i })).toBeNull();
  });

  it("draws a key nobody here has a name for", () => {
    // The property the whole picker has to keep: a key from a client with a
    // wider set than this one still draws, and can still be sent again.
    draw({ recent: ["🛸👽", "👍"] });

    const recent = screen.getByRole("group", { name: /recent/i });
    expect(within(recent).getByRole("button", { name: "React with 🛸👽" }))
      .toBeVisible();
  });
});

describe("skin tone", () => {
  it("offers the five the data knows about", () => {
    draw();

    const strip = screen.getByRole("group", { name: /skin tone/i });
    expect(
      within(strip)
        .getAllByRole("button")
        .map((one) => one.getAttribute("aria-label")),
    ).toEqual([
      "light skin tone",
      "medium-light skin tone",
      "medium skin tone",
      "medium-dark skin tone",
      "dark skin tone",
    ]);
  });

  it("sends the toned key once one is chosen", async () => {
    const { onPick, user } = draw({ tone: 3 });

    await user.click(screen.getByRole("button", { name: "People & Body" }));
    await user.click(
      screen.getByRole("button", {
        name: "React with waving hand, medium skin tone",
      }),
    );

    expect(onPick).toHaveBeenCalledWith("👋🏽");
  });

  it("leaves an emoji that has no tones alone", async () => {
    const { onPick, user } = draw({ tone: 3 });

    await user.click(screen.getByRole("button", { name: "People & Body" }));
    await user.click(screen.getByRole("button", { name: "React with thumbs up" }));

    expect(onPick).toHaveBeenCalledWith("👍");
  });

  it("asks for the tone that was pressed", async () => {
    const { onTone, user } = draw({ tone: 0 });

    await user.click(screen.getByRole("button", { name: "medium skin tone" }));

    expect(onTone).toHaveBeenCalledWith(3);
  });

  it("asks to go back to none when the chosen one is pressed again", async () => {
    // The rule the reaction pills already have: the control that turned
    // something on is the control that turns it off.
    const { onTone, user } = draw({ tone: 3 });

    await user.click(screen.getByRole("button", { name: "medium skin tone" }));

    expect(onTone).toHaveBeenCalledWith(0);
  });
});
