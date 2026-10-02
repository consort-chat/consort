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

const usePicture = vi.hoisted(() => vi.fn());
vi.mock("../lib/usePicture", () => ({ usePicture }));

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
  away: {
    shown?: boolean;
    onHide?: () => void;
    cameraOn?: boolean;
    sharing?: string | null;
  } = {},
) {
  return (
    <CallCard
      call={call}
      channelName="Lounge"
      selfId="@bob:example.org"
      shown={away.shown ?? true}
      onHide={away.onHide ?? vi.fn()}
      onOpenRoom={vi.fn()}
      cameraOn={away.cameraOn ?? false}
      sharing={away.sharing ?? null}
      {...(speaking === undefined ? {} : { speaking })}
    />
  );
}

/** The card itself, which is a region named after the channel. */
function onScreen() {
  return screen.queryByRole("region", { name: "Call in Lounge" });
}

/**
 * What the card would measure, if jsdom measured anything.
 *
 * Reports the whole window once the card is filling it, because that is the
 * one size change the card measures itself back after.
 */
function stubLayout(box: { left: number; top: number; width?: number }) {
  const node = onScreen();
  if (node === null) throw new Error("no card to measure");
  const width = box.width ?? 240;
  node.getBoundingClientRect = () => {
    const at =
      node.dataset.size === "full"
        ? {
            left: 0,
            top: 0,
            width: window.innerWidth,
            height: window.innerHeight,
          }
        : { left: box.left, top: box.top, width, height: 160 };
    return {
      ...at,
      right: at.left + at.width,
      bottom: at.top + at.height,
      x: at.left,
      y: at.top,
      toJSON: () => ({}),
    } as DOMRect;
  };
}

