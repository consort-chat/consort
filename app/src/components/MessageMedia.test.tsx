import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const saveAttachment = vi.hoisted(() => vi.fn());
const canPlay = vi.hoisted(() => vi.fn());
vi.mock("../lib/playable", () => ({ canPlay }));
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  saveAttachment,
}));

import { MessageMedia } from "./MessageMedia";
import { mediaUrl, type Media } from "../lib/api";

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

const SHEET: Media = {
  source: '{"url":"mxc://example.org/sheet"}',
  name: "accounts.ods",
  size: 51_200,
};

beforeEach(() => {
  saveAttachment.mockReset().mockResolvedValue("/home/ada/accounts.ods");
  // The real one reads WebKitGTK's GStreamer registry, which is the whole
  // point of it and exactly what jsdom cannot have.
  canPlay.mockReset().mockReturnValue("yes");
  // jsdom implements none of the media element's methods, and the control bar
  // drives all of them.
  HTMLMediaElement.prototype.play = vi.fn().mockResolvedValue(undefined);
  HTMLMediaElement.prototype.pause = vi.fn();
});

describe("MessageMedia, for a picture", () => {
  it("draws it as soon as the room is drawn", () => {
    render(<MessageMedia kind="image" media={PICTURE} />);

    expect(screen.getByRole("img", { name: "screenshot.png" })).toHaveAttribute(
      "src",
      mediaUrl(PICTURE.source),
    );
  });

  it("holds the space it will take before the bytes land", () => {
    // Otherwise every picture that loads shoves the conversation below it
    // downwards, which in a room that follows the bottom is the whole view
    // moving under somebody reading. On the picture rather than on the frame,
    // so the browser derives the ratio the way it does for every other image
    // on the web.
    render(<MessageMedia kind="image" media={PICTURE} />);

    const picture = screen.getByRole("img", { name: "screenshot.png" });
    expect(picture).toHaveAttribute("width", "800");
    expect(picture).toHaveAttribute("height", "600");
  });

  it("sizes the picture from the picture rather than from its frame", () => {
    // The frame is a button, and a button is sized by its contents. A frame
    // that took its shape from an `aspect-ratio` while its contents took
    // their size from a percentage of the frame gave each layout pass a
    // slightly larger answer than the last, and the picture crept outwards
    // for as long as the room was open.
    const { container } = render(<MessageMedia kind="image" media={PICTURE} />);

    expect(container.querySelector(".media__frame")).not.toHaveAttribute(
      "style",
    );
  });

  it("opens it full size when it is pressed", async () => {
    // In a room capped at 480 by 340, a screenshot of anything with words in
    // it cannot be read until it is opened.
    render(<MessageMedia kind="image" media={PICTURE} />);

    await userEvent.click(
      screen.getByRole("button", { name: /open screenshot\.png/i }),
    );

    expect(screen.getByRole("dialog", { name: "screenshot.png" })).toBeVisible();
  });

  it("leaves the shape to what arrives when nobody measured it", () => {
    // `info` is optional off the wire. Guessing a ratio would be worse than
    // the jump: a tall picture drawn in a wide box moves twice.
    render(
      <MessageMedia
        kind="image"
        media={{ source: PICTURE.source, name: "screenshot.png" }}
      />,
    );

    const picture = screen.getByRole("img", { name: "screenshot.png" });
    expect(picture).not.toHaveAttribute("width");
    expect(picture).not.toHaveAttribute("height");
  });
});

