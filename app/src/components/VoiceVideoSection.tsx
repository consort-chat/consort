import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import {
  asCommandError,
  audioDevices,
  audioSettings,
  audioTestStart,
  audioTestStop,
  audioTonePlay,
  audioToneStop,
  onAudio,
  setAudioSettings,
  type AudioDeviceList,
  type AudioSettings,
} from "../lib/api";
import { LevelMeter } from "./LevelMeter";
import "./VoiceVideoSection.css";

/** What the level meter is showing, which is nothing until the first reading. */
interface Meter {
  level: number;
  open: boolean;
  running: boolean;
  /** The device the backend actually opened, which is the last word on it. */
  device: string | null;
}

const SILENT: Meter = { level: 0, open: false, running: false, device: null };

/** What the output test has to show for itself. */
interface Chime {
  /** The output the last chime came out of, or null before the first press. */
  device: string | null;
  playing: boolean;
}

const QUIET: Chime = { device: null, playing: false };

interface PickerProps {
  id: string;
  label: string;
  list: AudioDeviceList;
  /**
   * The device to show as chosen, which is not always the one in `list`.
   *
   * Between picking a device and the backend confirming it there is a gap, and
   * during it the list still names the old device. Drawing that would tell
   * somebody their click did nothing.
   */
  selected: string | null;
  /** What to say when the machine has none of this kind. */
  absent: string;
  onChange: (name: string) => void;
  /** Anything belonging under the picker, such as the output test button. */
  children?: ReactNode;
}

/**
 * One device picker.
 *
 * A native `select` rather than a styled listbox. Two of them, on a screen
 * that has to work the first time somebody opens it, is the wrong place to
 * reimplement keyboard handling and typeahead that the platform already has.
 */
function DevicePicker({
  id,
  label,
  list,
  selected,
  absent,
  onChange,
  children,
}: PickerProps) {
  if (list.devices.length === 0) {
    return (
      <div className="voice-field">
        <span className="voice-field__label">{label}</span>
        <p className="voice-field__note">{absent}</p>
      </div>
    );
  }

  return (
    <div className="voice-field">
      <label className="voice-field__label" htmlFor={id}>
        {label}
      </label>
      <select
        id={id}
        className="voice-field__select"
        value={selected ?? ""}
        onChange={(event) => onChange(event.target.value)}
      >
        {list.devices.map((device) => (
          <option key={device.name} value={device.name}>
            {device.name}
          </option>
        ))}
      </select>
      {/*
        Said out loud rather than resolved silently. Somebody who chose a
        headset and is now being recorded by a laptop lid is entitled to know
        that, and the moment to tell them is while they are looking at the
        picker.
      */}
      {list.missing !== null && (
        <p className="voice-field__note voice-field__note--warn">
          {list.missing} is not plugged in. Using {list.selected} instead.
        </p>
      )}
      {children}
    </div>
  );
}

/**
 * Input, output, and proof that the microphone works.
 *
 * The microphone opens when this appears and closes when it goes, with no
 * button in between. That is the whole point of the screen: somebody who came
 * here to find out whether Consort can hear them should find out by arriving,
 * not by finding a second control and pressing it.
 *
 * Devices are re-read on every open and after every change rather than cached.
 * A device can appear or vanish while the window is up, and a picker drawn
 * from a stale list offers things that are not there.
 */