beforeEach(() => {
  resetAvatarCache();
  usePicture.mockReset().mockReturnValue(null);
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
      expect(onScreen()).toHaveAttribute("data-size", "expanded");

      await userEvent.dblClick(screen.getByRole("list", { name: "People in Lounge" }));
      expect(onScreen()).toHaveAttribute("data-size", "card");
    });

    it("does not expand when the double-click was on a face", async () => {
      // A face opens the menu about that person. Expanding the card under it
      // at the same time would move the thing that was just clicked.
      render(card(inCall([person("@ada:example.org", "Ada")])));

      await userEvent.dblClick(screen.getByRole("button", { name: /Ada/ }));

      expect(onScreen()).toHaveAttribute("data-size", "card");
    });

    it("expands from a button as well", async () => {
      // A size that can only be changed by a double-click is a size some
      // people cannot change, and nothing on a card says it can be
      // double-clicked in the first place.
      render(card(inCall([person("@ada:example.org", "Ada")])));

      await userEvent.click(
        screen.getByRole("button", { name: "Expand the call card" }),
      );
      expect(onScreen()).toHaveAttribute("data-size", "expanded");

      await userEvent.click(
        screen.getByRole("button", { name: "Expand the call card" }),
      );
      expect(onScreen()).toHaveAttribute("data-size", "card");
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

      expect(onScreen()).toHaveAttribute("data-size", "expanded");
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

      expect(onScreen()).toHaveAttribute("data-size", "card");
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

describe("your own camera on the card", () => {
  const PICTURE = "data:image/jpeg;base64,aaaa";

  /** The camera, which is the one picture of a face that has a name. */
  function preview() {
    return screen.queryByRole("img", { name: "Your camera" });
  }

  /** Somebody's square, by the control that opens their card. */
  function face(name: string) {
    return screen.getByRole("button", { name: new RegExp(name) });
  }

  it("is not drawn while the camera is off", () => {
    // The card as it was. Nothing appears and nothing moves for somebody who
    // never switches a camera on, which is most of every call.
    render(card(inCall([person("@bob:example.org", "Bob")])));

    expect(preview()).toBe(null);
  });

  it("is drawn in your own square rather than in a strip of its own", () => {
    // #140's review, and the reason the avatar became a square: a camera is
    // what that person looks like right now, so it belongs where their face
    // was and not in a band above the call.
    usePicture.mockReturnValue(PICTURE);

    render(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        cameraOn: true,
      }),
    );

    expect(within(face("Bob")).getByRole("img", { name: "Your camera" })).toHaveAttribute(
      "src",
      PICTURE,
    );
  });

  it("is never drawn in somebody else's square", () => {
    // There is one local capture. Drawing it against another name would be
    // this client telling a room that somebody else is on camera.
    usePicture.mockReturnValue(PICTURE);

    render(
      card(
        inCall([
          person("@bob:example.org", "Bob"),
          person("@ann:example.org", "Ann"),
        ]),
        undefined,
        { cameraOn: true },
      ),
    );

    expect(
      within(face("Ann")).queryByRole("img", { name: "Your camera" }),
    ).toBeNull();
  });

  it("is not drawn before the first frame arrives", () => {
    // Between opening a device and its first frame. A card drawing an empty
    // picture there would flash an empty box every time the camera came on.
    usePicture.mockReturnValue(null);

    render(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        cameraOn: true,
      }),
    );

    expect(preview()).toBe(null);
  });

  it("leaves the face underneath while it waits for one", () => {
    // Which is what makes the line above safe. An avatar taken away to make
    // room would leave an empty square for however long a device takes.
    usePicture.mockReturnValue(null);

    render(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        cameraOn: true,
      }),
    );

    expect(face("Bob")).toHaveTextContent("Bob");
  });

  it("goes away again when the camera does", () => {
    usePicture.mockReturnValue(PICTURE);
    const { rerender } = render(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        cameraOn: true,
      }),
    );
    expect(preview()).not.toBe(null);

    rerender(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        cameraOn: false,
      }),
    );

    expect(preview()).toBe(null);
  });

  it("keeps the faces beside it", () => {
    // The camera is added to the card rather than replacing what it was for.
    // Who else is in the call is the thing the card exists to say.
    usePicture.mockReturnValue(PICTURE);

    render(
      card(
        inCall([
          person("@bob:example.org", "Bob"),
          person("@ann:example.org", "Ann"),
        ]),
        undefined,
        { cameraOn: true },
      ),
    );

    expect(preview()).not.toBe(null);
    expect(screen.getByText("Ann")).toBeInTheDocument();
  });

  it("is not asked for at all while the card is put away", () => {
    // Not merely asked for with the camera off: a hidden card draws nothing,
    // so the component holding the timer is never mounted.
    render(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        cameraOn: true,
        shown: false,
      }),
    );

    expect(usePicture).not.toHaveBeenCalled();
  });

  it("is asked for while the card is up and the camera is on", () => {
    render(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        cameraOn: true,
      }),
    );

    expect(usePicture).toHaveBeenCalledWith("camera");
  });

  it("survives being dragged", () => {
    // The picture is inside the card, so moving the card moves it. What this
    // pins is that a drag does not remount it and lose the frame.
    usePicture.mockReturnValue(PICTURE);
    render(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        cameraOn: true,
      }),
    );
    const before = preview();
    stubLayout({ left: 100, top: 100 });

    const grip = screen.getByRole("button", {
      name: "Move the Lounge call card with the arrow keys",
    });
    fireEvent.pointerDown(grip, { clientX: 150, clientY: 150, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 200, clientY: 220, pointerId: 1 });
    fireEvent.pointerUp(window, { clientX: 200, clientY: 220, pointerId: 1 });

    expect(preview()).toBe(before);
    expect(preview()).toHaveAttribute("src", PICTURE);
  });
});