describe("MessageMedia, for a clip", () => {
  it("draws no player until somebody asks", () => {
    // Scrolling back through a room of clips would otherwise start a download
    // of every one of them, and they are the large ones.
    const { container } = render(<MessageMedia kind="video" media={CLIP} />);

    expect(screen.getByRole("button")).toBeVisible();
    expect(container.querySelector("video")).toBeNull();
  });

  it("says what it is called and what it will cost first", () => {
    render(<MessageMedia kind="video" media={CLIP} />);

    const play = screen.getByRole("button");
    expect(play).toHaveTextContent("clip.mp4");
    expect(play).toHaveTextContent("12.4 MB");
  });

  it("draws the still the sender sent with it", () => {
    // A black rectangle and a filename says almost nothing about what is in a
    // clip, and the still is a few kilobytes against tens of megabytes.
    const { container } = render(
      <MessageMedia
        kind="video"
        media={{ ...CLIP, thumbnail: '{"url":"mxc://example.org/still"}' }}
      />,
    );

    expect(container.querySelector(".media__poster")).toHaveAttribute(
      "src",
      mediaUrl('{"url":"mxc://example.org/still"}'),
    );
  });

  it("draws an empty frame for a clip nobody sent a still with", () => {
    // Inventing one would mean fetching the clip to make it, which is the
    // download the card exists to postpone.
    const { container } = render(<MessageMedia kind="video" media={CLIP} />);

    expect(container.querySelector(".media__poster")).toBeNull();
    expect(screen.getByRole("button", { name: /play clip\.mp4/i })).toBeVisible();
  });

  it("points the player at the attachment once asked", async () => {
    const { container } = render(<MessageMedia kind="video" media={CLIP} />);

    await userEvent.click(screen.getByRole("button"));

    expect(container.querySelector("video")).toHaveAttribute(
      "src",
      mediaUrl(CLIP.source),
    );
  });

  it("offers the name alone for a clip nobody measured", () => {
    render(
      <MessageMedia
        kind="video"
        media={{ source: CLIP.source, name: "clip.mp4" }}
      />,
    );

    expect(screen.getByRole("button")).toHaveTextContent("clip.mp4");
  });

  it("draws its own controls rather than the platform's", async () => {
    // What WebKitGTK draws for `controls` is its own shadow DOM: a fullscreen
    // button in the top left of the picture, a speaker in the top right, and
    // a bar that says "Error". It looks nothing like a browser's because the
    // browser here is not the one anybody has seen.
    const { container } = render(<MessageMedia kind="video" media={CLIP} />);

    await userEvent.click(screen.getByRole("button"));

    expect(container.querySelector("video")).not.toHaveAttribute("controls");
    expect(screen.getByRole("slider", { name: /position in clip/i })).toBeVisible();
    expect(screen.getByRole("button", { name: /play clip\.mp4/i })).toBeVisible();
  });

  it("offers a save card instead of a player this machine cannot use", () => {
    // A player that cannot play is worse than no player. Asked before the
    // element is drawn, so a room of clips on a machine with no H.264 decoder
    // says so rather than showing a garbled picture with "Error" in a corner.
    canPlay.mockReturnValue("no");

    const { container } = render(<MessageMedia kind="video" media={CLIP} />);

    expect(screen.getByText(/no decoder for this clip/i)).toBeVisible();
    expect(screen.getByText(/gst-libav/)).toBeVisible();
    expect(container.querySelector("video")).toBeNull();
  });

  it("saves a clip it cannot play, which is the thing that does work", async () => {
    canPlay.mockReturnValue("no");
    saveAttachment.mockResolvedValue("/home/ada/clip.mp4");
    render(<MessageMedia kind="video" media={CLIP} />);

    await userEvent.click(screen.getByRole("button"));

    await waitFor(() =>
      expect(saveAttachment).toHaveBeenCalledWith(CLIP.source, "clip.mp4"),
    );
  });

  it("gives the player its go when nobody can say whether it will work", async () => {
    // A clip whose sender named no type, or a container with no representative
    // codecs. Refusing on a shrug would refuse clips that play.
    canPlay.mockReturnValue("unknown");
    const { container } = render(<MessageMedia kind="video" media={CLIP} />);

    await userEvent.click(screen.getByRole("button"));

    expect(container.querySelector("video")).not.toBeNull();
  });

  it("falls back to the save card when the player errors anyway", async () => {
    // The probe guesses at what is inside a container, so it can be wrong.
    // This is the other half, and it costs nothing to have both.
    const { container } = render(<MessageMedia kind="video" media={CLIP} />);
    await userEvent.click(screen.getByRole("button"));

    const video = container.querySelector("video");
    expect(video).not.toBeNull();
    fireEvent.error(video as HTMLVideoElement);

    expect(await screen.findByText(/no decoder for this clip/i)).toBeVisible();
  });
});

describe("MessageMedia, for a file", () => {
  it("names a file and what it weighs, and points nothing at it", () => {
    // Consort has no viewer for a spreadsheet and should not pretend to. What
    // it can honestly offer is the name, the size, and a way to save it.
    const { container } = render(<MessageMedia kind="file" media={SHEET} />);

    expect(screen.getByRole("button")).toHaveTextContent("accounts.ods");
    expect(screen.getByRole("button")).toHaveTextContent("51 kB");
    expect(container.querySelector("img, video")).toBeNull();
  });

  it("draws a voice note on the same terms", () => {
    render(
      <MessageMedia
        kind="audio"
        media={{
          source: '{"url":"mxc://example.org/spoken"}',
          name: "voice-message.ogg",
        }}
      />,
    );

    expect(screen.getByRole("button")).toHaveTextContent("voice-message.ogg");
  });

  it("opens the save window with the file's own name in it", async () => {
    render(<MessageMedia kind="file" media={SHEET} />);

    await userEvent.click(screen.getByRole("button"));

    await waitFor(() =>
      expect(saveAttachment).toHaveBeenCalledWith(SHEET.source, "accounts.ods"),
    );
  });

  it("says where it went", async () => {
    render(<MessageMedia kind="file" media={SHEET} />);

    await userEvent.click(screen.getByRole("button"));

    expect(await screen.findByText(/\/home\/ada\/accounts\.ods/)).toBeVisible();
  });

  it("says nothing at all when the window was closed without choosing", async () => {
    // Not a failure, and drawing one would be telling somebody off for
    // changing their mind.
    saveAttachment.mockResolvedValue(null);
    render(<MessageMedia kind="file" media={SHEET} />);

    await userEvent.click(screen.getByRole("button"));

    await waitFor(() => expect(saveAttachment).toHaveBeenCalled());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says why when it cannot be saved", async () => {
    saveAttachment.mockRejectedValue({
      message: "Consort could not write that file.",
      detail: "permission denied",
    });
    render(<MessageMedia kind="file" media={SHEET} />);

    await userEvent.click(screen.getByRole("button"));

    expect(
      await screen.findByText("Consort could not write that file."),
    ).toBeVisible();
  });
});
