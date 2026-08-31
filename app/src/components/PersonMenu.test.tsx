import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const audioSettings = vi.hoisted(() => vi.fn());
const setPersonVolume = vi.hoisted(() => vi.fn());
const memberProfile = vi.hoisted(() => vi.fn());
const memberAvatar = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  audioSettings,
  setPersonVolume,
  memberProfile,
  memberAvatar,
}));

import { PersonMenu } from "./PersonMenu";
import type {
  AudioSettings,
  MemberProfile,
  Participant,
} from "../lib/api";

const settings: AudioSettings = {
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

const ada: Participant = { id: "@ada:example.org", name: "Ada" };

/** A homeserver that answers, with nothing remarkable to say. */
const plain: MemberProfile = {
  presence: "online",
  status: null,
  lastActiveAgo: null,
  standing: "member",
};

function open(
  personVolumes: Record<string, number> = {},
  person: Participant = ada,
  profile: MemberProfile = plain,
) {
  audioSettings.mockResolvedValue({ ...settings, personVolumes });
  memberProfile.mockResolvedValue(profile);
  const onClose = vi.fn();
  render(
    <PersonMenu
      person={person}
      roomId="!room:example.org"
      at={{ x: 40, y: 60 }}
      onClose={onClose}
    />,
  );
  return onClose;
}

/** The one control, once the saved level has been read. */
function slider() {
  return screen.findByRole("slider");
}

describe("PersonMenu", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setPersonVolume.mockResolvedValue(undefined);
    memberProfile.mockResolvedValue(plain);
    memberAvatar.mockResolvedValue(null);
  });

  it("shows the level that was saved for this person", async () => {
    open({ "@ada:example.org": 55 });

    expect(await slider()).toHaveValue("55");
  });

  it("shows full volume for somebody nobody has adjusted", async () => {
    // Absent is not zero. A map that holds only the people who have been
    // changed is the whole reason the file stays small, and reading a missing
    // entry as silence would mute everybody it had never heard of.
    open();

    expect(await slider()).toHaveValue("100");
  });

  it("names the person it is about", async () => {
    // One menu at a time, opened from a list of similar rows. Without the name
    // on it there is nothing to say which row it belongs to.
    open();

    expect(await screen.findByRole("dialog", { name: /Ada/ })).toBeVisible();
  });

  it("remembers a new level against the user id", async () => {
    open({ "@ada:example.org": 100 });
    const control = await slider();

    // `fireEvent.change` rather than a drag or an arrow key. jsdom implements
    // neither the pointer geometry of a thumb nor a range input's own keyboard
    // handling, so the change event is the only way to move one here.
    fireEvent.change(control, { target: { value: "55" } });

    await waitFor(() =>
      expect(setPersonVolume).toHaveBeenCalledWith("@ada:example.org", 55),
    );
  });

  it("writes once for a slider that was dragged across the range", async () => {
    // A range input fires an event per step. Every one of them writing the
    // settings file would be a hundred rewrites for one adjustment, so the
    // write waits for somebody to stop moving.
    open({ "@ada:example.org": 100 });
    const control = await slider();

    for (const value of ["90", "80", "70", "60"]) {
      fireEvent.change(control, { target: { value } });
    }

    await waitFor(() => expect(setPersonVolume).toHaveBeenCalled());
    expect(setPersonVolume).toHaveBeenCalledTimes(1);
    expect(setPersonVolume).toHaveBeenCalledWith("@ada:example.org", 60);
  });

  it("follows the slider before the write lands", async () => {
    // The number under the thumb is what somebody is aiming with. Waiting for
    // the write would make it lag the hand by a tenth of a second, which reads
    // as a control that is not keeping up.
    open({ "@ada:example.org": 50 });
    const control = await slider();

    fireEvent.change(control, { target: { value: "51" } });

    expect(control).toHaveValue("51");
    expect(setPersonVolume).not.toHaveBeenCalled();
  });

  it("closes on Escape", async () => {
    const onClose = open();
    await slider();

    await userEvent.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalled();
  });

  it("closes when something else is clicked", async () => {
    const onClose = open();
    await slider();

    await userEvent.click(document.body);

    expect(onClose).toHaveBeenCalled();
  });

  it("stays open while its own slider is being used", async () => {
    // The outside-click handler is on the document, so a menu that did not
    // check what was clicked would close itself the moment somebody grabbed
    // the thumb.
    const onClose = open();
    const control = await slider();

    await userEvent.click(control);

    expect(onClose).not.toHaveBeenCalled();
  });

  it("shows who it is about by name and by user id", async () => {
    // Two people in a room can carry one display name, and the user id is the
    // only thing on the card that tells them apart.
    open();

    expect(await screen.findByText("Ada")).toBeVisible();
    expect(screen.getByText("@ada:example.org")).toBeVisible();
  });

  it("draws the presence the homeserver reported", async () => {
    open({}, ada, { ...plain, presence: "idle" });

    expect(await screen.findByText("Idle")).toBeVisible();
  });

  it("says the status is unknown rather than guessing offline", async () => {
    // Most homeservers have presence switched off. Reading that silence as
    // "offline" would put a grey dot on somebody sitting right there.
    open({}, ada, { ...plain, presence: "unknown" });

    expect(await screen.findByText("Status unknown")).toBeVisible();
  });

  it("still draws the card when the profile request fails", async () => {
    // The one request this panel makes. Everything else on the card came with
    // the roster and is already on screen behind it.
    memberProfile.mockRejectedValue(new Error("signed out"));
    audioSettings.mockResolvedValue(settings);
    render(
      <PersonMenu
        person={ada}
        roomId="!room:example.org"
        at={{ x: 0, y: 0 }}
        onClose={vi.fn()}
      />,
    );

    expect(await slider()).toHaveValue("100");
    expect(screen.getByText("Ada")).toBeVisible();
  });

  it("shows a status message when somebody set one", async () => {
    open({}, ada, { ...plain, status: "in a meeting" });

    expect(await screen.findByText("in a meeting")).toBeVisible();
  });

  it("badges the standings that change what somebody can do", async () => {
    open({}, ada, { ...plain, standing: "moderator" });

    expect(await screen.findByText("Moderator")).toBeVisible();
  });

  it("puts no badge on an ordinary member", async () => {
    // A badge on everybody is a badge that says nothing.
    open({}, ada, { ...plain, standing: "member" });
    await slider();

    expect(screen.queryByText("Member")).toBeNull();
  });

  it("says how long they have been in the call", async () => {
    const since = Date.now() - 5 * 60 * 1000;
    open({}, { ...ada, since });

    expect(await screen.findByText("5 minutes")).toBeVisible();
  });

  it("says nothing about a join time it does not have", async () => {
    // Everybody listed from room state, and anybody whose media has not
    // appeared. No answer beats a made-up one.
    open();
    await slider();

    expect(screen.queryByText("In call")).toBeNull();
  });

  it("writes out every call state at once", async () => {
    // Unlike the row, where the three flags compete for one icon slot. Here
    // there is room to say all of them, and "deafened and away" is a different
    // fact from either alone.
    open({}, { ...ada, muted: true, deafened: true, camera: true });

    expect(await screen.findByText("Deafened, Muted, Camera on")).toBeVisible();
  });

  it("offers messaging as a button that says it does not work yet", async () => {
    // Rather than leaving it off. A card with no way to talk to somebody reads
    // as a feature that was forgotten rather than one that is not built.
    open();

    const message = await screen.findByRole("button", { name: /message/i });
    expect(message).toBeDisabled();
    expect(screen.getByText(/cannot show messages yet/i)).toBeVisible();
  });

  it("keeps the volume slider under the person's details", async () => {
    // The order is the order somebody wants them in: who is this, and then,
    // occasionally, turn them down.
    open();
    const control = await slider();
    const name = screen.getByText("Ada");

    expect(
      name.compareDocumentPosition(control) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("still opens when the settings cannot be read", async () => {
    // Drawn at full rather than left saying nothing. A menu that never
    // resolves is worse than one showing the value almost everybody is at.
    audioSettings.mockRejectedValue(new Error("no settings"));
    memberProfile.mockResolvedValue(plain);
    render(
      <PersonMenu
        person={ada}
        roomId="!room:example.org"
        at={{ x: 0, y: 0 }}
        onClose={vi.fn()}
      />,
    );

    expect(await slider()).toHaveValue("100");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /something went wrong/i,
    );
  });
});