describe("screens being shared on the card", () => {
  const PICTURE = "data:image/jpeg;base64,bbbb";

  function sharer(id: string, name: string): Participant {
    return { id, name, muted: false, screen: true };
  }

  /** The screen filling the width of the card, announced by what it shows. */
  function stage() {
    return screen.queryByRole("button", { name: /, fill the window$/ });
  }

  /** The screens that did not get the stage, which is a list and not a roster. */
  function strip() {
    return screen.queryByRole("list", {
      name: "Other screens shared in Lounge",
    });
  }

  it("says nothing about screens while nobody is sharing one", () => {
    // Which is most of every call. An empty stage announced to a screen reader
    // is furniture that says nothing.
    render(card(inCall([person("@bob:example.org", "Bob")])));

    expect(stage()).toBe(null);
    expect(strip()).toBe(null);
  });

  it("puts this session's own screen on the stage, captioned with what is going out", () => {
    usePicture.mockReturnValue(PICTURE);

    render(card(inCall([person("@bob:example.org", "Bob")]), undefined, {
      sharing: "DP-0 (2560x1440)",
    }));

    expect(stage()).toHaveTextContent("DP-0 (2560x1440)");
    expect(
      within(stage()!).getByRole("img", { name: "Your screen" }),
    ).toBeVisible();
  });

  it("puts somebody else's screen on the stage when theirs is the only one", () => {
    render(
      card(
        inCall([
          person("@bob:example.org", "Bob"),
          sharer("@ada:example.org", "Ada"),
        ]),
      ),
    );

    expect(stage()).toHaveTextContent("Ada's screen");
  });

  it("leaves no strip behind while one screen is being shared", () => {
    // The stage is the whole answer then. A list of one that is already on the
    // stage is the same tile drawn twice.
    render(card(inCall([person("@bob:example.org", "Bob")]), undefined, {
      sharing: "DP-0",
    }));

    expect(strip()).toBe(null);
  });

  it("sends every screen but the staged one to the strip", () => {
    // The requirement is a square per screen rather than a slot somebody wins.
    // Three is the example; nothing here counts.
    render(
      card(
        inCall([
          sharer("@ada:example.org", "Ada"),
          sharer("@cyd:example.org", "Cyd"),
          person("@bob:example.org", "Bob"),
        ]),
        undefined,
        { sharing: "DP-0" },
      ),
    );

    expect(stage()).toHaveTextContent("DP-0");
    expect(within(strip()!).getAllByRole("listitem")).toHaveLength(2);
    expect(strip()!).toHaveTextContent("Ada's screen");
    expect(strip()!).toHaveTextContent("Cyd's screen");
  });

  it("draws this session's own screen once, not twice", () => {
    // Our own publication comes back in the roster a moment after the screen
    // channel has already said so. Both read naively is one screen in two
    // places.
    render(
      card(inCall([sharer("@bob:example.org", "Bob")]), undefined, {
        sharing: "DP-0",
      }),
    );

    expect(stage()).toHaveTextContent("DP-0");
    expect(strip()).toBe(null);
  });

  it("asks for no picture for anybody else's screen", () => {
    // One local capture, and no path from anybody else's into this window.
    render(card(inCall([sharer("@ada:example.org", "Ada")])));

    expect(usePicture).not.toHaveBeenCalled();
  });

  it("keeps the people out of the screens and the screens out of the people", () => {
    // Separate lists because only one of them is a list of people, and that is
    // what each is announced as.
    render(
      card(
        inCall([
          person("@bob:example.org", "Bob"),
          sharer("@ada:example.org", "Ada"),
        ]),
        undefined,
        { sharing: "DP-0" },
      ),
    );

    expect(within(strip()!).queryByText("Bob")).toBeNull();
    const people = screen.getByRole("list", { name: "People in Lounge" });
    expect(within(people).queryByText("DP-0")).toBeNull();
    expect(within(people).queryByText("Ada's screen")).toBeNull();
  });
});

