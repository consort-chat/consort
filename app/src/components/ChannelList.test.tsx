import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

// The people under a voice channel draw their avatars, which is a command.
// Mocked here rather than left to fail quietly, because an unmocked `invoke`
// throws into the catch that turns a missing picture into an initial, and the
// tests would still pass while exercising the wrong path.
const memberAvatar = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberAvatar,
}));

import { ChannelList } from "./ChannelList";
import type { Call, Channel, Participant, Space } from "../lib/api";
import { resetAvatarCache } from "../lib/avatars";

function text(id: string, name: string | null, joined = true): Channel {
  return { id, name, kind: "text", avatar: null, joined, participants: [] };
}

function voice(
  id: string,
  name: string,
  participants: Participant[] = [],
): Channel {
  return {
    id,
    name,
    kind: "voice",
    avatar: null,
    joined: true,
    participants,
  };
}

function person(id: string, name: string, muted = false): Participant {
  return { id, name, muted };
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

const PNG = "data:image/png;base64,iVBORw0KGgo=";

/** No call, which is what almost every test here is about. */
const IDLE: Call = { state: "disconnected" };

describe("ChannelList", () => {
  beforeEach(() => {
    resetAvatarCache();
    memberAvatar.mockReset().mockResolvedValue(null);
  });

  it("names the space at the top", () => {
    render(
      <ChannelList
        space={space([text("!a:example.org", "general")])}
        selectedId={null}
        call={IDLE}
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
        call={IDLE}
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
        call={IDLE}
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
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole("region", { name: "Voice" }),
    ).not.toBeInTheDocument();
  });

  it("says so when a space has no channels at all", () => {
    render(
      <ChannelList
        space={space([])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
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
        call={IDLE}
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
        call={IDLE}
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
        call={IDLE}
        onSelect={onSelect}
      />,
    );

    const entry = screen.getByRole("button", { name: /unknown channel/i });
    expect(entry).toBeDisabled();
    expect(entry).toHaveAttribute("title", expect.stringMatching(/not joined/i));

    await userEvent.click(entry);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("shows who is in a voice channel without anybody opening it", () => {
    // The whole point of this half of the feature: presence is a read of room
    // state, so it is on screen before anything is clicked or connected to.
    render(
      <ChannelList
        space={space([
          voice("!v:example.org", "Lounge", [
            person("@ada:example.org", "Ada"),
            person("@ben:example.org", "Ben"),
          ]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    const people = within(screen.getByRole("list", { name: "In Lounge" }));
    expect(
      people.getAllByRole("listitem").map((item) => item.textContent),
    ).toEqual(["AAda", "BBen"]);
  });

  it("draws people in the order it was given", () => {
    // Oldest membership first, decided in Rust. Re-sorting here would make the
    // list move under the pointer every time somebody joined.
    render(
      <ChannelList
        space={space([
          voice("!v:example.org", "Lounge", [
            person("@zoe:example.org", "Zoe"),
            person("@ada:example.org", "Ada"),
          ]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    const people = within(screen.getByRole("list", { name: "In Lounge" }));
    expect(
      people.getAllByRole("listitem").map((item) => item.textContent),
    ).toEqual(["ZZoe", "AAda"]);
  });

  it("draws nothing under a voice channel nobody is in", () => {
    // An empty voice channel has to keep exactly the shape it had before
    // presence existed, or every quiet channel gains a gap under it.
    render(
      <ChannelList
        space={space([voice("!v:example.org", "Lounge")])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole("list", { name: "In Lounge" }),
    ).not.toBeInTheDocument();
  });

  it("does not turn a person into a way to open the channel", async () => {
    // Nesting the list inside the button would make every name a target that
    // opens the room, which is not what clicking somebody should ever mean.
    const onSelect = vi.fn();
    render(
      <ChannelList
        space={space([
          voice("!v:example.org", "Lounge", [person("@ada:example.org", "Ada")]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={onSelect}
      />,
    );

    await userEvent.click(screen.getByText("Ada"));

    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Lounge" })).toBeVisible();
  });

  it("asks for a person's picture in the room they are in", async () => {
    // A Matrix profile is per room, so the room is half of the question. The
    // answer replaces the initial in place.
    memberAvatar.mockResolvedValue(PNG);
    render(
      <ChannelList
        space={space([
          voice("!v:example.org", "Lounge", [person("@ada:example.org", "Ada")]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(memberAvatar).toHaveBeenCalledWith(
        "!v:example.org",
        "@ada:example.org",
      );
    });
    await waitFor(() => {
      expect(document.querySelector(".avatar__image")).toHaveAttribute(
        "src",
        PNG,
      );
    });
  });

  it("falls back to an initial for somebody with no picture", async () => {
    render(
      <ChannelList
        space={space([
          voice("!v:example.org", "Lounge", [person("@ada:example.org", "Ada")]),
        ])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    await waitFor(() => expect(memberAvatar).toHaveBeenCalled());
    expect(document.querySelector(".avatar__image")).toBeNull();
    expect(screen.getByText("A")).toBeVisible();
  });

  it("never puts a room ID where a name goes", () => {
    // The whole reason `name` is nullable rather than defaulting to the id.
    render(
      <ChannelList
        space={space([text("!never:example.org", null, false)])}
        selectedId={null}
        call={IDLE}
        onSelect={vi.fn()}
      />,
    );

    expect(screen.queryByText(/!never:example\.org/)).not.toBeInTheDocument();
  });

  describe("the call this session is in", () => {
    const LOUNGE = "!lounge:example.org";

    function withLounge() {
      return space([
        text("!a:example.org", "general"),
        voice(LOUNGE, "Lounge"),
        voice("!b:example.org", "Music"),
      ]);
    }

    function entryFor(name: string | RegExp): HTMLElement {
      return screen.getByRole("button", { name });
    }

    it("marks the channel it is connected to", () => {
      render(
        <ChannelList
          space={withLounge()}
          selectedId={null}
          call={{
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    }}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Lounge")).toHaveAttribute("data-call", "connected");
      expect(entryFor("Music")).not.toHaveAttribute("data-call");
    });

    it("marks the channel it is still joining", () => {
      render(
        <ChannelList
          space={withLounge()}
          selectedId={null}
          call={{ state: "connecting", roomId: LOUNGE }}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Lounge")).toHaveAttribute("data-call", "connecting");
    });

    it("keeps being in a channel separate from having it selected", () => {
      // A voice channel stays joined while somebody clicks around the list.
      // If the two looked the same, clicking elsewhere would read as leaving.
      render(
        <ChannelList
          space={withLounge()}
          selectedId="!a:example.org"
          call={{
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    }}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Lounge")).toHaveAttribute("data-selected", "false");
      expect(entryFor("Lounge")).toHaveAttribute("data-call", "connected");
      expect(entryFor(/general/)).toHaveAttribute("data-selected", "true");
      expect(entryFor(/general/)).not.toHaveAttribute("data-call");
    });

    it("puts the reason a join failed beside the channel that refused it", () => {
      render(
        <ChannelList
          space={withLounge()}
          selectedId={null}
          call={{
            state: "failed",
            roomId: LOUNGE,
            error: "no voice server would take this call",
          }}
          onSelect={vi.fn()}
        />,
      );

      const problem = screen.getByRole("alert");
      expect(problem).toHaveTextContent("no voice server would take this call");
      expect(entryFor("Lounge").parentElement).toContainElement(problem);
    });

    it("says nothing about a failure in the channel that did not fail", () => {
      // The room id on the failure is what makes this possible. A second
      // channel can be clicked while the first is still connecting.
      render(
        <ChannelList
          space={withLounge()}
          selectedId={null}
          call={{ state: "failed", roomId: LOUNGE, error: "no voice server" }}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Music").parentElement).not.toHaveTextContent(
        "no voice server",
      );
    });

    it("draws the call's own roster for the channel it is in", () => {
      // Better than room state for this one channel, and only this one. The
      // call roster comes from MatrixRTC signalling, so it is right in every
      // generation; room state is only right in the oldest.
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge", [person("@stale:example.org", "Stale")])])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada")],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      const people = screen.getByRole("list", { name: "In Lounge" });
      expect(people).toHaveTextContent("Ada");
      expect(people).not.toHaveTextContent("Stale");
    });

    it("marks somebody who has muted themselves", () => {
      // Comes from the SFU rather than from anything this session did, so it
      // is the one thing here that is true of other people. Named as well as
      // drawn: a colour with no glyph beside it is nothing to a screen reader,
      // and a glyph with no name on it is nothing either.
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              person("@ada:example.org", "Ada", true),
              person("@bob:example.org", "Bob"),
            ],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is muted")).toBeVisible();
      expect(screen.queryByLabelText("Bob is muted")).toBeNull();
    });

    it("shows headphones rather than a microphone for somebody deafened", () => {
      // One icon, not two. Deafening mutes, so both flags are set on the same
      // person, and the headphones are the stronger claim: somebody muted may
      // still be listening, somebody deafened is not.
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              { id: "@ada:example.org", name: "Ada", muted: true, deafened: true },
            ],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is deafened")).toBeVisible();
      expect(screen.queryByLabelText("Ada is muted")).toBeNull();
    });

    it("shows a clock for somebody who is away", () => {
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              { id: "@ada:example.org", name: "Ada", muted: true, away: true },
            ],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is away")).toBeVisible();
      // The clock replaces the microphone rather than joining it. Away mutes,
      // so both flags are set, and two icons would spend twice the width on
      // one fact.
      expect(screen.queryByLabelText("Ada is muted")).toBeNull();
    });

    it("shows headphones rather than a clock for somebody both away and deafened", () => {
      // The precedence, at the one point where all three flags are true.
      // Deafened outranks away because it is the stronger claim: an away
      // person may come back and hear what was said, a deafened one will not.
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              {
                id: "@ada:example.org",
                name: "Ada",
                muted: true,
                deafened: true,
                away: true,
              },
            ],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is deafened")).toBeVisible();
      expect(screen.queryByLabelText("Ada is away")).toBeNull();
    });

    it("says nothing about being away for somebody who has only muted", () => {
      // Away is Consort clients telling each other over the call's data
      // channel. Somebody in Element Call says nothing, and guessing would put
      // a clock beside a person sitting right there.
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada", true)],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.queryByLabelText(/is away/)).toBeNull();
    });

    it("says nothing about deafening for somebody who has only muted", () => {
      // Deafening is Consort clients telling each other over the call's data
      // channel. Somebody in Element Call says nothing, and guessing would put
      // headphones beside a person who can hear perfectly well.
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada", true)],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByLabelText("Ada is muted")).toBeVisible();
      expect(screen.queryByLabelText(/is deafened/)).toBeNull();
    });

    it("marks who is talking", () => {
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [
              person("@ada:example.org", "Ada"),
              person("@bob:example.org", "Bob"),
            ],
            trouble: null,
          }}
          speaking={new Set(["@ada:example.org"])}
          onSelect={vi.fn()}
        />,
      );

      const people = within(
        screen.getByRole("list", { name: "In Lounge" }),
      ).getAllByRole("listitem");

      expect(people[0]).toHaveAttribute("data-speaking", "true");
      expect(people[1]).toHaveAttribute("data-speaking", "false");
    });

    it("marks this session's own user when they are the one talking", () => {
      // The SFU reports every speaker it can hear, including us. Excluding
      // ourselves would leave the one person most likely to be looking for the
      // ring as the only one who never gets it.
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@me:example.org", "Me")],
            trouble: null,
          }}
          speaking={new Set(["@me:example.org"])}
          onSelect={vi.fn()}
        />,
      );

      const [me] = within(
        screen.getByRole("list", { name: "In Lounge" }),
      ).getAllByRole("listitem");

      expect(me).toHaveAttribute("data-speaking", "true");
    });

    it("marks nobody when nobody is talking", () => {
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge")])}
          selectedId={null}
          call={{
            state: "connected",
            roomId: LOUNGE,
            participants: [person("@ada:example.org", "Ada")],
            trouble: null,
          }}
          onSelect={vi.fn()}
        />,
      );

      const [ada] = within(
        screen.getByRole("list", { name: "In Lounge" }),
      ).getAllByRole("listitem");

      expect(ada).toHaveAttribute("data-speaking", "false");
    });

    it("says nothing about mute for a channel this session is not in", () => {
      // Room state lists who is in a channel and nothing else about them. An
      // unmuted mark there would be a finding rather than a silence, and it
      // would be wrong for anybody who had in fact muted.
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge", [person("@ada:example.org", "Ada")])])}
          selectedId={null}
          call={{ state: "disconnected" }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByRole("list", { name: "In Lounge" })).toHaveTextContent(
        "Ada",
      );
      expect(screen.queryByLabelText(/is muted/)).toBeNull();
    });

    it("leaves every other channel on room state", () => {
      render(
        <ChannelList
          space={space([
            voice(LOUNGE, "Lounge", [person("@ada:example.org", "Ada")]),
            voice("!b:example.org", "Music", [person("@bob:example.org", "Bob")]),
          ])}
          selectedId={null}
          call={{
      state: "connected",
      roomId: LOUNGE,
      participants: [],
      trouble: null,
    }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByRole("list", { name: "In Music" })).toHaveTextContent(
        "Bob",
      );
      // And the joined channel really did take the call's answer, which is
      // that nobody is in it yet.
      expect(
        screen.queryByRole("list", { name: "In Lounge" }),
      ).toBeNull();
    });

    it("keeps room state while a join is still in flight", () => {
      // A connecting call has no roster yet and a failed one never will.
      // Blanking the list for either would wipe something that was correct a
      // second ago and put it back when the join lands.
      render(
        <ChannelList
          space={space([voice(LOUNGE, "Lounge", [person("@ada:example.org", "Ada")])])}
          selectedId={null}
          call={{ state: "connecting", roomId: LOUNGE }}
          onSelect={vi.fn()}
        />,
      );

      expect(screen.getByRole("list", { name: "In Lounge" })).toHaveTextContent(
        "Ada",
      );
    });

    it("marks nothing when there is no call", () => {
      render(
        <ChannelList
          space={withLounge()}
          selectedId={null}
          call={IDLE}
          onSelect={vi.fn()}
        />,
      );

      expect(entryFor("Lounge")).not.toHaveAttribute("data-call");
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });
});
