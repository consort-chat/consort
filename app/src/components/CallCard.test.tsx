import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The faces draw avatars and a face opens a menu that reads the saved levels.
// Both are commands, and an unmocked `invoke` throws into a catch that turns a
// missing answer into a fallback: the tests would pass having exercised the
// wrong path.
const memberAvatar = vi.hoisted(() => vi.fn());
const audioSettings = vi.hoisted(() => vi.fn());
const setPersonVolume = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  memberAvatar,
  audioSettings,
  setPersonVolume,
}));

import { CallCard } from "./CallCard";
import type { Call, Participant } from "../lib/api";
import { resetAvatarCache } from "../lib/avatars";

const LOUNGE = "!lounge:example.org";

/** What `audioSettings` answers with, for the menu a face opens. */
const SETTINGS = {
  input: null,
  output: null,
  gate: {
    openAt: 0.6,
    closeAt: 0.3,
    attackFrames: 2,
    holdMs: 300,
    denoise: true,
    voiceActivity: true,
  },
  callSounds: false,
  callVoices: true,
  outputVolume: 100,
  notificationVolume: 60,
  personVolumes: {},
};

function person(id: string, name: string): Participant {
  return { id, name, muted: false };
}

function inCall(participants: Participant[]): Call {
  return { state: "connected", roomId: LOUNGE, participants, trouble: null };
}

function card(
  call: Call,
  speaking?: ReadonlySet<string>,
  away: { shown?: boolean; onHide?: () => void } = {},
) {
  return (
    <CallCard
      call={call}
      channelName="Lounge"
      selfId="@bob:example.org"
      shown={away.shown ?? true}
      onHide={away.onHide ?? vi.fn()}
      onOpenRoom={vi.fn()}
      {...(speaking === undefined ? {} : { speaking })}
    />
  );
}

/** The card itself, which is a region named after the channel. */
function onScreen() {
  return screen.queryByRole("region", { name: "Call in Lounge" });
}

/** What the card would measure, if jsdom measured anything. */
function stubLayout(box: { left: number; top: number; width?: number }) {
  const node = onScreen();
  if (node === null) throw new Error("no card to measure");
  const width = box.width ?? 240;
  node.getBoundingClientRect = () =>
    ({
      left: box.left,
      top: box.top,
      width,
      height: 160,
      right: box.left + width,
      bottom: box.top + 160,
      x: box.left,
      y: box.top,
      toJSON: () => ({}),
    }) as DOMRect;
}

beforeEach(() => {
  resetAvatarCache();
  memberAvatar.mockReset().mockResolvedValue(null);
  audioSettings.mockReset().mockResolvedValue(SETTINGS);
  setPersonVolume.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(window, "innerWidth", { value: 1024, configurable: true });
  Object.defineProperty(window, "innerHeight", { value: 768, configurable: true });
});

afterEach(() => {
  Object.defineProperty(window, "innerWidth", { value: 1024, configurable: true });
  Object.defineProperty(window, "innerHeight", { value: 768, configurable: true });
});

