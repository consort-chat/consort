import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const saveAttachment = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  saveAttachment,
}));

import { ImageViewer } from "./ImageViewer";
import { mediaUrl, type Media } from "../lib/api";

const PICTURE: Media = {
  source: '{"url":"mxc://example.org/abc"}',
  name: "screenshot.png",
  mime: "image/png",
  size: 94_600,
  width: 800,
  height: 600,
};

function open(media: Media = PICTURE) {
  const onClose = vi.fn();
  const { container } = render(<ImageViewer media={media} onClose={onClose} />);
  return Object.assign(onClose, { container });
}

beforeEach(() => {
  saveAttachment.mockReset().mockResolvedValue("/home/ada/screenshot.png");
});

describe("ImageViewer", () => {
  it("draws the picture it was given", () => {
    open();

    expect(screen.getByRole("img", { name: "screenshot.png" })).toHaveAttribute(
      "src",
      mediaUrl(PICTURE.source),
    );
  });

  it("closes on Escape", () => {
    // At the document rather than on the element. Most of what is in here
    // cannot take focus, so after the first click on the picture a handler
    // bound to the element would never see the key.
    const onClose = open();

    fireEvent.keyDown(document, { key: "Escape" });

    expect(onClose).toHaveBeenCalled();
  });

  it("closes on the cross", async () => {
    const onClose = open();

    await userEvent.click(screen.getByRole("button", { name: "Close" }));

    expect(onClose).toHaveBeenCalled();
  });

  it("closes on a press outside the picture", () => {
    const onClose = open();

    fireEvent.mouseDown(screen.getByRole("dialog"));

    expect(onClose).toHaveBeenCalled();
  });

  it("stays open when the press was on the picture", () => {
    // A drag that starts on the picture and finishes outside it counts as a
    // click on the backdrop, and closing on that throws away what was being
    // done.
    const onClose = open();

    fireEvent.mouseDown(screen.getByRole("img", { name: "screenshot.png" }));

    expect(onClose).not.toHaveBeenCalled();
  });

  it("says nothing about the picture until it is asked", async () => {
    open();

    expect(screen.queryByText("800 by 600 pixels")).toBeNull();

    await userEvent.click(
      screen.getByRole("button", { name: /about this picture/i }),
    );

    expect(screen.getByText("800 by 600 pixels")).toBeVisible();
    expect(screen.getByText("95 kB")).toBeVisible();
    expect(screen.getByText("image/png")).toBeVisible();
  });

  it("leaves out what nobody said about it", async () => {
    // `info` is optional off the wire, and a row reading "undefined" is worse
    // than no row.
    open({ source: PICTURE.source, name: "screenshot.png" });

    await userEvent.click(
      screen.getByRole("button", { name: /about this picture/i }),
    );

    expect(screen.getByText("screenshot.png")).toBeVisible();
    expect(screen.queryByText(/pixels/)).toBeNull();
  });

  it("saves the picture and says where it went", async () => {
    open();

    await userEvent.click(
      screen.getByRole("button", { name: /save screenshot\.png/i }),
    );

    await waitFor(() =>
      expect(saveAttachment).toHaveBeenCalledWith(
        PICTURE.source,
        "screenshot.png",
      ),
    );
    expect(
      await screen.findByText(/\/home\/ada\/screenshot\.png/),
    ).toBeVisible();
  });

  it("says nothing when the save window was closed without choosing", async () => {
    saveAttachment.mockResolvedValue(null);
    open();

    await userEvent.click(
      screen.getByRole("button", { name: /save screenshot\.png/i }),
    );

    await waitFor(() => expect(saveAttachment).toHaveBeenCalled());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("says why when it cannot be saved", async () => {
    saveAttachment.mockRejectedValue({
      message: "Consort could not write that file.",
      detail: "permission denied",
    });
    open();

    await userEvent.click(
      screen.getByRole("button", { name: /save screenshot\.png/i }),
    );

    expect(
      await screen.findByText("Consort could not write that file."),
    ).toBeVisible();
  });

  it("gives focus back to whatever opened it", async () => {
    // Otherwise closing leaves focus on `body` and the next Tab starts from
    // the top of the room rather than from the picture that was just shut.
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();

    const { unmount } = render(
      <ImageViewer media={PICTURE} onClose={vi.fn()} />,
    );
    unmount();

    await waitFor(() => expect(document.activeElement).toBe(opener));
    opener.remove();
  });
});
