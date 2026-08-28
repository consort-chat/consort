import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { CallRefusedNotice } from "./CallRefusedNotice";
import type { CallRefused } from "../lib/api";

const LOUNGE = "!lounge:example.org";

function refusal(
  readiness: CallRefused["readiness"]["state"],
): CallRefused {
  return { roomId: LOUNGE, readiness: { state: readiness } as never };
}

describe("CallRefusedNotice", () => {
  it("names the channel that was not joined", () => {
    // The click that did nothing. Without a name somebody who tried two
    // channels in a row cannot tell which one this is about.
    render(
      <CallRefusedNotice
        refusal={refusal("sessionUnverified")}
        channelName="Lounge"
        onDismiss={vi.fn()}
      />,
    );

    expect(screen.getByText(/Lounge was not joined/)).toBeVisible();
  });

  it("says something rather than nothing when the room list has no name", () => {
    // A channel clicked before the room list arrived, or one this account has
    // since left. "undefined was not joined" would be worse than vague.
    render(
      <CallRefusedNotice
        refusal={refusal("sessionUnverified")}
        channelName={null}
        onDismiss={vi.fn()}
      />,
    );

    expect(screen.getByText(/That voice channel was not joined/)).toBeVisible();
  });

  it("leads with nobody being able to hear you", () => {
    // The fact somebody needs. That it is about cross-signing is the
    // explanation and comes second.
    render(
      <CallRefusedNotice
        refusal={refusal("sessionUnverified")}
        channelName="Lounge"
        onDismiss={vi.fn()}
      />,
    );

    expect(screen.getByText(/Nobody in the call would have been able to hear you/)).toBeVisible();
  });

  it("sends an unverified session to verify this device", () => {
    render(
      <CallRefusedNotice
        refusal={refusal("sessionUnverified")}
        channelName="Lounge"
        onDismiss={vi.fn()}
      />,
    );

    expect(screen.getByText(/this session is not verified/)).toBeVisible();
    expect(screen.queryByText(/set up recovery/i)).toBeNull();
  });

  it("sends an account with no identity somewhere else entirely", () => {
    // The two are cleared in two different places, which is why
    // `CallReadiness` keeps them apart. Telling somebody who has already set
    // up cross-signing to go and do it again is the failure this prevents.
    render(
      <CallRefusedNotice
        refusal={refusal("noIdentity")}
        channelName="Lounge"
        onDismiss={vi.fn()}
      />,
    );

    expect(screen.getByText(/no encryption identity set up/)).toBeVisible();
    expect(screen.getByText(/any other client you are signed in to/)).toBeVisible();
  });

  it("is an alert, because nothing else on screen says the click failed", () => {
    render(
      <CallRefusedNotice
        refusal={refusal("sessionUnverified")}
        channelName="Lounge"
        onDismiss={vi.fn()}
      />,
    );

    expect(screen.getByRole("alert")).toBeVisible();
  });

  it("can be dismissed", async () => {
    const onDismiss = vi.fn();
    render(
      <CallRefusedNotice
        refusal={refusal("sessionUnverified")}
        channelName="Lounge"
        onDismiss={onDismiss}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: /dismiss/i }));

    expect(onDismiss).toHaveBeenCalled();
  });
});