describe("CallCard", () => {
  describe("when it is drawn at all", () => {
    it("draws nothing when there is no call", () => {
      render(card({ state: "disconnected" }));

      expect(onScreen()).toBeNull();
    });

    it("draws nothing when a join failed", () => {
      // The same condition the call panel uses. A failure is explained beside
      // the channel that would not take it, and a card about a call that does
      // not exist would be a second answer to a question nobody asked.
      render(card({ state: "failed", roomId: LOUNGE, error: "No" }));

      expect(onScreen()).toBeNull();
    });

    it("appears as soon as a join is in flight", () => {
      render(card({ state: "connecting", roomId: LOUNGE }));

      expect(onScreen()).toBeVisible();
      expect(onScreen()).toHaveTextContent("Connecting");
    });

    it("says voice channel rather than a room id when it cannot name one", () => {
      // The room list is what names a channel, and it can be behind. A card
      // headed with a raw room id would be worse than a card headed with
      // nothing at all.
      render(
        <CallCard
          call={inCall([])}
          channelName={null}
          selfId="@bob:example.org"
          shown
          onHide={vi.fn()}
          onOpenRoom={vi.fn()}
        />,
      );

      const drawn = screen.getByRole("region", { name: /^Call in/ });
      expect(drawn).toHaveTextContent(/voice channel/i);
      expect(drawn).not.toHaveTextContent(LOUNGE);
    });

    it("appears for a connected call", () => {
      render(card(inCall([person("@ada:example.org", "Ada")])));

      expect(onScreen()).toBeVisible();
    });
  });

  describe("who is on it", () => {
    it("draws one face per person in the call", () => {
      render(
        card(
          inCall([
            person("@ada:example.org", "Ada"),
            person("@bob:example.org", "Bob"),
          ]),
        ),
      );

      expect(
        within(screen.getByRole("list", { name: "People in Lounge" }))
          .getAllByRole("listitem")
          .map((face) => face.textContent),
      ).toEqual(["AAda", "BBob"]);
    });

    it("rings whoever is talking", () => {
      render(
        card(
          inCall([
            person("@ada:example.org", "Ada"),
            person("@bob:example.org", "Bob"),
          ]),
          new Set(["@ada:example.org"]),
        ),
      );

      const faces = within(
        screen.getByRole("list", { name: "People in Lounge" }),
      ).getAllByRole("listitem");

      expect(faces[0]).toHaveAttribute("data-speaking", "true");
      expect(faces[1]).toHaveAttribute("data-speaking", "false");
    });

    it("clears every ring when the room goes quiet", () => {
      // `Speaking` is not a state channel, so a set that empties is the only
      // thing that ever clears a ring. A card that kept the last set would
      // leave somebody lit up after they stopped talking.
      const call = inCall([person("@ada:example.org", "Ada")]);
      const { rerender } = render(card(call, new Set(["@ada:example.org"])));

      rerender(card(call, new Set()));

      expect(
        within(screen.getByRole("list", { name: "People in Lounge" })).getByRole(
          "listitem",
        ),
      ).toHaveAttribute("data-speaking", "false");
    });

    it("opens the card about somebody whose face is clicked", async () => {
      render(card(inCall([person("@ada:example.org", "Ada")])));

      await userEvent.click(screen.getByRole("button", { name: /Ada/ }));

      expect(await screen.findByRole("dialog", { name: /Ada/ })).toBeVisible();
    });
  });

  describe("moving it", () => {
    it("follows the pointer by the handle", () => {
      render(card(inCall([person("@ada:example.org", "Ada")])));
      stubLayout({ left: 300, top: 200 });

      const handle = screen.getByRole("button", { name: /^Move the/ });
      fireEvent.pointerDown(handle, { clientX: 320, clientY: 210 });
      fireEvent.pointerMove(window, { clientX: 350, clientY: 260 });
      fireEvent.pointerUp(window);

      expect(onScreen()).toHaveStyle({ left: "330px", top: "250px" });
    });

    it("stops at the edge of the window", () => {
      render(card(inCall([person("@ada:example.org", "Ada")])));
      stubLayout({ left: 300, top: 200 });

      const handle = screen.getByRole("button", { name: /^Move the/ });
      fireEvent.pointerDown(handle, { clientX: 320, clientY: 210 });
      fireEvent.pointerMove(window, { clientX: 4000, clientY: 4000 });

      expect(onScreen()).toHaveStyle({
        left: `${1024 - 240 - 8}px`,
        top: `${768 - 160 - 8}px`,
      });
    });

    it("moves with the arrow keys", () => {
      // A control that only exists as a drag target is a control some people
      // cannot reach.
      render(card(inCall([person("@ada:example.org", "Ada")])));
      stubLayout({ left: 300, top: 200 });

      fireEvent.keyDown(
        screen.getByRole("button", { name: /^Move the/ }),
        { key: "ArrowRight" },
      );

      expect(onScreen()).toHaveStyle({ left: "316px" });
    });
  });

  describe("expanding it", () => {
    it("expands on a double-click and collapses on the next one", async () => {
      render(card(inCall([person("@ada:example.org", "Ada")])));

      await userEvent.dblClick(screen.getByRole("list", { name: "People in Lounge" }));
      expect(onScreen()).toHaveAttribute("data-expanded", "true");

      await userEvent.dblClick(screen.getByRole("list", { name: "People in Lounge" }));
      expect(onScreen()).toHaveAttribute("data-expanded", "false");
    });

    it("does not expand when the double-click was on a face", async () => {
      // A face opens the menu about that person. Expanding the card under it
      // at the same time would move the thing that was just clicked.
      render(card(inCall([person("@ada:example.org", "Ada")])));

      await userEvent.dblClick(screen.getByRole("button", { name: /Ada/ }));

      expect(onScreen()).toHaveAttribute("data-expanded", "false");
    });

    it("expands from a button as well", async () => {
      // A size that can only be changed by a double-click is a size some
      // people cannot change, and nothing on a card says it can be
      // double-clicked in the first place.
      render(card(inCall([person("@ada:example.org", "Ada")])));

      await userEvent.click(
        screen.getByRole("button", { name: "Expand the call card" }),
      );
      expect(onScreen()).toHaveAttribute("data-expanded", "true");

      await userEvent.click(
        screen.getByRole("button", { name: "Expand the call card" }),
      );
      expect(onScreen()).toHaveAttribute("data-expanded", "false");
    });

    it("pulls a card at the edge back in when it grows", async () => {
      // Expanding makes it wider. A card parked against the right edge grows
      // straight off it, and nothing but this would bring it back.
      render(card(inCall([person("@ada:example.org", "Ada")])));
      stubLayout({ left: 300, top: 200 });

      const handle = screen.getByRole("button", { name: /^Move the/ });
      fireEvent.pointerDown(handle, { clientX: 320, clientY: 210 });
      fireEvent.pointerMove(window, { clientX: 4000, clientY: 210 });
      fireEvent.pointerUp(window);
      expect(onScreen()).toHaveStyle({ left: `${1024 - 240 - 8}px` });

      // What it measures once the wider rules apply.
      stubLayout({ left: 776, top: 200, width: 420 });
      await userEvent.click(
        screen.getByRole("button", { name: "Expand the call card" }),
      );

      expect(onScreen()).toHaveStyle({ left: `${1024 - 420 - 8}px` });
    });

    it("still expands after an earlier drag", () => {
      // The drag that refuses a double-click is the one that just happened.
      // A card dragged once must not go on refusing every double-click it is
      // given for the rest of the call.
      render(card(inCall([person("@ada:example.org", "Ada")])));
      stubLayout({ left: 300, top: 200 });

      const handle = screen.getByRole("button", { name: /^Move the/ });
      fireEvent.pointerDown(handle, { clientX: 320, clientY: 210 });
      fireEvent.pointerMove(window, { clientX: 420, clientY: 210 });
      fireEvent.pointerUp(window);

      const people = screen.getByRole("list", { name: "People in Lounge" });
      fireEvent.pointerDown(people);
      fireEvent.pointerUp(people);
      fireEvent.doubleClick(people);

      expect(onScreen()).toHaveAttribute("data-expanded", "true");
    });

    it("does not expand when the pointer was dragging", () => {
      // Somebody who moved the card and put it back has dragged it, and
      // expanding it under their hand is not what they asked for.
      render(card(inCall([person("@ada:example.org", "Ada")])));
      stubLayout({ left: 300, top: 200 });

      const handle = screen.getByRole("button", { name: /^Move the/ });
      fireEvent.pointerDown(handle, { clientX: 320, clientY: 210 });
      fireEvent.pointerMove(window, { clientX: 420, clientY: 210 });
      fireEvent.pointerMove(window, { clientX: 320, clientY: 210 });
      fireEvent.pointerUp(window);
      /*
        On the card rather than on the handle. A press that starts on the grip
        and ends anywhere else is a click on the pair's nearest common
        ancestor, which is the card, so this is the one way a drag arrives
        here looking like something that was clicked.
      */
      const drawn = onScreen();
      if (drawn === null) throw new Error("no card");
      fireEvent.doubleClick(drawn);

      expect(onScreen()).toHaveAttribute("data-expanded", "false");
    });
  });

  describe("closing it", () => {
    it("asks to be put away rather than putting itself away", async () => {
      // The shell holds whether the card is up, because the control that
      // brings it back is in the sidebar and has to outlive this markup.
      const onHide = vi.fn();
      render(
        card(inCall([person("@ada:example.org", "Ada")]), undefined, {
          onHide,
        }),
      );

      await userEvent.click(
        screen.getByRole("button", { name: "Hide the call card" }),
      );

      expect(onHide).toHaveBeenCalledTimes(1);
    });

    it("draws nothing once it has been put away", () => {
      render(
        card(inCall([person("@ada:example.org", "Ada")]), undefined, {
          shown: false,
        }),
      );

      expect(onScreen()).toBeNull();
    });

    it("comes back where it was left", () => {
      // The place it was moved to survives being put away, because the hook
      // outlives the markup. A card that came back in its opening corner
      // would have thrown away the drag that put it somewhere useful.
      const { rerender } = render(
        card(inCall([person("@ada:example.org", "Ada")])),
      );
      stubLayout({ left: 300, top: 200 });
      fireEvent.keyDown(screen.getByRole("button", { name: /^Move the/ }), {
        key: "ArrowRight",
      });
      expect(onScreen()).toHaveStyle({ left: "316px" });

      rerender(
        card(inCall([person("@ada:example.org", "Ada")]), undefined, {
          shown: false,
        }),
      );
      rerender(card(inCall([person("@ada:example.org", "Ada")])));

      expect(onScreen()).toHaveStyle({ left: "316px" });
    });

    it("comes back on screen when the window shrank while it was away", () => {
      // The edge case the clamp is for. A card dragged against the right edge
      // and then put away is not being measured by anything: the node is
      // gone, so the resize listener has nothing to read and the place it was
      // left at stays where it was. Give the window back smaller and that
      // place is outside it, so a restore that trusted the old coordinate
      // would put the card where nobody can reach it, and the only way back
      // would be the drag handle that went with it.
      const people = [person("@ada:example.org", "Ada")];
      const { rerender } = render(card(inCall(people)));
      stubLayout({ left: 300, top: 200 });

      const handle = screen.getByRole("button", { name: /^Move the/ });
      fireEvent.pointerDown(handle, { clientX: 320, clientY: 210 });
      fireEvent.pointerMove(window, { clientX: 4000, clientY: 4000 });
      fireEvent.pointerUp(window);
      expect(onScreen()).toHaveStyle({
        left: `${1024 - 240 - 8}px`,
        top: `${768 - 160 - 8}px`,
      });

      rerender(card(inCall(people), undefined, { shown: false }));
      expect(onScreen()).toBeNull();

      // Smaller, with nothing on screen to notice. The listener runs and
      // finds no card to measure, which is what leaves the stale coordinate.
      Object.defineProperty(window, "innerWidth", {
        value: 640,
        configurable: true,
      });
      Object.defineProperty(window, "innerHeight", {
        value: 480,
        configurable: true,
      });
      fireEvent(window, new Event("resize"));

      /*
        The card that comes back is a new node, so the stub above went with
        the old one. On the prototype for this one restore, because the size
        has to be readable the moment the layout effect runs and there is no
        gap between the commit and that effect to put it back in.
      */
      const measured = Element.prototype.getBoundingClientRect;
      Element.prototype.getBoundingClientRect = function () {
        return {
          left: 0,
          top: 0,
          width: 240,
          height: 160,
          right: 240,
          bottom: 160,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        } as DOMRect;
      };
      try {
        rerender(card(inCall(people)));
      } finally {
        Element.prototype.getBoundingClientRect = measured;
      }

      expect(onScreen()).toHaveStyle({
        left: `${640 - 240 - 8}px`,
        top: `${480 - 160 - 8}px`,
      });
    });

    it("leaves the call alone", () => {
      // The panel in the sidebar is what says you are in a call and what hangs
      // up. Closing the card must not be a way to lose either.
      render(card(inCall([person("@ada:example.org", "Ada")])));

      expect(
        screen.queryByRole("button", { name: /disconnect|hang up|leave/i }),
      ).toBeNull();
    });
  });
});
