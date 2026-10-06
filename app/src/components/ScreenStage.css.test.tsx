/*
  The shape of the stage, read off the stylesheets that draw it.

  jsdom does no layout, so none of this can be measured here. What it does is
  parse the cascade, which is enough to hold the rules that decide the shape.
  The numbers they produce were measured in Chromium and are written down in
  docs/adr/0009-a-shared-screen-takes-the-stage.md.
*/
import { describe, expect, it } from "vitest";

import "./CallCard.css";
import "./ScreenStage.css";
import "./ScreenTile.css";
import "./CallPicture.css";

/** Every rule jsdom parsed, for the ones no element can be asked about. */
function everyRule(): readonly string[] {
  return Array.from(document.styleSheets).flatMap((sheet) =>
    Array.from(sheet.cssRules).map((rule) => rule.cssText),
  );
}

function ruleFor(selector: string): string {
  const found = everyRule().filter((rule) => rule.startsWith(`${selector} {`));
  expect(found).toHaveLength(1);
  return found.join("");
}

describe("the shape the stage letterboxes into", () => {
  it("is 16:9 while the card is floating", () => {
    // A desktop's shape. A stage without one takes its height from the picture
    // inside it, which arrives twelve and a half times a second and would move
    // every face under it each time.
    expect(ruleFor(".call-stage__picture")).toContain("aspect-ratio: 16 / 9");
  });

  it("gives that up for the height the window left once it fills it", () => {
    // Measured at 1280x800, a 16:9 box as wide as the window came out 49px
    // taller than the stage clipping it, which is a cropped desktop.
    const full = ruleFor('.call-card[data-size="full"] .call-stage__picture');
    expect(full).toContain("height: 100%");
    expect(full).toContain("aspect-ratio: auto");
  });

  it("lets the stage shrink to what the squares leave", () => {
    // A flex item will not go below its content without this, and the content
    // is a picture that would rather be 16:9 than fit.
    const stage = ruleFor('.call-card[data-size="full"] .call-stage');
    expect(stage).toContain("flex: 1 1 auto");
    expect(stage).toContain("min-height: 0");
  });

  it("never crops the picture to fill that shape", () => {
    // `contain` is the whole no-stretch, no-crop guarantee: a 4:3 desktop
    // pillarboxes in a 16:9 stage rather than losing its edges.
    expect(ruleFor('.call-picture[data-of="screen"]')).toContain(
      "object-fit: contain",
    );
  });
});

describe("the stage against the strip under it", () => {
  it("reserves the larger type for the stage's own caption", () => {
    // The hierarchy the type scale carries. Same colour, two sizes, so the
    // stage reads as the thing the call is about.
    expect(ruleFor(".call-stage__what")).toContain(
      "font-size: var(--text-sm)",
    );
    expect(ruleFor(".call-screen__what")).toContain("font-size: var(--text-xs)");
  });

  it("takes the mint from the token the speaking ring uses", () => {
    // Mint is semantic here: voice and presence, and a share is the other live
    // thing a call carries. A second green would read as a second thing.
    expect(ruleFor(".call-stage__what")).toContain("color: var(--mint)");
  });

  it("moves nothing but the compositor on hover, focus and press", () => {
    // Transform and box-shadow only. A stage that animated its own size would
    // reflow every square under it for the length of the transition.
    expect(ruleFor(".call-stage")).toContain("transition: box-shadow");
    expect(ruleFor(".call-stage:active")).toContain("transform: scale(0.995)");
    expect(ruleFor(".call-stage:hover")).toContain("var(--line-strong)");
    expect(ruleFor(".call-stage:focus-visible")).toContain(
      "box-shadow: var(--shadow-focus)",
    );
  });
});
