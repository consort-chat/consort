import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const audioDevices = vi.hoisted(() => vi.fn());
const audioSettings = vi.hoisted(() => vi.fn());
const audioTestStart = vi.hoisted(() => vi.fn());
const audioTestStop = vi.hoisted(() => vi.fn());
const onAudio = vi.hoisted(() => vi.fn());
const logout = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  audioDevices,
  audioSettings,
  audioTestStart,
  audioTestStop,
  onAudio,
  logout,
}));

import { SettingsModal } from "./SettingsModal";
import type { AudioDeviceReport, AudioSettings, Profile } from "../lib/api";

const profile: Profile = {
  user_id: "@ada:example.org",
  device_id: "ABCDEFGH",
  homeserver: "https://example.org",
  display_name: "Ada",
  avatar_url: null,
};

const report: AudioDeviceReport = {
  input: {
    devices: [{ name: "Built-in Microphone", isDefault: true }],
    selected: "Built-in Microphone",
    missing: null,
  },
  output: {
    devices: [{ name: "Built-in Speakers", isDefault: true }],
    selected: "Built-in Speakers",
    missing: null,
  },
};

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
};

function open(onClose = vi.fn()) {
  render(
    <SettingsModal
      profile={profile}
      onClose={onClose}
      onSignedOut={vi.fn()}
    />,
  );
  return onClose;
}

describe("SettingsModal", () => {
  beforeEach(() => {
    audioDevices.mockReset().mockResolvedValue(report);
    audioSettings.mockReset().mockResolvedValue(settings);
    audioTestStart.mockReset().mockResolvedValue(undefined);
    audioTestStop.mockReset().mockResolvedValue(undefined);
    onAudio.mockReset().mockResolvedValue(() => {});
    logout.mockReset().mockResolvedValue(undefined);
  });

  it("is a modal dialog with a name", () => {
    open();

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog).toHaveAccessibleName(/settings/i);
  });

  it("closes on Escape", async () => {
    // The shortcut every modal has and the one people try first. Discord even
    // prints it beside the close button, which is why this one does too.
    const onClose = open();

    await userEvent.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape after a click has landed on nothing focusable", async () => {
    /*
      The case the plain Escape test cannot reach. A key event is delivered to
      whatever has focus, and clicking any dead space inside the dialog, a
      heading, a label, the gap between two fields, leaves focus on `body`.
      A handler bound to the dialog element never sees a keystroke that starts
      outside it, so Escape stops working the moment somebody clicks anywhere
      before pressing it. Which is to say: almost always.
    */
    const onClose = open();

    await userEvent.click(screen.getByRole("heading", { name: /my account/i }));
    expect(document.activeElement).toBe(document.body);
    await userEvent.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes when the backdrop is clicked", async () => {
    const onClose = open();

    await userEvent.click(screen.getByTestId("settings-backdrop"));

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not close when something inside it is clicked", async () => {
    // The bug this guards against is a click that starts on a control and
    // ends on the backdrop, or the other way round. Both are ordinary
    // pointer behaviour and both would throw away whatever was being done.
    const onClose = open();

    await userEvent.click(screen.getByRole("dialog"));

    expect(onClose).not.toHaveBeenCalled();
  });

  it("has a close button that says how else to close it", async () => {
    const onClose = open();

    await userEvent.click(screen.getByRole("button", { name: /close settings/i }));

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("moves focus inside itself when it opens", async () => {
    open();

    await waitFor(() =>
      expect(screen.getByRole("dialog")).toContainElement(
        document.activeElement as HTMLElement,
      ),
    );
  });

  it("gives focus back to whatever opened it", async () => {
    // Otherwise focus lands back at the top of the document and somebody
    // navigating by keyboard has to tab through the whole shell to get back
    // to where they were.
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();

    const { unmount } = render(
      <SettingsModal profile={profile} onClose={vi.fn()} onSignedOut={vi.fn()} />,
    );
    await waitFor(() => expect(document.activeElement).not.toBe(opener));

    unmount();

    await waitFor(() => expect(document.activeElement).toBe(opener));
    opener.remove();
  });

  it("keeps Tab inside itself", async () => {
    // A trap rather than a hope. Without it, tabbing past the last control
    // walks into the channel list behind, which is still rendered.
    open();
    const dialog = screen.getByRole("dialog");
    const focusable = within(dialog).getAllByRole("button");
    focusable.at(-1)?.focus();

    await userEvent.tab();

    expect(dialog).toContainElement(document.activeElement as HTMLElement);
  });

  it("keeps Shift+Tab inside itself", async () => {
    open();
    const dialog = screen.getByRole("dialog");
    within(dialog).getAllByRole("button").at(0)?.focus();

    await userEvent.tab({ shift: true });

    expect(dialog).toContainElement(document.activeElement as HTMLElement);
  });

  it("opens on My Account", async () => {
    // Which is where Discord opens, and the pane where the thing people came
    // to check about themselves lives.
    open();

    expect(await screen.findByText("@ada:example.org")).toBeVisible();
  });

  it("goes to Voice and Video when asked", async () => {
    open();

    await userEvent.click(screen.getByRole("button", { name: /voice/i }));

    expect(await screen.findByLabelText(/input device/i)).toBeVisible();
  });

  it("does not open the microphone until Voice and Video is showing", async () => {
    // Opening settings to change something else should not take the device
    // away from whatever else is using it.
    open();

    expect(audioTestStart).not.toHaveBeenCalled();
  });

  it("closes the microphone again when leaving Voice and Video", async () => {
    open();
    await userEvent.click(screen.getByRole("button", { name: /voice/i }));
    await waitFor(() => expect(audioTestStart).toHaveBeenCalled());

    await userEvent.click(screen.getByRole("button", { name: /my account/i }));

    await waitFor(() => expect(audioTestStop).toHaveBeenCalled());
  });

  it("marks the section that is showing", async () => {
    open();

    const account = screen.getByRole("button", { name: /my account/i });
    expect(account).toHaveAttribute("aria-current", "page");

    await userEvent.click(screen.getByRole("button", { name: /voice/i }));

    expect(account).not.toHaveAttribute("aria-current", "page");
  });

  it("says it is reading the machine's devices while it does", async () => {
    // Enumerating ALSA means opening every PCM on the machine to ask what it
    // supports, and a menu item that does nothing for most of a second reads
    // as one that does not work.
    audioDevices.mockReturnValue(new Promise(() => {}));
    render(<SettingsModal profile={profile} onClose={vi.fn()} onSignedOut={vi.fn()} />);

    await userEvent.click(screen.getByRole("button", { name: /voice & video/i }));

    expect(
      await screen.findByRole("status", { name: /sound devices/i }),
    ).toBeVisible();
  });

  it("stops saying so once they have arrived", async () => {
    render(<SettingsModal profile={profile} onClose={vi.fn()} onSignedOut={vi.fn()} />);

    await userEvent.click(screen.getByRole("button", { name: /voice & video/i }));

    await waitFor(() =>
      expect(screen.queryByRole("status", { name: /sound devices/i })).toBeNull(),
    );
  });

  it("stops saying so when the machine will not answer either", async () => {
    // The spinner says a question is outstanding, and one answered badly has
    // still been answered.
    audioDevices.mockRejectedValue({ message: "no sound system", detail: "" });
    render(<SettingsModal profile={profile} onClose={vi.fn()} onSignedOut={vi.fn()} />);

    await userEvent.click(screen.getByRole("button", { name: /voice & video/i }));

    await waitFor(() =>
      expect(screen.queryByRole("status", { name: /sound devices/i })).toBeNull(),
    );
  });
});
