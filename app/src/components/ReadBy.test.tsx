import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const memberAvatar = vi.hoisted(() => vi.fn());
const memberProfile = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberAvatar,
  memberProfile,
}));

import { ReadBy, readBySays } from "./ReadBy";
import { publishedReaders, resetReaders } from "../lib/readers";
import { resetAvatarCache } from "../lib/avatars";
import { resetPresenceCache } from "../lib/presence";
import type { ReadOn } from "../lib/api";

const GENERAL = "!general:example.org";
const ADA = "@ada:example.org";
const BOB = "@bob:example.org";
const SAID = "$said:example.org";

beforeEach(() => {
  resetReaders();
  resetAvatarCache();
  resetPresenceCache();
  memberAvatar.mockReset().mockResolvedValue(null);
  memberProfile.mockReset().mockResolvedValue(null);
});

/** Publish one answer about the room's own timeline. */
function arriving(on: ReadOn[]) {
  publishedReaders({ roomId: GENERAL, main: on });
}

function draw(names: Record<string, string> = {}) {
  return render(<ReadBy roomId={GENERAL} eventId={SAID} names={names} />);
}

/** The row, if one was drawn. */
function row(container: HTMLElement): HTMLElement | null {
  return container.querySelector(".read-by");
}

describe("what is drawn", () => {
  it("draws nothing at all when nobody has read it", () => {
    // Not an empty row holding space open. A message nobody has read yet is
    // the ordinary case in a quiet room, and a gutter of blanks down the
    // length of one is worse than the information is worth.
    const { container } = draw();

    expect(container).toBeEmptyDOMElement();
  });

  it("draws a face for each person who has read it", () => {
    arriving([{ eventId: SAID, readers: [ADA, BOB] }]);

    const { container } = draw({ [ADA]: "Ada", [BOB]: "Bob" });

    expect(container.querySelectorAll(".read-by__face")).toHaveLength(2);
  });

  it("names each face for a pointer", () => {
    arriving([{ eventId: SAID, readers: [ADA] }]);

    draw({ [ADA]: "Ada" });

    expect(screen.getByTitle("Ada")).toBeTruthy();
  });

  it("names people by their user ID when the room has not said otherwise", () => {
    arriving([{ eventId: SAID, readers: [ADA] }]);

    draw();

    expect(screen.getByTitle(ADA)).toBeTruthy();
  });

  it("says how many more there are than it can show", () => {
    arriving([{ eventId: SAID, readers: [ADA, BOB], more: 45 }]);

    draw({ [ADA]: "Ada", [BOB]: "Bob" });

    expect(screen.getByText("+45")).toBeTruthy();
  });

  it("says nothing about an overflow when there is none", () => {
    arriving([{ eventId: SAID, readers: [ADA] }]);

    draw({ [ADA]: "Ada" });

    expect(screen.queryByText(/^\+/)).toBeNull();
  });
});

describe("what it says about the people it cannot see", () => {
  /*
    The decision this file exists to pin, and the one most likely to generate a
    false bug report. Somebody sending `m.read.private` is visible to nobody,
    so they never appear in anybody's row, and turning your own off does not
    stop you seeing other people. Silently showing fewer faces than there are
    people in the room is indistinguishable from the feature being broken.

    So the row always says that it shows only the people who share receipts,
    whether or not anybody in this room has turned theirs off. Always, because
    saying it only when somebody is missing is itself the disclosure: a
    sentence that appeared the moment a particular colleague went quiet would
    tell you the thing they chose not to share. And for the same reason the row
    never counts the people it cannot see: "three of twelve" is an exact
    disclosure in a room of two and a narrowing one in a room of ten.
  */

  it("says it shows only the people who share receipts", () => {
    arriving([{ eventId: SAID, readers: [ADA] }]);

    const { container } = draw({ [ADA]: "Ada" });

    expect(row(container)?.getAttribute("aria-label")).toMatch(
      /only people who share read receipts/i,
    );
  });

  it("says it in a room where nobody is missing, just the same", () => {
    // The sentence cannot be a signal, or it becomes the leak it exists to
    // avoid. It reads identically either way.
    const alone = readBySays(["Ada"], 0);
    const crowded = readBySays(["Ada"], 45);

    for (const said of [alone, crowded]) {
      expect(said).toMatch(/only people who share read receipts/i);
    }
  });

  it("never counts the people it cannot see", () => {
    // There is no count of the unseen in the payload and there must be none
    // in the markup: the difference between a room's size and the faces drawn
    // is what would name somebody.
    arriving([{ eventId: SAID, readers: [ADA] }]);

    const { container } = draw({ [ADA]: "Ada" });

    expect(container.textContent).not.toMatch(/\bof\s+\d/);
    expect(row(container)?.getAttribute("aria-label")).not.toMatch(/\bof\s+\d/);
  });

  it("names the readers it does have", () => {
    arriving([{ eventId: SAID, readers: [ADA, BOB] }]);

    const { container } = draw({ [ADA]: "Ada", [BOB]: "Bob" });

    expect(row(container)?.getAttribute("aria-label")).toMatch(/Ada.*Bob/);
  });

  it("counts the faces it could not fit, which is not the same claim", () => {
    // How many more have read it, not how many have not. Every one of these
    // is somebody whose receipt this account can see.
    expect(readBySays(["Ada"], 45)).toMatch(/45 more/);
  });

  it("says nothing about an overflow that is not there", () => {
    // "Ada and 0 more" is the shape this goes wrong in, and it reads as a
    // claim about people who are not being shown, which is the one thing the
    // row must not make.
    expect(readBySays(["Ada", "Bob"], 0)).toBe(
      "Read by Ada and Bob. Only people who share read receipts are shown.",
    );
  });
});

describe("which conversation it is about", () => {
  it("reads a thread's own answer when it is in one", () => {
    publishedReaders({
      roomId: GENERAL,
      main: [{ eventId: SAID, readers: [ADA] }],
      thread: {
        rootId: "$root:example.org",
        on: [{ eventId: SAID, readers: [BOB] }],
      },
    });

    render(
      <ReadBy
        roomId={GENERAL}
        threadRoot="$root:example.org"
        eventId={SAID}
        names={{ [ADA]: "Ada", [BOB]: "Bob" }}
      />,
    );

    expect(screen.getByTitle("Bob")).toBeTruthy();
    expect(screen.queryByTitle("Ada")).toBeNull();
  });
});
