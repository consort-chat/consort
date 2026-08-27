import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const audioDevices = vi.hoisted(() => vi.fn());
const audioSettings = vi.hoisted(() => vi.fn());
const audioTestStart = vi.hoisted(() => vi.fn());
const audioTestStop = vi.hoisted(() => vi.fn());
const onAudio = vi.hoisted(() => vi.fn());
const logout = vi.hoisted(() => vi.fn());
const roomAvatar = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  audioDevices,
  audioSettings,
  audioTestStart,
  audioTestStop,
  onAudio,
  logout,
  roomAvatar,
}));

import { AppShell } from "./AppShell";
import { resetAvatarCache } from "../lib/avatars";
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

function shell(onSignedOut = vi.fn()) {
  const { container } = render(
    <AppShell
      profile={profile}
      rooms={{ spaces: [{ id: "home", name: "Home", avatar: null, channels: [] }] }}
      connection={{ state: "live" }}
      verification={{ state: "verified" }}
      keyBackup={{ state: "enabled" }}
      storage={null}
      flows={[]}
      canStartVerification
      onDismissFlow={vi.fn()}
      onSignedOut={onSignedOut}
    />,
  );
  return { container, onSignedOut };
}

describe("AppShell", () => {
  beforeEach(() => {
    resetAvatarCache();
    audioDevices.mockReset().mockResolvedValue(report);
    audioSettings.mockReset().mockResolvedValue(settings);
    audioTestStart.mockReset().mockResolvedValue(undefined);
    audioTestStop.mockReset().mockResolvedValue(undefined);
    onAudio.mockReset().mockResolvedValue(() => {});
    logout.mockReset().mockResolvedValue(undefined);
    roomAvatar.mockReset().mockResolvedValue(null);
  });

  it("opens settings from the gear on the user strip", async () => {
    shell();

    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    expect(await screen.findByRole("dialog")).toBeVisible();
  });

  it("shows no dialog until it is asked for", () => {
    shell();

    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("takes the rest of the application out of reach while settings are open", async () => {
    // The focus trap keeps Tab inside the dialog. This is the other half:
    // nothing behind it should be clickable, focusable, or read out.
    const { container } = shell();
    const layout = container.querySelector(".shell");

    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    expect(layout).toHaveAttribute("inert");
  });

  it("gives the application back when settings close", async () => {
    const { container } = shell();
    const layout = container.querySelector(".shell");
    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    await userEvent.click(screen.getByRole("button", { name: /close settings/i }));

    expect(layout).not.toHaveAttribute("inert");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("signs out from inside settings", async () => {
    const { onSignedOut } = shell();
    await userEvent.click(screen.getByRole("button", { name: /user settings/i }));

    await userEvent.click(screen.getByRole("button", { name: /log out/i }));

    await waitFor(() => expect(logout).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onSignedOut).toHaveBeenCalledTimes(1));
  });

  it("puts focus back on the gear when settings close", async () => {
    shell();
    const gear = screen.getByRole("button", { name: /user settings/i });
    await userEvent.click(gear);

    await userEvent.click(screen.getByRole("button", { name: /close settings/i }));

    await waitFor(() => expect(document.activeElement).toBe(gear));
  });
});
