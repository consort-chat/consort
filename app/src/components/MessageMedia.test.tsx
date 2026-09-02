import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const saveAttachment = vi.hoisted(() => vi.fn());
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
    // moving under somebody reading.
    const { container } = render(<MessageMedia kind="image" media={PICTURE} />);

    expect(container.querySelector(".media__frame")).toHaveStyle({
      aspectRatio: "800 / 600",
    });
  });

  it("leaves the frame to be sized by what arrives when nobody measured it", () => {
    // `info` is optional off the wire. Guessing a ratio would be worse than
    // the jump: a tall picture drawn in a wide box moves twice.
    const { container } = render(
      <MessageMedia
        kind="image"
        media={{ source: PICTURE.source, name: "screenshot.png" }}
      />,
    );

    expect(container.querySelector(".media__frame")).not.toHaveStyle({
      aspectRatio: "800 / 600",
    });
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
