import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const audioDevices = vi.hoisted(() => vi.fn());
const audioSettings = vi.hoisted(() => vi.fn());
const setAudioSettings = vi.hoisted(() => vi.fn());
const audioTestStart = vi.hoisted(() => vi.fn());
const audioTestStop = vi.hoisted(() => vi.fn());
const onAudio = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  audioDevices,
  audioSettings,
  setAudioSettings,
  audioTestStart,
  audioTestStop,
  onAudio,
}));

import { VoiceVideoSection } from "./VoiceVideoSection";
import type { AudioActivity, AudioDeviceReport, AudioSettings } from "../lib/api";

const defaults: AudioSettings = {
  input: null,
  output: null,
  gate: {
    openAt: 0.6,
    closeAt: 0.3,
    attackFrames: 2,
    holdMs: 300,
    denoise: true,
  },
};

const report: AudioDeviceReport = {
  input: {
    devices: [
      { name: "Built-in Microphone", isDefault: true },
      { name: "Yeti", isDefault: false },
    ],
    selected: "Built-in Microphone",
    missing: null,
  },
  output: {
    devices: [
      { name: "Built-in Speakers", isDefault: true },
      { name: "Headphones", isDefault: false },
    ],
    selected: "Built-in Speakers",
    missing: null,
  },
};

/** The handler the component registered, so a test can push events at it. */
let emit: (activity: AudioActivity) => void = () => {};

/** Deliver one event the way the Tauri listener would, inside React's batch. */
function push(activity: AudioActivity) {
  act(() => emit(activity));
}
const unlisten = vi.fn();

