import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

// A face draws an avatar, which is a command. Mocked rather than left to fail
// quietly, because an unmocked `invoke` throws into the catch that turns a
// missing picture into an initial: the test would pass having exercised the
// wrong path.
const memberAvatar = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberAvatar,
}));

import { CallFace, type CallFaceProps } from "./CallFace";
import type { Participant } from "../lib/api";
import { resetAvatarCache } from "../lib/avatars";

const LOUNGE = "!lounge:example.org";
const PNG = "data:image/png;base64,iVBORw0KGgo=";

function person(id: string, name: string): Participant {
  return { id, name, muted: false };
}

/** One face in a list, because that is where both callers put it. */
function draw(overrides: Partial<CallFaceProps> = {}) {
  const onOpen = vi.fn();
  render(
    <ul aria-label="In Lounge">
      <CallFace
        person={person("@ada:example.org", "Ada")}
        roomId={LOUNGE}
        speaking={false}
        live={false}
        onOpen={onOpen}
        {...overrides}
      />
    </ul>,
  );
  return onOpen;
}

beforeEach(() => {
  resetAvatarCache();
  memberAvatar.mockResolvedValue(null);
});

describe("CallFace", () => {
  it("draws an initial when the person has no picture", async () => {
    draw();

    expect(screen.getByRole("listitem")).toHaveTextContent("AAda");
    await waitFor(() => {
      expect(memberAvatar).toHaveBeenCalledWith(LOUNGE, "@ada:example.org");
    });
    expect(document.querySelector(".avatar__image")).toBeNull();
  });

  it("draws the picture when the person has one", async () => {
    memberAvatar.mockResolvedValue(PNG);
    draw();

    await waitFor(() => {
      expect(document.querySelector(".avatar__image")).toHaveAttribute(
        "src",
        PNG,
      );
    });
  });

  it("marks somebody who is talking", () => {
    draw({ speaking: true });

    expect(screen.getByRole("listitem")).toHaveAttribute(
      "data-speaking",
      "true",
    );
  });

  it("does not mark somebody who is not talking", () => {
    draw();

    expect(screen.getByRole("listitem")).toHaveAttribute(
      "data-speaking",
      "false",
    );
  });

  it("marks somebody who has muted themselves", () => {
    draw({ person: { ...person("@ada:example.org", "Ada"), muted: true } });

    expect(screen.getByLabelText("Ada is muted")).toBeVisible();
    expect(screen.getByRole("listitem")).toHaveAttribute("data-muted", "true");
  });

  it("draws headphones rather than a microphone for somebody deafened", () => {
    // One icon, never two. Deafening mutes, so both are true of the same
    // person and the stronger claim is the one worth the width.
    draw({
      person: {
        ...person("@ada:example.org", "Ada"),
        muted: true,
        deafened: true,
      },
    });

    expect(screen.getByLabelText("Ada is deafened")).toBeVisible();
    expect(screen.queryByLabelText("Ada is muted")).toBeNull();
  });

  it("draws a clock for somebody who is away", () => {
    draw({
      person: { ...person("@ada:example.org", "Ada"), muted: true, away: true },
    });

    expect(screen.getByLabelText("Ada is away")).toBeVisible();
    expect(screen.queryByLabelText("Ada is muted")).toBeNull();
  });

  it("says nothing about a camera unless the roster is live", () => {
    // Room state carries nothing about cameras, so a cross drawn from it would
    // be an invention rather than a reading.
    draw();

    expect(screen.queryByLabelText(/camera/)).toBeNull();
  });

  it("draws a crossed-out camera for a live roster", () => {
    draw({ live: true });

    expect(screen.getByLabelText("Ada has their camera off")).toBeVisible();
  });

  it("draws an uncrossed camera for somebody who has theirs on", () => {
    draw({
      live: true,
      person: { ...person("@ada:example.org", "Ada"), camera: true },
    });

    expect(screen.getByLabelText("Ada has their camera on")).toBeVisible();
    expect(screen.queryByLabelText("Ada has their camera off")).toBeNull();
  });

  it("asks for the person's card when clicked", async () => {
    const onOpen = draw();

    await userEvent.click(screen.getByRole("button"));

    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(onOpen).toHaveBeenCalledWith({
      x: expect.any(Number),
      y: expect.any(Number),
    });
  });

  it("asks for the person's card on a right-click too", async () => {
    // Where anybody looks for a card about a person, and the left button is
    // what a touchpad without a second one has. Both open the same thing:
    // a person is one subject.
    const onOpen = draw();

    await userEvent.pointer({
      keys: "[MouseRight]",
      target: screen.getByRole("button"),
    });

    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("stacks the name under the face when asked for a tile", () => {
    // The card wants a grid of faces rather than a column of rows. One
    // component either way, because #69 is about to put video in both.
    draw({ layout: "tile" });

    expect(screen.getByRole("listitem")).toHaveAttribute("data-layout", "tile");
  });
});
