/*
  Where the mark takes its colour from. The risk is not the hue, it is somebody
  writing the hue down instead of naming the token that owns it.

  Named `.css.test.tsx` because it reads a stylesheet. See `vitest.config.ts`.
*/
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import "../styles/tokens.css";
import "./RoomTimeline.css";

import { UntrustedMark } from "./UntrustedMark";

/** The token a declaration points at, or `null` if it names a colour outright. */
function tokenBehind(value: string): string | null {
  return /^var\((--[a-z0-9-]+)\)$/.exec(value.trim())?.[1] ?? null;
}

/** What `:root` says that token is. */
function rootValue(token: string): string {
  return getComputedStyle(document.documentElement)
    .getPropertyValue(token)
    .trim();
}

describe("the colour of the mark", () => {
  it("comes from a token rather than a colour written into the component", () => {
    render(<UntrustedMark trust="unsignedDevice" />);

    const token = tokenBehind(getComputedStyle(screen.getByRole("img")).color);

    expect(token).not.toBeNull();
  });

  it("is the warning red, not a hue that is spoken for elsewhere", () => {
    // `tokens.css` reserves the mint for voice and the gold for a mention, and
    // says why: three colours mean three things only while each means one.
    render(<UntrustedMark trust="unsignedDevice" />);

    const token = tokenBehind(getComputedStyle(screen.getByRole("img")).color);

    expect(token).toBe("--danger");
  });

  it("names a token the token file actually defines", () => {
    render(<UntrustedMark trust="unsignedDevice" />);

    const token = tokenBehind(getComputedStyle(screen.getByRole("img")).color);

    expect(rootValue(token ?? "--missing")).toMatch(/^oklch\(/);
  });
});

describe("the row holding the mark", () => {
  it("leaves room for it, so the words do not start underneath it", () => {
    render(<UntrustedMark trust="unsignedDevice" />);
    const row = document.createElement("div");
    row.className = "timeline__message";
    row.dataset.untrusted = "true";
    document.body.append(row);

    // From the space scale, like every other gap in this row.
    expect(getComputedStyle(row).paddingLeft).toMatch(/var\(--space-/);
  });
});
