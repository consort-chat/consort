import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const memberProfile = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberProfile,
}));

import { PresenceDot } from "./PresenceDot";
import { resetPresenceCache } from "../lib/presence";
import type { MemberProfile, Presence } from "../lib/api";

const ADA = "@ada:example.org";

function profile(presence: Presence): MemberProfile {
  return { presence, status: null, lastActiveAgo: null };
}

beforeEach(() => {
  resetPresenceCache();
  memberProfile.mockReset().mockResolvedValue(profile("online"));
});

describe("PresenceDot", () => {
  it("says where somebody is once the homeserver has answered", async () => {
    render(<PresenceDot userId={ADA} />);

    expect(await screen.findByRole("img", { name: "Online" })).toBeVisible();
  });

  it("carries the state as a tooltip, which is how a dot is read at all", async () => {
    // A coloured circle means nothing on its own. The pointer is where the
    // word lives, because a label beside every avatar would be a second name
    // down the whole room.
    render(<PresenceDot userId={ADA} />);

    expect(await screen.findByTitle("Online")).toBeVisible();
  });

  it("colours itself by the state rather than by a class per case", async () => {
    memberProfile.mockResolvedValue(profile("idle"));

    render(<PresenceDot userId={ADA} />);

    expect(await screen.findByTitle("Idle")).toHaveAttribute(
      "data-presence",
      "idle",
    );
  });

  it("draws nothing at all when the homeserver will not say", async () => {
    // The ordinary case. Presence is off by default on Synapse and stays off
    // on most homeservers of any size, and a grey dot on somebody sitting
    // right there is worse than no dot.
    memberProfile.mockResolvedValue(profile("unknown"));

    const { container } = render(<PresenceDot userId={ADA} />);

    await waitFor(() => expect(memberProfile).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("draws nothing before the answer arrives", async () => {
    // Not a dot that changes colour a moment later. Somebody scanning a room
    // would read the first colour, and it was never a claim about anybody.
    memberProfile.mockReturnValue(new Promise(() => {}));

    const { container } = render(<PresenceDot userId={ADA} />);

    expect(container).toBeEmptyDOMElement();
  });

  it("asks once for somebody who said six things", async () => {
    // Every message in a burst draws one of these, and one request per
    // message would be a burst of requests for one answer.
    render(
      <>
        <PresenceDot userId={ADA} />
        <PresenceDot userId={ADA} />
        <PresenceDot userId={ADA} />
      </>,
    );

    await waitFor(() => expect(screen.getAllByTitle("Online")).toHaveLength(3));
    expect(memberProfile).toHaveBeenCalledTimes(1);
  });

  it("says nothing rather than failing when the ask does", async () => {
    memberProfile.mockRejectedValue(new Error("no"));

    const { container } = render(<PresenceDot userId={ADA} />);

    await waitFor(() => expect(memberProfile).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });
});
