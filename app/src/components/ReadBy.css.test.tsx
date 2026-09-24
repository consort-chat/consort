/*
  Which end of a message the row of faces sits at, read off the stylesheet that
  draws it.

  A file of its own, because it is the only kind of test that wants CSS.
  `vitest.config.ts` runs a second project over `.css.test.tsx` with the
  stylesheets processed, and `RoomTimeline.css.test.tsx` says why that cannot
  be on for the whole run.
*/
import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const memberAvatar = vi.hoisted(() => vi.fn());
const memberProfile = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberAvatar,
  memberProfile,
}));

import { ReadBy } from "./ReadBy";
import { publishedReaders, resetReaders } from "../lib/readers";
import { resetAvatarCache } from "../lib/avatars";
import { resetPresenceCache } from "../lib/presence";

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

/** The row, with two faces and however many more it could not fit. */
function drawTheRow(more?: number) {
  publishedReaders({
    roomId: GENERAL,
    main: [
      {
        eventId: SAID,
        readers: [ADA, BOB],
        ...(more === undefined ? {} : { more }),
      },
    ],
  });

  const { container } = render(
    <ReadBy
      roomId={GENERAL}
      eventId={SAID}
      names={{ [ADA]: "Ada", [BOB]: "Bob" }}
    />,
  );

  return container.querySelector(".read-by") as HTMLElement;
}

/*
  jsdom does no layout, so there is no box to ask which side the row came out
  on. It does resolve the cascade, which is enough to read back the end the row
  packs itself against. A rule that stops matching fails these too, because
  `getComputedStyle` then answers with the initial value.
*/
describe("which end of the message the faces sit at", () => {
  it("packs the faces against the far end of the row", () => {
    const row = drawTheRow();

    expect(getComputedStyle(row).justifyContent).toBe("flex-end");
  });

  it("keeps the count beside the faces rather than against the edge", () => {
    // An auto margin is the shape this goes wrong in: it would strand the
    // count at the edge and leave the faces behind in the middle of the row.
    const row = drawTheRow(45);
    const count = row.querySelector(".read-by__more") as HTMLElement;

    expect(count.previousElementSibling).toHaveClass("read-by__face");
    expect(getComputedStyle(count).marginLeft).not.toBe("auto");
  });
});
