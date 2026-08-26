import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const roomAvatar = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  roomAvatar,
}));

import { RoomAvatar } from "./RoomAvatar";
import { resetAvatarCache } from "../lib/avatars";
import { initialsOf } from "../lib/labels";

const PNG = "data:image/png;base64,iVBORw0KGgo=";

describe("initialsOf", () => {
  it("uses the first letter, upper case", () => {
    expect(initialsOf("general")).toBe("G");
  });

  it("drops a leading sigil rather than drawing it", () => {
    // A room ID, a channel name and a user ID all start with punctuation, and
    // an avatar showing "!" tells nobody anything.
    expect(initialsOf("!room:example.org")).toBe("R");
    expect(initialsOf("#general:example.org")).toBe("G");
    expect(initialsOf("@bob:example.org")).toBe("B");
  });

  it("counts a character rather than a byte", () => {
    // "…" and the like are one character and several bytes. Slicing bytes
    // would produce half a character and render as a replacement glyph.
    expect(initialsOf("étoile")).toBe("É");
    expect(initialsOf("🛰 telemetry")).toBe("🛰");
  });

  it("falls back to a question mark when there is nothing to use", () => {
    expect(initialsOf("")).toBe("?");
    expect(initialsOf("   ")).toBe("?");
    expect(initialsOf("!")).toBe("?");
  });
});

describe("RoomAvatar", () => {
  beforeEach(() => {
    resetAvatarCache();
    roomAvatar.mockReset().mockResolvedValue(PNG);
  });

  it("draws the initial and asks nothing when the room has no avatar", async () => {
    // Four rooms in ten have none. A round trip that was always going to
    // answer nothing costs a flicker on every one of them.
    render(<RoomAvatar roomId="!a:example.org" name="general" avatar={null} />);

    expect(screen.getByText("G")).toBeVisible();
    await waitFor(() => expect(roomAvatar).not.toHaveBeenCalled());
  });

  it("draws the image once it arrives", async () => {
    render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );

    const image = await screen.findByRole("presentation");
    expect(image).toHaveAttribute("src", PNG);
    expect(roomAvatar).toHaveBeenCalledWith("!a:example.org");
  });

  it("shows the initial while the image is still coming", () => {
    roomAvatar.mockReturnValue(new Promise(() => {}));

    render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );

    expect(screen.getByText("G")).toBeVisible();
  });

  it("keeps the initial when the homeserver had no image after all", async () => {
    roomAvatar.mockResolvedValue(null);

    render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );

    await waitFor(() => expect(roomAvatar).toHaveBeenCalled());
    expect(screen.getByText("G")).toBeVisible();
  });

  it("keeps the initial and logs when the fetch fails", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    roomAvatar.mockRejectedValue({ message: "no", detail: "no" });

    render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );

    await waitFor(() => expect(logged).toHaveBeenCalled());
    expect(screen.getByText("G")).toBeVisible();
  });

  it("asks once for a room drawn twice", async () => {
    // The rail and the channel list mount together and can want the same
    // space. Two round trips for one picture is one too many.
    render(
      <>
        <RoomAvatar
          roomId="!a:example.org"
          name="general"
          avatar="mxc://example.org/abc"
        />
        <RoomAvatar
          roomId="!a:example.org"
          name="general"
          avatar="mxc://example.org/abc"
        />
      </>,
    );

    await waitFor(() =>
      expect(screen.getAllByRole("presentation")).toHaveLength(2),
    );
    expect(roomAvatar).toHaveBeenCalledTimes(1);
  });

  it("does not ask again for a room it has already drawn", async () => {
    const first = render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );
    await screen.findByRole("presentation");
    first.unmount();

    render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );

    expect(await screen.findByRole("presentation")).toHaveAttribute("src", PNG);
    expect(roomAvatar).toHaveBeenCalledTimes(1);
  });

  it("remembers that a room had nothing, and does not ask again", async () => {
    roomAvatar.mockResolvedValue(null);
    const first = render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );
    await waitFor(() => expect(roomAvatar).toHaveBeenCalledTimes(1));
    first.unmount();

    render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );

    await waitFor(() => expect(screen.getByText("G")).toBeVisible());
    expect(roomAvatar).toHaveBeenCalledTimes(1);
  });

  it("does not set state after unmounting", async () => {
    let resolve: (url: string | null) => void = () => {};
    roomAvatar.mockReturnValue(
      new Promise<string | null>((r) => {
        resolve = r;
      }),
    );
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});

    const { unmount } = render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );
    unmount();
    resolve(PNG);

    await waitFor(() => expect(roomAvatar).toHaveBeenCalled());
    // React logs an act warning through console.error if a setState lands
    // after unmount, so silence is the assertion.
    expect(logged).not.toHaveBeenCalled();
  });

  it("swaps back to the initial when the room loses its avatar", async () => {
    const { rerender } = render(
      <RoomAvatar
        roomId="!a:example.org"
        name="general"
        avatar="mxc://example.org/abc"
      />,
    );
    await screen.findByRole("presentation");

    rerender(
      <RoomAvatar roomId="!a:example.org" name="general" avatar={null} />,
    );

    expect(screen.getByText("G")).toBeVisible();
    expect(screen.queryByRole("presentation")).not.toBeInTheDocument();
  });
});
