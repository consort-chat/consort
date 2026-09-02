import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const timelineMedia = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  timelineMedia,
}));

import { MessageMedia } from "./MessageMedia";
import type { Media } from "../lib/api";

const PICTURE: Media = {
  source: '{"url":"mxc://example.org/abc"}',
  name: "screenshot.png",
  mime: "image/png",
  size: 94_600,
  width: 800,
  height: 600,
};

const CLIP: Media = {
  source: '{"url":"mxc://example.org/reel"}',
  name: "clip.mp4",
  mime: "video/mp4",
  size: 12_400_000,
  width: 1920,
  height: 1080,
};

/**
 * jsdom implements neither half of the blob URL pair, and both are the whole
 * point of this component: the bytes never become a string.
 */
const made: string[] = [];
const revoked: string[] = [];

beforeEach(() => {
  made.length = 0;
  revoked.length = 0;
  URL.createObjectURL = vi.fn((blob: Blob) => {
    const url = `blob:${blob.type || "none"}/${made.length}`;
    made.push(url);
    return url;
  });
  URL.revokeObjectURL = vi.fn((url: string) => {
    revoked.push(url);
  });
  timelineMedia.mockReset().mockResolvedValue(new ArrayBuffer(8));
});

describe("MessageMedia, for a picture", () => {
  it("draws it as soon as the room is drawn", async () => {
    render(<MessageMedia kind="image" media={PICTURE} />);

    const picture = await screen.findByRole("img", { name: "screenshot.png" });
    expect(picture).toHaveAttribute("src", made[0]);
  });

  it("wraps the bytes in the type the sender named", async () => {
    // The browser sniffs a picture either way, and a clip it often does not,
    // so the type is carried through rather than dropped.
    render(<MessageMedia kind="image" media={PICTURE} />);

    await screen.findByRole("img", { name: "screenshot.png" });
    expect(made[0]).toContain("image/png");
  });

  it("holds the space it will take before the bytes land", async () => {
    // Otherwise every picture that loads shoves the conversation below it
    // downwards, which in a room that follows the bottom is the whole view
    // moving under somebody reading.
    timelineMedia.mockReturnValue(new Promise(() => {}));

    const { container } = render(<MessageMedia kind="image" media={PICTURE} />);

    await waitFor(() => expect(timelineMedia).toHaveBeenCalled());
    expect(container.querySelector(".media__frame")).toHaveStyle({
      aspectRatio: "800 / 600",
    });
  });

  it("says so rather than leaving a gap when it will not load", async () => {
    timelineMedia.mockRejectedValue({
      message: "That attachment is too large for Consort to show.",
    });

    render(<MessageMedia kind="image" media={PICTURE} />);

    expect(
      await screen.findByText("That attachment is too large for Consort to show."),
    ).toBeVisible();
  });

  it("lets go of the bytes when the message leaves the room", async () => {
    const { unmount } = render(<MessageMedia kind="image" media={PICTURE} />);
    await screen.findByRole("img", { name: "screenshot.png" });

    unmount();

    expect(revoked).toEqual([made[0]]);
  });
});

describe("MessageMedia, for a clip", () => {
  it("does not fetch one until somebody asks", async () => {
    // Scrolling back through a room of clips would otherwise be a download of
    // every one of them, and they are the large ones.
    render(<MessageMedia kind="video" media={CLIP} />);

    expect(await screen.findByRole("button")).toBeVisible();
    expect(timelineMedia).not.toHaveBeenCalled();
  });

  it("says what it is called and what it will cost first", async () => {
    render(<MessageMedia kind="video" media={CLIP} />);

    const play = await screen.findByRole("button");
    expect(play).toHaveTextContent("clip.mp4");
    expect(play).toHaveTextContent("12.4 MB");
  });

  it("fetches and plays it once asked", async () => {
    const { container } = render(<MessageMedia kind="video" media={CLIP} />);

    await userEvent.click(await screen.findByRole("button"));

    await waitFor(() => expect(timelineMedia).toHaveBeenCalledWith(CLIP.source));
    await waitFor(() =>
      expect(container.querySelector("video")).toHaveAttribute("src", made[0]),
    );
  });

  it("offers the name alone for a clip nobody measured", async () => {
    // `info` is optional off the wire and a bridge that omits it is common.
    render(
      <MessageMedia kind="video" media={{ source: CLIP.source, name: "clip.mp4" }} />,
    );

    expect(await screen.findByRole("button")).toHaveTextContent("clip.mp4");
  });
});

describe("MessageMedia, for a file", () => {
  it("names a file and what it weighs, and fetches nothing", async () => {
    // Consort has no viewer for a spreadsheet and should not pretend to. What
    // it can honestly offer is the name, the size, and a way to save it.
    render(
      <MessageMedia
        kind="file"
        media={{
          source: '{"url":"mxc://example.org/sheet"}',
          name: "accounts.ods",
          size: 51_200,
        }}
      />,
    );

    expect(await screen.findByText("accounts.ods")).toBeVisible();
    expect(screen.getByText("51 kB")).toBeVisible();
    expect(timelineMedia).not.toHaveBeenCalled();
  });

  it("draws a voice note on the same terms", async () => {
    render(
      <MessageMedia
        kind="audio"
        media={{
          source: '{"url":"mxc://example.org/spoken"}',
          name: "voice-message.ogg",
        }}
      />,
    );

    expect(await screen.findByText("voice-message.ogg")).toBeVisible();
    expect(timelineMedia).not.toHaveBeenCalled();
  });
});
