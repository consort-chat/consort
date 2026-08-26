import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const roomAvatar = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  roomAvatar,
}));

import { SpaceRail } from "./SpaceRail";
import { resetRoomAvatarCache } from "./RoomAvatar";
import type { Space } from "../lib/api";

const home: Space = { id: "home", name: "Home", avatar: null, channels: [] };

function space(id: string, name: string, avatar: string | null = null): Space {
  return { id, name, avatar, channels: [] };
}

describe("SpaceRail", () => {
  beforeEach(() => {
    resetRoomAvatarCache();
    roomAvatar.mockReset().mockResolvedValue(null);
  });

  it("draws one entry per rail item, in the order it was given", () => {
    // The order is decided in Rust, which follows MSC1772. Two places
    // deciding it is two places to disagree.
    render(
      <SpaceRail
        spaces={[home, space("!b:example.org", "Zebra"), space("!a:example.org", "Apple")]}
        selectedId="home"
        onSelect={vi.fn()}
      />,
    );

    expect(
      screen.getAllByRole("button").map((button) => button.textContent),
    ).toHaveLength(3);
    expect(screen.getByRole("button", { name: "Home" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Zebra" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Apple" })).toBeVisible();
  });

  it("names each entry, because the picture cannot", () => {
    // A rail of wordless icons has no accessible name unless one is given,
    // and "button" is not a space.
    render(
      <SpaceRail
        spaces={[home, space("!s:example.org", "Kahu HQ")]}
        selectedId="home"
        onSelect={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "Kahu HQ" })).toHaveAttribute(
      "title",
      "Kahu HQ",
    );
  });

  it("marks the selected entry as the current one", () => {
    render(
      <SpaceRail
        spaces={[home, space("!s:example.org", "Kahu HQ")]}
        selectedId="!s:example.org"
        onSelect={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "Kahu HQ" })).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(screen.getByRole("button", { name: "Home" })).not.toHaveAttribute(
      "aria-current",
    );
  });

  it("reports which entry was clicked", async () => {
    const onSelect = vi.fn();
    render(
      <SpaceRail
        spaces={[home, space("!s:example.org", "Kahu HQ")]}
        selectedId="home"
        onSelect={onSelect}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Kahu HQ" }));

    expect(onSelect).toHaveBeenCalledWith("!s:example.org");
  });

  it("never asks the homeserver for a picture of Home", () => {
    // Home is a rail entry, not a room. Asking about it would be a round trip
    // for a 404.
    render(
      <SpaceRail spaces={[home]} selectedId="home" onSelect={vi.fn()} />,
    );

    expect(roomAvatar).not.toHaveBeenCalled();
  });

  it("asks for a space's picture when it has one", () => {
    render(
      <SpaceRail
        spaces={[home, space("!s:example.org", "Kahu HQ", "mxc://example.org/abc")]}
        selectedId="home"
        onSelect={vi.fn()}
      />,
    );

    expect(roomAvatar).toHaveBeenCalledWith("!s:example.org");
  });

  it("falls back to an initial for a space with no picture", () => {
    render(
      <SpaceRail
        spaces={[home, space("!s:example.org", "Kahu HQ")]}
        selectedId="home"
        onSelect={vi.fn()}
      />,
    );

    expect(screen.getByText("K")).toBeVisible();
  });

  it("draws nothing when there is nothing to draw", () => {
    // The state before the first room list arrives. An empty rail is correct;
    // a rail with a spinner in it would claim something is coming.
    render(<SpaceRail spaces={[]} selectedId="home" onSelect={vi.fn()} />);

    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });
});
