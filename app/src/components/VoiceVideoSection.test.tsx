import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const audioDevices = vi.hoisted(() => vi.fn());
const audioSettings = vi.hoisted(() => vi.fn());
const setAudioSettings = vi.hoisted(() => vi.fn());
const audioTestStart = vi.hoisted(() => vi.fn());
const audioTestStop = vi.hoisted(() => vi.fn());
const audioTonePlay = vi.hoisted(() => vi.fn());
const audioToneStop = vi.hoisted(() => vi.fn());
const onAudio = vi.hoisted(() => vi.fn());

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  audioDevices,
  audioSettings,
  setAudioSettings,
  audioTestStart,
  audioTestStop,
  audioTonePlay,
  audioToneStop,
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
    voiceActivity: true,
  },
  callSounds: true,
  callVoices: true,
  outputVolume: 100,
  notificationVolume: 60,
  personVolumes: {},
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
    audioTonePlay.mockReset().mockResolvedValue(undefined);
    audioToneStop.mockReset().mockResolvedValue(undefined);
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

  it("plays a test tone out of the chosen output when asked", async () => {
    // The whole of Phase 7. An input can be checked by talking at it; an
    // output cannot be checked by anything unless something plays.
    render(<VoiceVideoSection />);

    await userEvent.click(await screen.findByRole("button", { name: /check/i }));

    expect(audioTonePlay).toHaveBeenCalled();
  });

  it("names the output the tone actually came out of", async () => {
    // The question the button was pressed to answer. "Something played" is
    // half of it; "out of these speakers" is the half that settles whether
    // the picker is pointing where somebody thought it was.
    render(<VoiceVideoSection />);
    await waitFor(() => expect(onAudio).toHaveBeenCalled());

    push({ state: "toneStarted", device: "Headphones" });

    expect(await screen.findByText(/Playing through Headphones/)).toBeVisible();
  });

  it("keeps saying which output it was after the chime has finished", async () => {
    // The chime is about a third of a second. A note that appears and
    // disappears inside that is a note nobody reads, so it stays and changes
    // tense.
    render(<VoiceVideoSection />);
    await waitFor(() => expect(onAudio).toHaveBeenCalled());
    push({ state: "toneStarted", device: "Headphones" });

    push({ state: "toneStopped" });

    expect(await screen.findByText(/Played through Headphones/)).toBeVisible();
  });

  it("says nothing about a tone until one has been played", async () => {
    render(<VoiceVideoSection />);
    await screen.findByLabelText(/output device/i);

    expect(screen.queryByText(/through/i)).not.toBeInTheDocument();
  });

  it("reports an output that would not play rather than staying silent", async () => {
    // Silence is the failure mode this button exists to diagnose, so silence
    // is the one thing it must never be the answer to.
    render(<VoiceVideoSection />);
    await waitFor(() => expect(onAudio).toHaveBeenCalled());

    push({ state: "toneFailed", error: "there is no audio output device" });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /no audio output device/,
    );
  });

  it("reports a tone command that never reached the backend", async () => {
    audioTonePlay.mockRejectedValue({ message: "no sound", detail: "no sound" });
    render(<VoiceVideoSection />);

    await userEvent.click(await screen.findByRole("button", { name: /check/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/no sound/);
  });

  it("does not put the level meter into running because a tone started", async () => {
    // Two devices on one event channel. A handler that treated `toneStarted`
    // as `started` would tell somebody their microphone was live because they
    // pressed the speaker button.
    render(<VoiceVideoSection />);
    await waitFor(() => expect(onAudio).toHaveBeenCalled());

    push({ state: "toneStarted", device: "Headphones" });

    expect(await screen.findByText("Not running.")).toBeVisible();
  });

  it("stops the tone when the section goes away", async () => {
    // Short, but not so short that it cannot outlive the panel that started
    // it. A chime left playing into a closed screen holds the output open for
    // a sound nobody can now stop.
    const { unmount } = render(<VoiceVideoSection />);
    await waitFor(() => expect(audioTestStart).toHaveBeenCalled());

    unmount();

    await waitFor(() => expect(audioToneStop).toHaveBeenCalled());
  });

  it("offers no test button on a machine with nothing to play through", async () => {
    audioDevices.mockResolvedValue({
      ...report,
      output: { devices: [], selected: null, missing: null },
    });

    render(<VoiceVideoSection />);

    expect(await screen.findByText(/nothing Consort can play sound/i)).toBeVisible();
    expect(screen.queryByRole("button", { name: /check/i })).not.toBeInTheDocument();
  });

  it("offers voice activity detection as a switch, on by default", async () => {
    render(<VoiceVideoSection />);

    const toggle = await screen.findByRole("switch", { name: /voice activity/i });
    expect(toggle).toBeChecked();
  });

  it("saves voice activity being turned off", async () => {
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", { name: /voice activity/i });

    await userEvent.click(toggle);

    await waitFor(() =>
      expect(setAudioSettings).toHaveBeenCalledWith({
        ...defaults,
        gate: { ...defaults.gate, voiceActivity: false },
      }),
    );
  });

  it("draws the chime switch from what the wire said", async () => {
    // The chime defaults off in Rust and the announcement defaults on, so this
    // pane cannot read both the same way. Drawn from the payload rather than
    // from a guess, which is where the two would drift.
    render(<VoiceVideoSection />);

    const toggle = await screen.findByRole("switch", { name: /chime as well/i });
    expect(toggle).toBeChecked();
  });

  it("draws the chime as off when the field is missing entirely", async () => {
    // The upgrade case, and the one asymmetry worth pinning: a payload written
    // before the field existed has to render as off, because off is the Rust
    // default. Read as `!== false` it would render as on and the switch would
    // disagree with what the call actually does.
    const { callSounds: _dropped, ...older } = defaults;
    audioSettings.mockResolvedValue(older as AudioSettings);
    render(<VoiceVideoSection />);

    const toggle = await screen.findByRole("switch", { name: /chime as well/i });
    expect(toggle).not.toBeChecked();
  });

  it("saves the chime being turned off", async () => {
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", { name: /chime as well/i });

    await userEvent.click(toggle);

    await waitFor(() =>
      expect(setAudioSettings).toHaveBeenCalledWith({
        ...defaults,
        callSounds: false,
      }),
    );
  });

  it("offers spoken notifications as a switch, on by default", async () => {
    // On is the default in Rust too, and this is where the two would drift. A
    // missing field on the wire has to render as on, not as off.
    render(<VoiceVideoSection />);

    const toggle = await screen.findByRole("switch", {
      name: /say out loud/i,
    });
    expect(toggle).toBeChecked();
  });

  it("saves spoken notifications being turned off", async () => {
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", {
      name: /say out loud/i,
    });

    await userEvent.click(toggle);

    await waitFor(() =>
      expect(setAudioSettings).toHaveBeenCalledWith({
        ...defaults,
        callVoices: false,
      }),
    );
  });

  it("leaves the chimes alone when the spoken notifications go off", async () => {
    // The whole reason there are two switches. One patch that reached the
    // other field would make the second setting decoration, and the symptom
    // would be somebody losing their chimes by turning off the sentences.
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", {
      name: /say out loud/i,
    });

    await userEvent.click(toggle);

    await waitFor(() => expect(setAudioSettings).toHaveBeenCalled());
    const [saved] = setAudioSettings.mock.calls.at(-1) as [typeof defaults];
    expect(saved.callSounds).toBe(true);
  });

  it("leaves the spoken notifications alone when the chimes go off", async () => {
    // The same claim in the other direction, which is the one an
    // implementation that shares a field would still pass without.
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", {
      name: /chime as well/i,
    });

    await userEvent.click(toggle);

    await waitFor(() => expect(setAudioSettings).toHaveBeenCalled());
    const [saved] = setAudioSettings.mock.calls.at(-1) as [typeof defaults];
    expect(saved.callVoices).toBe(true);
  });

  it("offers a call volume slider at what was saved", async () => {
    audioSettings.mockResolvedValue({ ...defaults, outputVolume: 70 });
    render(<VoiceVideoSection />);

    const slider = await screen.findByRole("slider", {
      name: /everybody in a call/i,
    });
    expect(slider).toHaveValue("70");
  });

  it("offers an announcement volume slider at what was saved", async () => {
    audioSettings.mockResolvedValue({ ...defaults, notificationVolume: 35 });
    render(<VoiceVideoSection />);

    const slider = await screen.findByRole("slider", {
      name: /announcement volume/i,
    });
    expect(slider).toHaveValue("35");
  });

  it("draws full volume when the field is missing entirely", async () => {
    // A settings file written before the sliders existed. Read as anything
    // other than the Rust default, the control would disagree with what the
    // call is actually doing.
    const { outputVolume: _o, notificationVolume: _n, ...older } = defaults;
    audioSettings.mockResolvedValue(older as AudioSettings);
    render(<VoiceVideoSection />);

    expect(
      await screen.findByRole("slider", { name: /everybody in a call/i }),
    ).toHaveValue("100");
    expect(
      await screen.findByRole("slider", { name: /announcement volume/i }),
    ).toHaveValue("60");
  });

  it("saves a call volume that was dragged", async () => {
    render(<VoiceVideoSection />);
    const slider = await screen.findByRole("slider", {
      name: /everybody in a call/i,
    });

    fireEvent.change(slider, { target: { value: "40" } });

    await waitFor(() =>
      expect(setAudioSettings).toHaveBeenCalledWith({
        ...defaults,
        outputVolume: 40,
      }),
    );
  });

  it("writes once for a slider dragged across the range", async () => {
    // A range input fires an event per step. Writing the settings file on each
    // of them would be a hundred rewrites for one adjustment.
    render(<VoiceVideoSection />);
    const slider = await screen.findByRole("slider", {
      name: /announcement volume/i,
    });

    for (const value of ["50", "40", "30", "20"]) {
      fireEvent.change(slider, { target: { value } });
    }

    await waitFor(() => expect(setAudioSettings).toHaveBeenCalled());
    expect(setAudioSettings).toHaveBeenCalledTimes(1);
    expect(setAudioSettings).toHaveBeenCalledWith({
      ...defaults,
      notificationVolume: 20,
    });
  });

  it("moves the slider before the write lands", async () => {
    // The number under the thumb is what somebody is aiming with, so it cannot
    // wait for a round trip to the settings file.
    render(<VoiceVideoSection />);
    const slider = await screen.findByRole("slider", {
      name: /everybody in a call/i,
    });

    fireEvent.change(slider, { target: { value: "40" } });

    expect(slider).toHaveValue("40");
    expect(setAudioSettings).not.toHaveBeenCalled();
  });

  it("leaves the two volumes independent of each other", async () => {
    // Two controls over two levels. One patch reaching the other field would
    // make the second slider decoration, and the symptom would be somebody
    // turning a call down and losing their announcements with it.
    render(<VoiceVideoSection />);
    const slider = await screen.findByRole("slider", {
      name: /announcement volume/i,
    });

    fireEvent.change(slider, { target: { value: "10" } });

    await waitFor(() => expect(setAudioSettings).toHaveBeenCalled());
    const [saved] = setAudioSettings.mock.calls.at(-1) as [typeof defaults];
    expect(saved.outputVolume).toBe(defaults.outputVolume);
  });

  it("leaves the gate alone when call sounds change", async () => {
    // The two patches merge at different depths, and a top-level change that
    // reached in and rewrote the gate would silently reset somebody's voice
    // activity tuning every time they touched an unrelated switch.
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", { name: /chime as well/i });

    await userEvent.click(toggle);

    await waitFor(() => expect(setAudioSettings).toHaveBeenCalled());
    const [saved] = setAudioSettings.mock.calls.at(-1) as [typeof defaults];
    expect(saved.gate).toEqual(defaults.gate);
  });

  it("does not reopen the microphone to change the input mode", async () => {
    // The point of the retune. Somebody flipping this switch is watching the
    // bar while they do it, and a sound card that closes and reopens under
    // them drops the bar to zero and re-announces the device.
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", { name: /voice activity/i });
    audioTestStart.mockClear();

    await userEvent.click(toggle);

    await waitFor(() => expect(setAudioSettings).toHaveBeenCalled());
    expect(audioTestStart).not.toHaveBeenCalled();
  });

  it("changes what the meter says when voice activity goes off", async () => {
    // The switch has to be visibly the thing deciding, or nobody can tell it
    // from a microphone that has stopped working.
    render(<VoiceVideoSection />);
    await waitFor(() => expect(onAudio).toHaveBeenCalled());
    push({ state: "started", device: "Built-in Microphone" });
    expect(await screen.findByText(/listening/i)).toBeVisible();

    await userEvent.click(
      await screen.findByRole("switch", { name: /voice activity/i }),
    );

    expect(
      await screen.findByText(/everything the microphone hears/i),
    ).toBeVisible();
  });

  it("puts the switch back when the save fails", async () => {
    // Unlike a device picker, which at worst points at the wrong name, a
    // switch stuck where the backend disagrees is telling somebody their
    // microphone is doing the opposite of what it is doing.
    setAudioSettings.mockRejectedValue({ message: "nope", detail: "nope" });
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", { name: /voice activity/i });

    await userEvent.click(toggle);

    await waitFor(() => expect(toggle).toBeChecked());
    expect(await screen.findByRole("alert")).toHaveTextContent(/nope/);
  });

  it("reads the switch from what was saved rather than assuming", async () => {
    audioSettings.mockResolvedValue({
      ...defaults,
      gate: { ...defaults.gate, voiceActivity: false },
    });

    render(<VoiceVideoSection />);

    expect(
      await screen.findByRole("switch", { name: /voice activity/i }),
    ).not.toBeChecked();
  });

  it("offers noise suppression as a switch, on by default", async () => {
    render(<VoiceVideoSection />);

    const toggle = await screen.findByRole("switch", { name: /background noise/i });
    expect(toggle).toBeChecked();
  });

  it("saves noise suppression being turned off", async () => {
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", { name: /background noise/i });

    await userEvent.click(toggle);

    await waitFor(() =>
      expect(setAudioSettings).toHaveBeenCalledWith({
        ...defaults,
        gate: { ...defaults.gate, denoise: false },
      }),
    );
  });

  it("leaves voice activity alone when noise suppression changes", async () => {
    // The two are separate switches over one model pass, and somebody who
    // turns the denoiser off to hear their raw microphone must not find the
    // gate has come off with it.
    render(<VoiceVideoSection />);

    await userEvent.click(
      await screen.findByRole("switch", { name: /background noise/i }),
    );

    await waitFor(() =>
      expect(setAudioSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          gate: expect.objectContaining({ voiceActivity: true }),
        }),
      ),
    );
    expect(
      await screen.findByRole("switch", { name: /voice activity/i }),
    ).toBeChecked();
  });

  it("puts the noise suppression switch back when the save fails", async () => {
    setAudioSettings.mockRejectedValue({ message: "nope", detail: "nope" });
    render(<VoiceVideoSection />);
    const toggle = await screen.findByRole("switch", { name: /background noise/i });

    await userEvent.click(toggle);

    await waitFor(() => expect(toggle).toBeChecked());
    expect(await screen.findByRole("alert")).toHaveTextContent(/nope/);
  });

  it("reads noise suppression from what was saved rather than assuming", async () => {
    audioSettings.mockResolvedValue({
      ...defaults,
      gate: { ...defaults.gate, denoise: false },
    });

    render(<VoiceVideoSection />);

    expect(
      await screen.findByRole("switch", { name: /background noise/i }),
    ).not.toBeChecked();
  });

  it("survives the device list failing to load", async () => {
    // A rejected command must not take the panel with it. The section is
    // reachable from a strip somebody clicks by accident.
    audioDevices.mockRejectedValue({ message: "no", detail: "no" });

    render(<VoiceVideoSection />);

    expect(await screen.findByRole("alert")).toBeVisible();
  });
});