describe("choosing which screen gets the stage", () => {
  const PICTURE = "data:image/jpeg;base64,bbbb";

  function sharer(id: string, name: string): Participant {
    return { id, name, muted: false, screen: true };
  }

  function stage() {
    return screen.queryByRole("button", { name: /, fill the window$/ });
  }

  function strip() {
    return screen.queryByRole("list", {
      name: "Other screens shared in Lounge",
    });
  }

  /** This session sharing DP-0 while Ada shares hers, which is two screens. */
  function two(extra: { sharing?: string | null } = {}) {
    return card(
      inCall([
        person("@bob:example.org", "Bob"),
        sharer("@ada:example.org", "Ada"),
      ]),
      undefined,
      { sharing: extra.sharing === undefined ? "DP-0" : extra.sharing },
    );
  }

  it("stages this session's own share before anybody else's", () => {
    // It is the only share that can draw a picture: nothing carries a remote
    // frame into this window yet. Staging a monitor glyph over a live desktop
    // would be the card choosing the emptier of the two.
    render(two());

    expect(stage()).toHaveTextContent("DP-0");
    expect(strip()!).toHaveTextContent("Ada's screen");
  });

  it("puts a tile on the stage when it is clicked", async () => {
    render(two());

    await userEvent.click(
      screen.getByRole("button", { name: "Put Ada's screen on the stage" }),
    );

    expect(stage()).toHaveTextContent("Ada's screen");
  });

  it("returns the screen that was on the stage to the strip", async () => {
    // A promotion that left the old occupant nowhere would lose a share by
    // clicking the other one.
    render(two());

    await userEvent.click(
      screen.getByRole("button", { name: "Put Ada's screen on the stage" }),
    );

    expect(within(strip()!).getAllByRole("listitem")).toHaveLength(1);
    expect(strip()!).toHaveTextContent("DP-0");
  });

  it("puts a tile on the stage from the keyboard", async () => {
    render(two());

    screen
      .getByRole("button", { name: "Put Ada's screen on the stage" })
      .focus();
    await userEvent.keyboard("{Enter}");

    expect(stage()).toHaveTextContent("Ada's screen");
  });

  it("hands the stage over when the screen on it stops", async () => {
    // Nothing clicked and nothing left to click: a stage still captioned with
    // a share that ended is a picture of something that is no longer going out.
    const { rerender } = render(two());
    await userEvent.click(
      screen.getByRole("button", { name: "Put Ada's screen on the stage" }),
    );
    expect(stage()).toHaveTextContent("Ada's screen");

    rerender(
      card(
        inCall([
          person("@bob:example.org", "Bob"),
          person("@ada:example.org", "Ada"),
        ]),
        undefined,
        { sharing: "DP-0" },
      ),
    );

    expect(stage()).toHaveTextContent("DP-0");
    expect(strip()).toBe(null);
  });

  it("empties the stage when the last share stops", () => {
    const { rerender } = render(two({ sharing: null }));
    expect(stage()).toHaveTextContent("Ada's screen");

    rerender(card(inCall([person("@bob:example.org", "Bob")])));

    expect(stage()).toBe(null);
  });

  it("keeps a chosen screen on the stage while it is still being shared", async () => {
    // A re-render is every roster update and every frame the picture polls
    // for. A choice that survived none of them would be a stage that sprang
    // back the moment anything moved.
    usePicture.mockReturnValue(PICTURE);
    const { rerender } = render(two());
    await userEvent.click(
      screen.getByRole("button", { name: "Put Ada's screen on the stage" }),
    );

    rerender(two());

    expect(stage()).toHaveTextContent("Ada's screen");
  });
});

