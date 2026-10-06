/*
  Where the brand mark takes its two colours from.

  Named `.css.test.tsx` because it reads a stylesheet. See `vitest.config.ts`.
*/
import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import "../styles/tokens.css";
import "./LoginScreen.css";

import { LoginScreen } from "./LoginScreen";

function mark() {
  const { container } = render(<LoginScreen onSignedIn={vi.fn()} />);
  const svg = container.querySelector("svg.login__mark");
  if (svg === null) throw new Error("the login page draws no brand mark");
  return svg;
}

describe("the colours of the brand mark", () => {
  it("strokes the C with the primary text colour, named as a token", () => {
    const c = mark().querySelector(".login__mark-c");

    expect(getComputedStyle(c!).stroke).toBe("var(--text-primary)");
  });

  it("fills the level bars with the mint, which is what they are a meter for", () => {
    // `tokens.css` reserves the mint for voice and presence. The bars in the
    // app mark are a level meter, so this is the one decorative-looking use
    // that is not decorative.
    const bars = mark().querySelector(".login__mark-bars");

    expect(getComputedStyle(bars!).fill).toBe("var(--mint)");
  });
});

describe("where the mark sits", () => {
  it("starts at the edge the wordmark starts at", () => {
    // An SVG that stretches centres its own glyph, which leaves the mark
    // floating mid-column while every line under it is flush left.
    expect(getComputedStyle(mark()).alignSelf).toBe("flex-start");
  });
});
