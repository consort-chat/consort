import { afterEach, describe, expect, it, vi } from "vitest";

import { canPlay } from "./playable";

/**
 * Answer `canPlayType` with `answer` for every type asked about.
 *
 * The real one reads WebKitGTK's GStreamer registry, which is the whole point
 * of the check and is exactly what a test cannot have.
 */
function answering(answer: string) {
  const spy = vi
    .spyOn(HTMLMediaElement.prototype, "canPlayType")
    .mockReturnValue(answer as CanPlayTypeResult);
  return spy;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("canPlay", () => {
  it("is sure when the browser is", () => {
    answering("probably");

    expect(canPlay("video/mp4")).toBe("yes");
  });

  it("takes a maybe as a yes", () => {
    // The empty string is the only definite no the API has. Refusing on a
    // maybe would refuse clips that play, which is a worse mistake than
    // letting the player try and fall back when it fails.
    answering("maybe");

    expect(canPlay("video/mp4")).toBe("yes");
  });

  it("is sure of a no when the browser says nothing at all", () => {
    // The empty string is a definite no, and on this machine it is what a
    // GStreamer with no H.264 decoder answers. That is the case the whole
    // check exists for.
    answering("");

    expect(canPlay("video/mp4")).toBe("no");
  });

  it("does not guess for a clip whose sender named no type", () => {
    // A bridge that omits `info.mimetype` is common. There is nothing to ask
    // about, so the answer is that nobody knows and the player gets its go.
    const spy = answering("");

    expect(canPlay(undefined)).toBe("unknown");
    expect(spy).not.toHaveBeenCalled();
  });

  it("asks about the codecs rather than the container alone", () => {
    // `video/mp4` on its own gets a "maybe" out of a browser with no decoder
    // at all, because the container is one it knows. Naming the codecs is what
    // turns the question into one with a real answer.
    const spy = answering("probably");

    canPlay("video/mp4");

    expect(spy).toHaveBeenCalledWith(expect.stringContaining("codecs="));
  });

  it("does not know about a container it has no codec guess for", () => {
    const spy = answering("");

    expect(canPlay("video/quicktime")).toBe("unknown");
    expect(spy).not.toHaveBeenCalled();
  });
});