describe("filling the window with the card", () => {
  function sharing(extra: { sharing?: string | null } = {}) {
    return card(inCall([person("@bob:example.org", "Bob")]), undefined, {
      sharing: extra.sharing ?? "DP-0",
    });
  }

  /** The stage, which is what the review asked be clickable. */
  function stage() {
    return screen.getByRole("button", { name: /, fill the window$/ });
  }

  it("fills the window when the staged screen is clicked", async () => {
    // The whole reason the stage is a control: a desktop in a floating card
    // says which window layout is going out and nothing about what it says.
    render(sharing());

    await userEvent.click(stage());

    expect(onScreen()).toHaveAttribute("data-size", "full");
  });

  it("comes back to the floating card from the same control", async () => {
    // A view somebody cannot leave is not finished, and the control that got
    // them there is the first place they will try.
    render(sharing());
    await userEvent.click(stage());
    // The way out is only worth testing from somewhere, and "card" is where
    // this starts: without this the test passes on a stage that never opened.
    expect(onScreen()).toHaveAttribute("data-size", "full");

    await userEvent.click(stage());

    expect(onScreen()).toHaveAttribute("data-size", "card");
  });

  it("comes back from the card's own control", async () => {
    render(sharing());
    await userEvent.click(stage());

    await userEvent.click(
      screen.getByRole("button", { name: "Expand the call card" }),
    );

    expect(onScreen()).toHaveAttribute("data-size", "card");
  });

  it("comes back on Escape", async () => {
    render(sharing());
    await userEvent.click(stage());
    expect(onScreen()).toHaveAttribute("data-size", "full");

    await userEvent.keyboard("{Escape}");

    expect(onScreen()).toHaveAttribute("data-size", "card");
  });

  it("leaves Escape alone while it is only a card", async () => {
    // Escape belongs to whatever was opened most recently. A card that closed
    // itself on it would vanish every time somebody dismissed a menu.
    render(sharing());
    await userEvent.click(
      screen.getByRole("button", { name: "Expand the call card" }),
    );

    await userEvent.keyboard("{Escape}");

    expect(onScreen()).toHaveAttribute("data-size", "expanded");
  });

  it("stops offering to move the card while it fills the window", async () => {
    // There is nothing to move: it is the window.
    render(sharing());

    await userEvent.click(stage());

    expect(screen.getByRole("button", { name: /^Move the/ })).toBeDisabled();
  });

  it("does not fill the window for the next call", async () => {
    // Filling the window is about one call, the way putting the card away is.
    // Left alone, a card that was full when a call ended would fill the window
    // again the moment the next one started, for a share nobody is making.
    const { rerender } = render(sharing());
    await userEvent.click(stage());
    expect(onScreen()).toHaveAttribute("data-size", "full");

    rerender(card({ state: "disconnected" }));
    rerender(sharing());

    expect(onScreen()).toHaveAttribute("data-size", "card");
  });

  it("keeps a card that was only expanded", async () => {
    // The size somebody chose for the card itself outlives a call, and did
    // before any of this. Only the window-filling view is about one call.
    const { rerender } = render(sharing());
    await userEvent.click(
      screen.getByRole("button", { name: "Expand the call card" }),
    );

    rerender(card({ state: "disconnected" }));
    rerender(sharing());

    expect(onScreen()).toHaveAttribute("data-size", "expanded");
  });

  it("puts a dragged card back where it was", async () => {
    // Filling the window makes the card as big as the screen, and the rule
    // that keeps a floating card on screen would read that as a card hanging
    // off every edge and pin it to the corner. There is nothing to keep in
    // view while it is the window.
    render(sharing());
    stubLayout({ left: 300, top: 200 });
    const grip = screen.getByRole("button", { name: /^Move the/ });
    fireEvent.pointerDown(grip, { clientX: 320, clientY: 210 });
    fireEvent.pointerMove(window, { clientX: 420, clientY: 210 });
    fireEvent.pointerUp(window);
    expect(onScreen()).toHaveStyle({ left: "400px" });

    await userEvent.click(stage());
    expect(onScreen()?.style.left).toBe("");
    await userEvent.keyboard("{Escape}");

    expect(onScreen()).toHaveStyle({ left: "400px" });
  });
});

