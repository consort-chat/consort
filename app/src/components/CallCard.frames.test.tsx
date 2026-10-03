import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * How often each face was drawn, and the handle on the newest frame.
 *
 * A file of its own because both stand-ins below would break the rest of the
 * card's tests: one replaces every face with a counter, and the other replaces
 * the poll with a setter a test can pull.
 */
const drawn = vi.hoisted(() => ({ faces: 0 }));
const frame = vi.hoisted(() => ({
  show: undefined as undefined | ((url: string | null) => void),
}));

vi.mock("./CallFace", () => ({
  CallFace: ({
    person,
    picture,
  }: {
    person: { name: string };
    picture?: unknown;
  }) => {
    drawn.faces += 1;
    return (
      <li>
        {person.name}
        {picture as never}
      </li>
    );
  },
}));

vi.mock("../lib/usePicture", async () => {
  const { useState } = await import("react");
  return {
    usePicture: () => {
      const [url, setUrl] = useState<string | null>(null);
      frame.show = setUrl;
      return url;
    },
  };
});

import { CallCard } from "./CallCard";
import type { Call } from "../lib/api";

const LOUNGE = "!lounge:example.org";
const PICTURE = "data:image/jpeg;base64,aaaa";

const CALL: Call = {
  state: "connected",
  roomId: LOUNGE,
  trouble: null,
  participants: [
    { id: "@bob:example.org", name: "Bob", muted: false },
    { id: "@ann:example.org", name: "Ann", muted: false },
    { id: "@cyd:example.org", name: "Cyd", muted: false },
  ],
};

beforeEach(() => {
  drawn.faces = 0;
  frame.show = undefined;
});

describe("what a frame arriving redraws", () => {
  it("redraws the picture and not the faces beside it", async () => {
    // #142's trap, which this card makes worse: the picture changes twelve and
    // a half times a second, and held one level up that is every face in the
    // call redrawn twelve and a half times a second. The poll has to live in
    // the leaf that draws it.
    render(
      <CallCard
        call={CALL}
        channelName="Lounge"
        selfId="@bob:example.org"
        shown
        onHide={vi.fn()}
        onOpenRoom={vi.fn()}
        cameraOn
      />,
    );
    await waitFor(() => expect(frame.show).toBeDefined());
    const before = drawn.faces;

    act(() => frame.show?.(PICTURE));

    expect(screen.getByRole("img", { name: "Your camera" })).toBeVisible();
    expect(drawn.faces).toBe(before);
  });
});
