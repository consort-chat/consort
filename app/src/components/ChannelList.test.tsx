import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ChannelList } from "./ChannelList";
import type { Channel, Space } from "../lib/api";

function text(id: string, name: string | null, joined = true): Channel {
  return { id, name, kind: "text", avatar: null, joined };
}

function voice(id: string, name: string, joined = true): Channel {
  return { id, name, kind: "voice", avatar: null, joined };
}

function space(channels: Channel[], name = "Kahu HQ"): Space {
  return { id: "!s:example.org", name, avatar: null, channels };
}

/** The names in one group, in the order they are drawn. */
function namesIn(label: string): string[] {
  return within(screen.getByRole("region", { name: label }))
    .getAllByRole("button")
    .map((button) => button.textContent ?? "");
}

describe("ChannelList", () => {
  it("names the space at the top", () => {
    render(
      <ChannelList
        space={space([text("!a:example.org", "general")])}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );

    expect(screen.getByText("Kahu HQ")).toBeVisible();
  });

  it("splits text from voice", () => {
    render(
      <ChannelList
        space={space([
          text("!a:example.org", "general"),
          voice("!b:example.org", "Lounge"),
        ])}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );

    expect(namesIn("Text")).toEqual(["#general"]);
    expect(namesIn("Voice")).toEqual(["Lounge"]);
  });

  it("keeps the order it was given inside each group", () => {
    // Filtering preserves the order the snapshot decided; sorting each group
    // separately would not, and the order is MSC1772's rather than ours.
    render(
      <ChannelList
        space={space([
          text("!c:example.org", "zulu"),
          voice("!d:example.org", "Zulu Voice"),
          text("!a:example.org", "alpha"),
          voice("!b:example.org", "Alpha Voice"),
        ])}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );

    expect(namesIn("Text")).toEqual(["#zulu", "#alpha"]);
    expect(namesIn("Voice")).toEqual(["Zulu Voice", "Alpha Voice"]);
  });

  it("omits a group with nothing in it", () => {
    // A "Voice" header over nothing reads as a list that failed to load.
    render(
      <ChannelList
        space={space([text("!a:example.org", "general")])}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole("region", { name: "Voice" }),
    ).not.toBeInTheDocument();
  });

  it("says so when a space has no channels at all", () => {
    render(
      <ChannelList space={space([])} selectedId={null} onSelect={vi.fn()} />,
    );

    expect(screen.getByText(/nothing in here yet/i)).toBeVisible();
  });

  it("marks the selected channel as the current one", () => {
    render(
      <ChannelList
        space={space([
          text("!a:example.org", "general"),
          text("!b:example.org", "random"),
        ])}
        selectedId="!b:example.org"
        onSelect={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "#random" })).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(
      screen.getByRole("button", { name: "#general" }),
    ).not.toHaveAttribute("aria-current");
  });

  it("reports which channel was clicked", async () => {
    const onSelect = vi.fn();
    render(
      <ChannelList
        space={space([voice("!v:example.org", "Lounge")])}
        selectedId={null}
        onSelect={onSelect}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: "Lounge" }));

    expect(onSelect).toHaveBeenCalledWith("!v:example.org");
  });

  it("shows a channel this account never joined, and will not open it", async () => {
    // Hiding it would make Consort disagree with every other client about how
    // many channels the space has. Offering it would open nothing.
    const onSelect = vi.fn();
    render(
      <ChannelList
        space={space([text("!never:example.org", null, false)])}
        selectedId={null}
        onSelect={onSelect}
      />,
    );

    const entry = screen.getByRole("button", { name: /unknown channel/i });
    expect(entry).toBeDisabled();
    expect(entry).toHaveAttribute("title", expect.stringMatching(/not joined/i));

    await userEvent.click(entry);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("never puts a room ID where a name goes", () => {
    // The whole reason `name` is nullable rather than defaulting to the id.
    render(
      <ChannelList
        space={space([text("!never:example.org", null, false)])}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );

    expect(screen.queryByText(/!never:example\.org/)).not.toBeInTheDocument();
  });
});