describe("the people tucked behind a shared screen", () => {
  function sharer(id: string, name: string): Participant {
    return { id, name, muted: false, screen: true };
  }

  /** The roster, which is one list under one name however it is arranged. */
  function roster() {
    return screen.getByRole("list", { name: /^People in Lounge/ });
  }

  /** How its faces are arranged, which is what the stylesheet reads. */
  function faces() {
    return within(roster())
      .queryAllByRole("listitem")
      .filter((item) => item.dataset.layout !== undefined);
  }

  /** A roomful, so the row has more than it can hold. */
  function crowd(many: number) {
    return Array.from({ length: many }, (_, index) =>
      person(`@p${index}:example.org`, `Person ${index}`),
    );
  }

  it("tucks them behind the screen rather than putting them beside it", () => {
    // #137. A card 240px across cannot give a screen room and still spread the
    // call out underneath it, and the screen is what the call is about.
    render(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        sharing: "DP-0",
      }),
    );

    expect(onScreen()).toHaveAttribute("data-peeking", "true");
    expect(faces()[0]).toHaveAttribute("data-layout", "peek");
  });

  it("spreads them back out when the share stops", () => {
    // The card as it was. Nothing about this outlives the share that caused it.
    const { rerender } = render(
      card(inCall([person("@bob:example.org", "Bob")]), undefined, {
        sharing: "DP-0",
      }),
    );

    rerender(card(inCall([person("@bob:example.org", "Bob")])));

    expect(onScreen()).toHaveAttribute("data-peeking", "false");
    expect(faces()[0]).toHaveAttribute("data-layout", "tile");
  });

  it("tucks them behind somebody else's screen too", () => {
    // The question is whether the call is carrying a screen, not whose.
    render(card(inCall([sharer("@ada:example.org", "Ada")])));

    expect(onScreen()).toHaveAttribute("data-peeking", "true");
  });

  it("leaves a peeking face a way into the card about that person", () => {
    // Tucked away is not the same as out of reach. A face is the control that
    // opens somebody's card, and it still is while it is half behind a screen.
    render(card(inCall([person("@ada:example.org", "Ada")]), undefined, {
      sharing: "DP-0",
    }));

    expect(within(roster()).getByRole("button", { name: /Ada/ })).toBeVisible();
  });

  it("counts whoever it had no room for", () => {
    // A row that ran off the side of the card would be a row that hid the
    // thing it is tucked behind.
    render(card(inCall(crowd(7)), undefined, { sharing: "DP-0" }));

    expect(faces()).toHaveLength(4);
    expect(roster()).toHaveTextContent("+3");
  });

  it("says how many it left out, rather than only drawing a number", () => {
    // `+3` is a glyph to anybody reading the screen rather than looking at it.
    render(card(inCall(crowd(7)), undefined, { sharing: "DP-0" }));

    expect(roster()).toHaveAccessibleName("People in Lounge, and 3 more");
  });

  it("counts nobody when everybody fits", () => {
    // `+0` on the end of a row that left nobody out is the shape this goes
    // wrong in, and it says the opposite of what it means.
    render(card(inCall(crowd(4)), undefined, { sharing: "DP-0" }));

    expect(faces()).toHaveLength(4);
    expect(roster()).toHaveAccessibleName("People in Lounge");
    expect(roster()).not.toHaveTextContent("+");
  });

  it("has room for more of them once the card has been expanded", () => {
    // The row is capped by what the card is wide enough for, and expanding it
    // is the one thing that changes that.
    render(card(inCall(crowd(7)), undefined, { sharing: "DP-0" }));

    fireEvent.click(
      screen.getByRole("button", { name: "Expand the call card" }),
    );

    expect(faces()).toHaveLength(7);
  });

  it("tucks nobody away while there is nobody in the call", () => {
    // A join in flight shares a screen with an empty roster. An empty strip
    // above the screen would be furniture that says nothing.
    render(card(inCall([]), undefined, { sharing: "DP-0" }));

    expect(onScreen()).toHaveAttribute("data-peeking", "false");
  });

  it("spreads them out again once the card fills the window", async () => {
    // Nothing is short of room there, which is the only reason to tuck them
    // away. ADR-0008 keeps that view as the same squares, only bigger.
    render(card(inCall([person("@bob:example.org", "Bob")]), undefined, {
      sharing: "DP-0",
    }));

    await userEvent.click(
      within(
        screen.getByRole("list", { name: "Screens shared in Lounge" }),
      ).getByRole("button"),
    );

    expect(onScreen()).toHaveAttribute("data-peeking", "false");
    expect(faces()[0]).toHaveAttribute("data-layout", "tile");
  });

  it("keeps your own camera in your peeking face", () => {
    // Sharing a screen with a camera on is two things at once, and the card
    // said both before this. It still says both.
    usePicture.mockReturnValue("data:image/jpeg;base64,cccc");

    render(card(inCall([person("@bob:example.org", "Bob")]), undefined, {
      sharing: "DP-0",
      cameraOn: true,
    }));

    expect(
      within(roster()).getByRole("img", { name: "Your camera" }),
    ).toBeVisible();
  });
});