export function VoiceVideoSection() {
  const [devices, setDevices] = useState<{
    input: AudioDeviceList;
    output: AudioDeviceList;
  } | null>(null);
  const [settings, setSettings] = useState<AudioSettings | null>(null);
  const [meter, setMeter] = useState<Meter>(SILENT);
  const [chime, setChime] = useState<Chime>(QUIET);
  const [problem, setProblem] = useState<string | null>(null);

  /*
    What was picked here, until the backend has been asked again and answered.

    Re-reading the device list means asking every device on the machine what it
    supports, which is the only way to find out and is not fast. Null once the
    answer is in, so the backend stays the authority on what is actually open
    and this never becomes a second, quietly diverging copy of it.
  */
  const [picked, setPicked] = useState<{
    input: string | null;
    output: string | null;
  }>({ input: null, output: null });

  // Held in a ref as well as in state because `change` needs the current value
  // and is not re-created per render. Reading it from state there would close
  // over whichever value existed when the handler was made.
  const saved = useRef<AudioSettings | null>(null);

  const reload = useCallback(async () => {
    const [report, current] = await Promise.all([
      audioDevices(),
      audioSettings(),
    ]);
    setDevices({ input: report.input, output: report.output });
    setSettings(current);
    saved.current = current;
  }, []);

  useEffect(() => {
    let cancelled = false;
    let stop: (() => void) | null = null;

    onAudio((activity) => {
      if (cancelled) return;
      switch (activity.state) {
        case "started":
          setProblem(null);
          setMeter({
            level: 0,
            open: false,
            running: true,
            device: activity.device,
          });
          break;
        case "stopped":
          setMeter(SILENT);
          break;
        case "failed":
          setMeter(SILENT);
          setProblem(activity.error);
          break;
        case "level":
          // `running` is not read off the event. A reading can only exist
          // while a stream is open, and taking the flag from `started` alone
          // would drop the bar to "not running" for anybody whose first event
          // arrived before this listener did.
          setMeter((current) => ({
            ...current,
            running: true,
            level: activity.level,
            open: activity.open,
          }));
          break;
        case "toneStarted":
          setProblem(null);
          setChime({ device: activity.device, playing: true });
          break;
        case "toneStopped":
          // The device is kept rather than cleared. It is the answer to the
          // question the button was pressed to ask, and a note that vanishes a
          // third of a second after it appears is one nobody reads.
          setChime((current) => ({ ...current, playing: false }));
          break;
        case "toneFailed":
          setChime(QUIET);
          setProblem(activity.error);
          break;
      }
    })
      .then((off) => {
        // Subscribing is asynchronous, so the section can be gone by the time
        // the listener is handed over. Stopping it straight away is the
        // difference between one that ends with the panel and one that lives
        // as long as the process.
        if (cancelled) off();
        else stop = off;
      })
      .catch((raw: unknown) => {
        console.error("could not follow the microphone", asCommandError(raw).detail);
      });

    reload()
      .then(() => (cancelled ? undefined : audioTestStart()))
      .catch((raw: unknown) => {
        if (!cancelled) setProblem(asCommandError(raw).message);
      });

    return () => {
      cancelled = true;
      if (stop !== null) stop();
      // Not conditional on having started. The command is a no-op when
      // nothing is open, and the case worth covering is the one where this
      // unmounts while the start is still in flight.
      audioTestStop().catch((raw: unknown) => {
        console.error("could not close the microphone", asCommandError(raw).detail);
      });
      // The chime is short, but not so short that it cannot outlive the panel
      // that started it. Leaving it playing into a closed screen would hold
      // the output open for a sound nobody can now stop.
      audioToneStop().catch((raw: unknown) => {
        console.error("could not stop the test tone", asCommandError(raw).detail);
      });
    };
  }, [reload]);

  /**
   * Save one device choice.
   *
   * Reopens the microphone for an input change and not for an output one. The
   * meter is showing the device that is open, so leaving the old one running
   * under a picker that says otherwise would be the screen lying about the one
   * thing it exists to report.
   */
  async function change(direction: "input" | "output", name: string) {
    const current = saved.current;
    if (current === null) return;

    const next: AudioSettings = { ...current, [direction]: name };
    setSettings(next);
    saved.current = next;
    setPicked((current) => ({ ...current, [direction]: name }));

    try {
      await setAudioSettings(next);
      // Before the re-read, not after. This is the part with something to
      // show for it: the meter starts moving on the new microphone while the
      // list is still being walked.
      if (direction === "input") await audioTestStart();
      await reload();
      setPicked((current) => ({ ...current, [direction]: null }));
    } catch (raw: unknown) {
      setProblem(asCommandError(raw).message);
    }
  }

  /**
   * Turn voice activity detection on or off.
   *
   * No reopening and no re-reading of the device list. The Rust side retunes
   * the gate that is already running, so the bar in front of somebody keeps
   * moving and simply starts behaving differently, which is the clearest
   * possible demonstration of what the switch does.
   */
  async function setVoiceActivity(on: boolean) {
    const current = saved.current;
    if (current === null) return;

    const next: AudioSettings = {
      ...current,
      gate: { ...current.gate, voiceActivity: on },
    };
    setSettings(next);
    saved.current = next;

    try {
      await setAudioSettings(next);
    } catch (raw: unknown) {
      // Put the switch back. Unlike a device picker, which at worst points at
      // the wrong name, a switch stuck in a position the backend does not
      // agree with is telling somebody their microphone is doing the opposite
      // of what it is doing.
      setSettings(current);
      saved.current = current;
      setProblem(asCommandError(raw).message);
    }
  }

  /**
   * Play the test chime.
   *
   * A failure to reach the backend at all lands in the same alert as a failure
   * to play, because from the outside they are the same event: the button was
   * pressed and no sound came out.
   */
  function check() {
    audioTonePlay().catch((raw: unknown) => {
      setProblem(asCommandError(raw).message);
    });
  }

  return (
    <div className="voice">
      {/*
        An alert rather than a status. Every one of these means the panel is
        not doing the thing it was opened to do, and none of them resolves on
        its own.
      */}
      {problem !== null && (
        <p className="voice__problem" role="alert">
          {problem}
        </p>
      )}

      {devices !== null && settings !== null && (
        <>
          <DevicePicker
            id="voice-input"
            label="Input device"
            list={devices.input}
            selected={picked.input ?? devices.input.selected}
            absent="This machine has no microphone Consort can open."
            onChange={(name) => void change("input", name)}
          />

          <DevicePicker
            id="voice-output"
            label="Output device"
            list={devices.output}
            selected={picked.output ?? devices.output.selected}
            absent="This machine has nothing Consort can play sound through."
            onChange={(name) => void change("output", name)}
          >
            {/*
              Not disabled while it plays. The chime is about a third of a
              second, so a button that greys out and comes back is a flicker,
              and pressing again during one is handled where it should be: the
              audio thread replaces the chime rather than layering a second one
              on top of it.
            */}
            <button type="button" className="voice-field__check" onClick={check}>
              Let&apos;s Check
            </button>
            {chime.device !== null && (
              <p className="voice-field__note">
                {chime.playing ? "Playing" : "Played"} through {chime.device}.
              </p>
            )}
          </DevicePicker>
        </>
      )}

      {settings !== null && (
        <div className="voice-field">
          <span className="voice-field__label">Input mode</span>
          <div className="voice-toggle">
            <input
              id="voice-activity"
              className="voice-toggle__switch"
              type="checkbox"
              role="switch"
              aria-describedby="voice-activity-note"
              checked={settings.gate.voiceActivity}
              onChange={(event) => void setVoiceActivity(event.target.checked)}
            />
            <label className="voice-toggle__label" htmlFor="voice-activity">
              Voice activity detection
            </label>
          </div>
          <p className="voice-field__note" id="voice-activity-note">
            Send audio only while you are speaking. Turn it off to transmit
            continuously, background noise included.
          </p>
        </div>
      )}

      <div className="voice-field">
        <span className="voice-field__label">Mic test</span>
        <LevelMeter
          level={meter.level}
          open={meter.open}
          running={meter.running}
          voiceActivity={settings?.gate.voiceActivity ?? true}
        />
        {meter.device !== null && (
          <p className="voice-field__note">Recording from {meter.device}.</p>
        )}
      </div>
    </div>
  );
}