describe("VoiceVideoSection", () => {
  beforeEach(() => {
    audioDevices.mockReset().mockResolvedValue(report);
    audioSettings.mockReset().mockResolvedValue(defaults);
    setAudioSettings.mockReset().mockResolvedValue(undefined);
    audioTestStart.mockReset().mockResolvedValue(undefined);
    audioTestStop.mockReset().mockResolvedValue(undefined);
    unlisten.mockReset();
    onAudio.mockReset().mockImplementation((handler) => {
      emit = handler;
      return Promise.resolve(unlisten);
    });
  });

  it("lists the input devices with the one in use selected", async () => {
    render(<VoiceVideoSection />);

    const input = await screen.findByLabelText<HTMLSelectElement>(/input device/i);
    expect(
      Array.from(input.options).map((option) => option.textContent),
    ).toEqual(["Built-in Microphone", "Yeti"]);
    expect(input.value).toBe("Built-in Microphone");
  });

  it("lists the output devices with the one in use selected", async () => {
    render(<VoiceVideoSection />);

    const output = await screen.findByLabelText<HTMLSelectElement>(
      /output device/i,
    );
    expect(output.value).toBe("Built-in Speakers");
  });

  it("opens the microphone as soon as the section appears", async () => {
    // The requirement. Nobody should have to find a button to discover
    // whether their microphone works.
    render(<VoiceVideoSection />);

    await waitFor(() => expect(audioTestStart).toHaveBeenCalled());
  });

  it("closes the microphone when the section goes away", async () => {
    const { unmount } = render(<VoiceVideoSection />);
    await waitFor(() => expect(audioTestStart).toHaveBeenCalled());

    unmount();

    await waitFor(() => expect(audioTestStop).toHaveBeenCalled());
    expect(unlisten).toHaveBeenCalled();
  });

  it("draws the level it is told about", async () => {
    const { container } = render(<VoiceVideoSection />);
    await waitFor(() => expect(onAudio).toHaveBeenCalled());

    push({ state: "started", device: "Built-in Microphone" });
    push({ state: "level", level: 0.5, probability: 0.9, open: true });

    await waitFor(() => {
      const fill = container.querySelector<HTMLElement>(".level-meter__fill");
      expect(fill?.style.width).toBe("50%");
    });
  });

  it("saves a different input device and reopens the microphone on it", async () => {
    render(<VoiceVideoSection />);
    const input = await screen.findByLabelText<HTMLSelectElement>(/input device/i);
    audioTestStart.mockClear();

    await userEvent.selectOptions(input, "Yeti");

    await waitFor(() =>
      expect(setAudioSettings).toHaveBeenCalledWith({
        ...defaults,
        input: "Yeti",
      }),
    );
    // Saving is not enough. The microphone still open is the old one, and a
    // meter that keeps moving while the picker says something else is a lie.
    await waitFor(() => expect(audioTestStart).toHaveBeenCalled());
  });

  it("shows the device you picked while the list is still being re-read", async () => {
    /*
      Re-reading the list means asking every device on the machine what it
      supports, which on a real sound stack takes long enough to watch. The
      picker is drawn from that list, so without an answer of its own it spends
      that time showing the device you just changed away from, and the click
      reads as having failed. People click again.
    */
    render(<VoiceVideoSection />);
    const input = await screen.findByLabelText<HTMLSelectElement>(/input device/i);

    let release: () => void = () => {};
    const settled: AudioDeviceReport = {
      ...report,
      input: { ...report.input, selected: "Yeti" },
    };
    audioDevices.mockImplementationOnce(
      () =>
        new Promise<AudioDeviceReport>((resolve) => {
          release = () => resolve(settled);
        }),
    );

    await userEvent.selectOptions(input, "Yeti");

    expect(input.value).toBe("Yeti");

    await act(async () => {
      release();
    });
    expect(input.value).toBe("Yeti");
  });

  it("saves a different output device without touching the microphone", async () => {
    render(<VoiceVideoSection />);
    const output = await screen.findByLabelText<HTMLSelectElement>(/output device/i);

    await userEvent.selectOptions(output, "Headphones");

    await waitFor(() =>
      expect(setAudioSettings).toHaveBeenCalledWith({
        ...defaults,
        output: "Headphones",
      }),
    );
  });

  it("says which device actually opened", async () => {
    // Not always the one asked for. The backend gets the last word and this
    // is the only place it can be read.
    render(<VoiceVideoSection />);
    await waitFor(() => expect(onAudio).toHaveBeenCalled());

    push({ state: "started", device: "Yeti Stereo Microphone, USB Audio" });

    expect(
      await screen.findByText(/Yeti Stereo Microphone, USB Audio/),
    ).toBeVisible();
  });

  it("shows the backend's own words when the microphone will not open", async () => {
    // Common rather than exceptional on a real desktop: the device is held by
    // something else, or it went away between the list being drawn and this
    // running. "Could not start" sends nobody anywhere.
    render(<VoiceVideoSection />);
    await waitFor(() => expect(onAudio).toHaveBeenCalled());

    push({
      state: "failed",
      error: "the audio backend failed: Device or resource busy",
    });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /Device or resource busy/,
    );
  });

  it("says when the saved device is not plugged in any more", async () => {
    audioDevices.mockResolvedValue({
      ...report,
      input: { ...report.input, missing: "A Headset Somebody Unplugged" },
    });

    render(<VoiceVideoSection />);

    expect(
      await screen.findByText(/A Headset Somebody Unplugged/),
    ).toBeVisible();
  });

  it("says so rather than drawing an empty picker on a machine with no microphone", async () => {
    audioDevices.mockResolvedValue({
      ...report,
      input: { devices: [], selected: null, missing: null },
    });

    render(<VoiceVideoSection />);

    expect(await screen.findByText(/no microphone/i)).toBeVisible();
  });

  it("survives the device list failing to load", async () => {
    // A rejected command must not take the panel with it. The section is
    // reachable from a strip somebody clicks by accident.
    audioDevices.mockRejectedValue({ message: "no", detail: "no" });

    render(<VoiceVideoSection />);

    expect(await screen.findByRole("alert")).toBeVisible();
  });
});
