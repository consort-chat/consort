import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import {
  asCommandError,
  audioDevices,
  audioMonitorStart,
  audioMonitorStop,
  audioSettings,
  audioTestStart,
  audioTestStop,
  audioTonePlay,
  audioToneStop,
  onAudio,
  setAudioSettings,
  type AudioDeviceList,
  FRAME_MS,
  type AudioSettings,
  type GateConfig,
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
 * One volume slider.
 *
 * A native `range` rather than a drawn track, for the same reason the pickers
 * above are native `select`s: keyboard handling, the arrow-key step, and the
 * drag behaviour are all already correct, and a screen somebody opens once has
 * no business reimplementing them.
 *
 * The percentage is drawn beside it because a slider with no number on it
 * cannot be described. "About two thirds" is not something somebody can write
 * down, come back to, or tell anybody else.
 */
function VolumeSlider({
  id,
  label,
  percent,
  onChange,
  describedBy,
  min = 0,
  max = 100,
  step = 1,
  format = (value: number) => `${value}%`,
}: {
  id: string;
  label: string;
  percent: number;
  onChange: (percent: number) => void;
  describedBy?: string;
  /**
   * The range and the readout, for the gate sliders below.
   *
   * Defaulted to a percentage because that is what every volume here is, so
   * the four call sites that are volumes say nothing about any of this.
   */
  min?: number;
  max?: number;
  step?: number;
  format?: (value: number) => string;
}) {
  return (
    <div className="voice-volume">
      <label className="voice-volume__label" htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        className="voice-volume__slider"
        type="range"
        min={min}
        max={max}
        step={step}
        value={percent}
        aria-describedby={describedBy}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      {/*
        `aria-hidden`, because the range input already announces its own value
        and a second reading of the same number is noise. This is for the eye.
      */}
      <output className="voice-volume__value" htmlFor={id} aria-hidden="true">
        {format(percent)}
      </output>
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
export function VoiceVideoSection({
  onReady,
}: {
  /**
   * Called once the machine's devices and the saved settings have landed.
   *
   * The settings screen draws a spinner beside this section's menu item until
   * then, because enumerating ALSA takes long enough that a menu item which
   * does nothing meanwhile reads as one that does not work.
   */
  onReady?: () => void;
}) {
  const [devices, setDevices] = useState<{
    input: AudioDeviceList;
    output: AudioDeviceList;
  } | null>(null);
  const [settings, setSettings] = useState<AudioSettings | null>(null);
  const [meter, setMeter] = useState<Meter>(SILENT);
  const [chime, setChime] = useState<Chime>(QUIET);
  /*
    Whether the microphone is being played back, and out of what.

    Read off the events rather than set on the press, for the reason every
    other control here is: what is drawn should be what the audio thread did,
    not what it was asked to do. A press that reached a machine with no working
    output leaves this off and puts the reason on screen.
  */
  const [listening, setListening] = useState<string | null>(null);
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
  // A slider's last value, and the timer that will write it. Refs rather than
  // state, because nothing renders differently while a write is pending and a
  // re-render per pixel of drag is exactly what this is here to avoid.
  const pending = useRef<AudioSettings | null>(null);
  const writing = useRef<ReturnType<typeof setTimeout> | null>(null);

  const reload = useCallback(async () => {
    const [report, current] = await Promise.all([
      audioDevices(),
      audioSettings(),
    ]);
    setDevices({ input: report.input, output: report.output });
    setSettings(current);
    saved.current = current;
  }, []);

  // Somebody who drags a slider and immediately closes the settings screen has
  // still made the change. Without this the timer is torn down with the
  // component and the last hundred and fifty milliseconds of intent is lost,
  // which reads as a setting that does not stick.
  useEffect(
    () => () => {
      if (writing.current === null) return;
      clearTimeout(writing.current);
      writing.current = null;
      const last = pending.current;
      pending.current = null;
      if (last !== null) void setAudioSettings(last);
    },
    [],
  );

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
        case "monitorStarted":
          setProblem(null);
          setListening(activity.device);
          break;
        case "monitorStopped":
          setListening(null);
          break;
        case "monitorFailed":
          setListening(null);
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
      .then(() => {
        if (cancelled) return undefined;
        onReady?.();
        return audioTestStart();
      })
      .catch((raw: unknown) => {
        if (cancelled) return;
        // Reported as ready even so. The spinner says a question is being
        // asked, and one that has been answered badly has still been answered.
        onReady?.();
        setProblem(asCommandError(raw).message);
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
    // `onReady` is deliberately not a dependency. It is a fresh closure on
    // every render of the settings screen, and listing it would tear the
    // microphone down and reopen it on each one.
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
   * Change one part of the gate.
   *
   * No reopening and no re-reading of the device list. The Rust side retunes
   * the gate that is already running, so the bar in front of somebody keeps
   * moving and simply starts behaving differently, which is the clearest
   * possible demonstration of what a switch does.
   *
   * Both switches come through here for the rollback below, which is the part
   * worth getting right once rather than twice.
   */
  /**
   * Write out whatever a slider last left behind.
   *
   * No rollback, unlike the two below. There is nothing sensible to roll back
   * to: by the time a write fails somebody has moved the slider several more
   * times, and putting the control back where it was a second ago would fight
   * the hand still on it. The message says what happened instead.
   */
  async function flush() {
    const next = pending.current;
    if (next === null) return;
    pending.current = null;

    try {
      await setAudioSettings(next);
    } catch (raw: unknown) {
      setProblem(asCommandError(raw).message);
    }
  }

  /**
   * Change a setting that is not part of the gate.
   *
   * Same shape as `retune` and the same rollback, split out because a gate
   * patch and a top-level one merge at different depths and a single function
   * taking both would be a function taking a shape nobody can read.
   */
  async function reset(patch: Partial<AudioSettings>) {
    const current = saved.current;
    if (current === null) return;

    const next: AudioSettings = { ...current, ...patch };
    setSettings(next);
    saved.current = next;

    try {
      await setAudioSettings(next);
    } catch (raw: unknown) {
      setSettings(current);
      saved.current = current;
      setProblem(asCommandError(raw).message);
    }
  }

  /**
   * The same as [`reset`], for a control that fires while it is being dragged.
   *
   * A slider produces an event per pixel, and every one of them is a settings
   * file rewritten. So the picture and the mixer move immediately, and the
   * write waits for somebody to stop moving.
   *
   * The delay is short enough not to be a delay. What it must not do is
   * outlast the settings screen being closed, which is why the timer is
   * cancelled on unmount and the pending value written out.
   */
  function slide(patch: Partial<AudioSettings>) {
    const current = saved.current;
    if (current === null) return;

    const next: AudioSettings = { ...current, ...patch };
    setSettings(next);
    saved.current = next;
    pending.current = next;

    if (writing.current !== null) clearTimeout(writing.current);
    writing.current = setTimeout(() => {
      writing.current = null;
      void flush();
    }, 150);
  }

  /**
   * The same as [`slide`], for a slider that is part of the gate.
   *
   * Two functions rather than one taking both shapes, for the reason `reset`
   * and `retune` are two: a gate patch and a top-level one merge at different
   * depths, and a single function taking either would take a shape nobody can
   * read.
   *
   * `closeAt` is held at or below `openAt` here rather than left to the
   * person dragging. Above it the two thresholds stop being hysteresis: the
   * gate opens and shuts on the same wavering score several times a second,
   * which is the exact stutter the pair exists to prevent, and no arrangement
   * of the two sliders makes it a useful thing to ask for.
   */
  function slideGate(patch: Partial<GateConfig>) {
    const current = saved.current;
    if (current === null) return;

    const gate = { ...current.gate, ...patch };
    if (gate.closeAt > gate.openAt) {
      // Whichever one was not just moved gives way, so dragging either of them
      // pushes the other rather than refusing to move.
      if (patch.openAt === undefined) gate.openAt = gate.closeAt;
      else gate.closeAt = gate.openAt;
    }

    const next: AudioSettings = { ...current, gate };
    setSettings(next);
    saved.current = next;
    pending.current = next;

    if (writing.current !== null) clearTimeout(writing.current);
    writing.current = setTimeout(() => {
      writing.current = null;
      void flush();
    }, 150);
  }

  async function retune(patch: Partial<GateConfig>) {
    const current = saved.current;
    if (current === null) return;

    const next: AudioSettings = {
      ...current,
      gate: { ...current.gate, ...patch },
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

  /**
   * Start or stop playing the microphone back.
   *
   * Nothing is set here either. The button follows `listening`, which comes
   * off the audio thread's own events, so a press that reached a machine with
   * no working output leaves the button where it was and puts the reason on
   * screen.
   */
  function listen() {
    const ask = listening === null ? audioMonitorStart : audioMonitorStop;
    ask().catch((raw: unknown) => {
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
        {/*
          The other half of the meter. A bar says the gate opened; this says
          what came out of it, which is the only way to hear a threshold set
          too high clipping the front of every sentence.
        */}
        <button
          type="button"
          className="voice-field__check"
          aria-pressed={listening !== null}
          onClick={listen}
        >
          {listening === null ? "Listen" : "Stop listening"}
        </button>
        <p className="voice-field__note">
          {listening === null
            ? "Play your microphone back through your speakers, exactly as a call would carry it: gated, denoised, and delayed by the same fraction of a second."
            : `Playing back through ${listening}. Use headphones, or your microphone will hear this and go round again.`}
        </p>
      </div>

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
              onChange={(event) =>
                void retune({ voiceActivity: event.target.checked })
              }
            />
            <label className="voice-toggle__label" htmlFor="voice-activity">
              Voice activity detection
            </label>
          </div>
          <p className="voice-field__note" id="voice-activity-note">
            Send audio only while you are speaking. Turn it off to transmit
            continuously, background noise included.
          </p>
          {/*
            Only while the gate is on, because these four are what it is tuned
            with and it ignores every one of them when it is off. Drawing them
            there would be four controls that do nothing.

            Live: the gate is retuned where it stands rather than restarted, so
            the meter above keeps moving and simply starts behaving
            differently, which is the clearest demonstration of what a
            threshold is.
          */}
          {settings.gate.voiceActivity && (
            <>
              <VolumeSlider
                id="voice-gate-open"
                label="Starts sending at"
                percent={Math.round(settings.gate.openAt * 100)}
                describedBy="voice-gate-open-note"
                onChange={(percent) => slideGate({ openAt: percent / 100 })}
              />
              <p className="voice-field__note" id="voice-gate-open-note">
                How sure Consort has to be that the sound is a voice before it
                sends anything. Lower it if the beginnings of your words are
                being cut off, or if a quiet microphone is not opening the gate
                at all. Raise it if a fan or a keyboard is being sent as speech.
              </p>
              <VolumeSlider
                id="voice-gate-close"
                label="Stops sending below"
                percent={Math.round(settings.gate.closeAt * 100)}
                describedBy="voice-gate-close-note"
                onChange={(percent) => slideGate({ closeAt: percent / 100 })}
              />
              <p className="voice-field__note" id="voice-gate-close-note">
                Kept below the value above, and the gap between them is what
                stops a score hovering on the line from chattering the gate
                open and shut mid-word. Widen the gap if that is what you are
                hearing.
              </p>
              <VolumeSlider
                id="voice-gate-hold"
                label="Keeps sending for"
                percent={settings.gate.holdMs}
                min={0}
                max={2000}
                step={50}
                format={(ms) => `${ms} ms`}
                describedBy="voice-gate-hold-note"
                onChange={(ms) => slideGate({ holdMs: ms })}
              />
              <p className="voice-field__note" id="voice-gate-hold-note">
                How long the gate stays open after you stop. Long enough to
                carry the pause between two words, short enough that the room
                is not sent afterwards.
              </p>
              <VolumeSlider
                id="voice-gate-attack"
                label="Ignores sounds under"
                percent={settings.gate.attackFrames}
                min={1}
                max={10}
                step={1}
                format={(frames) => `${frames * FRAME_MS} ms`}
                describedBy="voice-gate-attack-note"
                onChange={(frames) => slideGate({ attackFrames: frames })}
              />
              <p className="voice-field__note" id="voice-gate-attack-note">
                A door or a cough is louder than a voice and much shorter, so
                the gate waits to be sure. Nothing is lost while it waits:
                those frames are held back and sent once it opens, which is why
                raising this cuts out taps without clipping your first
                syllable.
              </p>
            </>
          )}
        </div>
      )}

      {settings !== null && (
        <div className="voice-field">
          <span className="voice-field__label">Noise suppression</span>
          <div className="voice-toggle">
            <input
              id="voice-denoise"
              className="voice-toggle__switch"
              type="checkbox"
              role="switch"
              aria-describedby="voice-denoise-note"
              checked={settings.gate.denoise}
              onChange={(event) => void retune({ denoise: event.target.checked })}
            />
            <label className="voice-toggle__label" htmlFor="voice-denoise">
              Remove background noise
            </label>
          </div>
          <p className="voice-field__note" id="voice-denoise-note">
            Strips fans, keyboards and room tone out of what you send. Separate
            from the switch above: turning voice activity detection off still
            leaves this running, and this is the one to turn off to hear your
            microphone untouched.
          </p>
        </div>
      )}

      {settings !== null && (
        <div className="voice-field">
          <span className="voice-field__label">Call volume</span>
          <VolumeSlider
            id="voice-output-volume"
            label="Everybody in a call"
            percent={settings.outputVolume ?? 100}
            describedBy="voice-output-volume-note"
            onChange={(percent) => slide({ outputVolume: percent })}
          />
          <p className="voice-field__note" id="voice-output-volume-note">
            Everybody in the call and the announcements below them. To change
            one person on their own, right-click their name in the channel.
          </p>
        </div>
      )}

      {settings !== null && (
        <div className="voice-field">
          <span className="voice-field__label">Call sounds</span>
          <div className="voice-toggle">
            <input
              id="voice-call-sounds"
              className="voice-toggle__switch"
              type="checkbox"
              role="switch"
              aria-describedby="voice-call-sounds-note"
              checked={settings.callSounds === true}
              onChange={(event) =>
                void reset({ callSounds: event.target.checked })
              }
            />
            <label className="voice-toggle__label" htmlFor="voice-call-sounds">
              Chime as well, before the announcement
            </label>
          </div>
          <p className="voice-field__note" id="voice-call-sounds-note">
            Off to begin with, because the announcement below already says
            somebody arrived and a chime in front of it is a doorbell before
            somebody who is already talking. Turn it on for a two-part sound,
            or turn the announcement off to have the chime on its own.
          </p>
        </div>
      )}

      {settings !== null && (
        <div className="voice-field">
          <span className="voice-field__label">Spoken notifications</span>
          <div className="voice-toggle">
            <input
              id="voice-call-voices"
              className="voice-toggle__switch"
              type="checkbox"
              role="switch"
              aria-describedby="voice-call-voices-note"
              checked={settings.callVoices !== false}
              onChange={(event) =>
                void reset({ callVoices: event.target.checked })
              }
            />
            <label className="voice-toggle__label" htmlFor="voice-call-voices">
              Say out loud when somebody joins or leaves
            </label>
          </div>
          <p className="voice-field__note" id="voice-call-voices-note">
            Only for the voice channel you are in, and only for people other
            than you. Arriving and leaving are recorded; coming back from away
            is not, so that one stays silent for now.
          </p>
          <VolumeSlider
            id="voice-notification-volume"
            label="Announcement volume"
            percent={settings.notificationVolume ?? 60}
            describedBy="voice-notification-volume-note"
            onChange={(percent) => slide({ notificationVolume: percent })}
          />
          <p className="voice-field__note" id="voice-notification-volume-note">
            The chime and the announcement together, measured against the call
            volume above rather than separately, so turning a call down turns
            these down with it. Lower than everything else to begin with: an
            announcement is recorded to be heard on its own, and a call is
            somebody talking three feet from a microphone.
          </p>
        </div>
      )}

    </div>
  );
}
