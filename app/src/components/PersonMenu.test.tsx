import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const audioSettings = vi.hoisted(() => vi.fn());
const setPersonVolume = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  audioSettings,
  setPersonVolume,
}));

import { PersonMenu } from "./PersonMenu";
import type { AudioSettings } from "../lib/api";

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

function open(personVolumes: Record<string, number> = {}) {
  audioSettings.mockResolvedValue({ ...settings, personVolumes });
  const onClose = vi.fn();
  render(
    <PersonMenu
      userId="@ada:example.org"
      name="Ada"
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

  it("still opens when the settings cannot be read", async () => {
    // Drawn at full rather than left saying nothing. A menu that never
    // resolves is worse than one showing the value almost everybody is at.
    audioSettings.mockRejectedValue(new Error("no settings"));
    render(
      <PersonMenu
        userId="@ada:example.org"
        name="Ada"
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
