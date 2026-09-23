/*
  What the row under a message measures, read off the stylesheet that draws it.

  WCAG 2.5.8 asks 24 by 24 CSS pixels of anything there is to press, and this
  row has been under it twice: the thread pill (#82), then the reaction pills
  and the control that adds one (#102). Each time the control took its height
  from the words or the glyph inside it, which is the shape where reading the
  stylesheet and believing it is exactly what fails.

  A file of its own, because it is the only kind of test that wants CSS.

  Vitest replaces CSS imports with nothing, which is right for almost
  everything: no test should turn on a colour. Turning it back on for the whole
  run is not an option either. `RoomTimeline.tsx` imports this stylesheet, the
  toolbar in this same row is `pointer-events: none` until it is hovered, and
  with the sheet live `userEvent` refuses to press those controls: 65 tests
  across two files fail on styling that is doing its job.

  So `vitest.config.ts` runs two projects and `.css.test.tsx` is what picks
  this one. Anything measuring a stylesheet belongs in a file named that way,
  and anything pressing a control does not.
*/
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

// Imported here because the component does not import it: these controls are
// drawn by `MessageGroups` and sized by the stylesheet `RoomTimeline` owns.
import "./RoomTimeline.css";

import { MessageGroups, group } from "./MessageGroups";
import type { Message } from "../lib/api";

// Nothing here presses anything, so the IPC only has to stay quiet: a picture
// nobody has, and a person nothing is known about.
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberAvatar: vi.fn().mockResolvedValue(null),
  memberProfile: vi.fn().mockResolvedValue({
    presence: "unknown",
    status: null,
    lastActiveAgo: null,
  }),
}));

const GENERAL = "!general:example.org";
const ADA = "@ada:example.org";
const BOB = "@bob:example.org";

/** One message carrying all three of the controls this row can draw. */
const BUSY: Message = {
  id: "$1",
  sender: ADA,
  at: Date.parse("2026-01-01T12:00:00Z"),
  body: "it works",
  kind: "text",
  reactions: [{ key: "🎉", count: 2 }],
  thread: { count: 3, participated: false },
};

/** What SC 2.5.8 (Target Size, Minimum) asks for, in CSS pixels. */
const FLOOR = 24;

function drawTheRow() {
  render(
    <MessageGroups
      groups={group([BUSY])}
      names={{ [ADA]: "Ada" }}
      roomId={GENERAL}
      selfId={BOB}
      known={new Map([[BUSY.id, BUSY]])}
      onAbout={vi.fn()}
      onOpenThread={vi.fn()}
      onReact={vi.fn()}
    />,
  );
}

/*
  Height, on all three, and not width. They are wide enough already: a pill's
  width grows with the glyph and the count, and the add control was drawn at
  26px from the start. Asserting a width here would be asserting something that
  never broke.

  jsdom does no layout, so there is no box to ask how tall it came out. It does
  resolve the cascade, which is enough to read back the floor each control
  declares for itself. A number that drops fails, and so does a rule that stops
  matching or a class the component stops using, because `getComputedStyle`
  answers with nothing and `parseFloat` makes that `NaN`.
*/
function standsAtLeastTheFloor(control: HTMLElement) {
  expect(
    parseFloat(getComputedStyle(control).minHeight),
  ).toBeGreaterThanOrEqual(FLOOR);
}

describe("the size of what there is to press under a message", () => {
  it("stands a reaction pill tall enough to hit", () => {
    drawTheRow();

    standsAtLeastTheFloor(screen.getByRole("button", { name: "🎉, 2" }));
  });

  it("stands the control that adds one just as tall", () => {
    // It sits in the row with the pills and was the shorter of the two, so
    // growing them without it would have made the row less even, not more.
    drawTheRow();

    standsAtLeastTheFloor(
      screen.getByRole("button", { name: "Add a reaction" }),
    );
  });
});
