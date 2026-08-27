# Plan: settings, and an input that can hear you

Presence answered "who is in there". This answers "will they hear me when I
go in", which has to be true before joining is worth building.

The shape is Discord's, because it is the shape a hand already knows. The
sign-out button in the bottom-left strip becomes a settings icon. It opens a
modal. The modal has a Voice & Video section with an input device, an output
device, and a meter that moves when you talk and shows the voice gate opening
and closing. Devices are chosen for you at boot and can be changed.

No call is joined in this phase. Nothing here needs libwebrtc.

## What has to be true by the end

1. The bottom-left strip has a settings icon where the sign-out button was.
2. Clicking it opens a modal. Escape closes it. Focus goes in on open and
   comes back to the icon on close.
3. Voice & Video lists the real input and output devices on this machine.
4. Something sensible is already selected on first run, without being asked.
5. The choice survives a restart.
6. Talking into the selected microphone moves a level meter, and the gate
   indicator opens on speech and closes on silence.
7. A test tone plays out of the selected output device on request.
8. Signing out is still reachable.

Acceptance is by hand, at a real microphone: pick each input in turn, talk,
watch the meter follow the right device.

## What already exists

`~/Documents/matrix-rtc-vad-spike/vad-core` is 859 lines that answer the hard
half of this. It is cpal plus nnnoiseless and nothing else, it builds in
seconds, and it has already been tuned by ear against a real call.

| Piece | State |
| --- | --- |
| `gate.rs` | RNNoise probability plus two-threshold hysteresis with attack and hold. Port as is. |
| `capture.rs` | cpal input capture, 48 kHz mono, i16 and f32 formats, device listing. Port and extend. |
| `bin/meter.rs` | A terminal meter. The reference for what the UI draws, then delete. |
| `tests/scaling.rs` | The i16-range scaling trap, tested. Port. |

Four things the spike's README already learned the expensive way, all of which
carry over unchanged:

- **Nothing resamples.** RNNoise takes 480 samples at 48 kHz and nothing else,
  which is exactly the 10 ms frame the media layer wants. A device that cannot
  do 48 kHz is a hard error, not a silent quality loss.
- **nnnoiseless wants f32 in i16 range**, not `[-1.0, 1.0]`. Dividing by 32768
  is the reflex and it hands the model a signal 90 dB too quiet, which reads as
  permanent silence.
- **Two thresholds, not one.** A single threshold chatters and clips the front
  of every word.
- **A shut gate publishes silence rather than stopping.** Not this phase's
  problem, but it is why the gate returns a decision rather than an
  `Option<frame>`.

## What is genuinely new

- **Output devices.** `vad-core` enumerates inputs only and never plays
  anything. Output listing and a test tone are new code.
- **Settings instead of environment variables.** `GateConfig::from_env` is a
  spike affordance. The config becomes a serialisable struct with a file
  behind it.
- **A device that is no longer there.** A saved device can be unplugged
  between runs. Falling back silently is wrong; falling over is worse.
- **A modal.** `role="dialog"` does not appear anywhere in this codebase yet.
- **A thread that owns the audio.** cpal's `Stream` is `!Send`, so it cannot
  live in `AppState` or be held across an await in a Tauri command.

That last one is worth dwelling on, because it is the same constraint
`state.rs:55` records for `Call::join`. Both need a thread owning a
current-thread world, reached over a command channel. Building it here, for a
meter that is easy to reason about, is much cheaper than discovering its shape
halfway through wiring up a call. This phase pays down that risk early.

## The shape

```
crates/consort-audio/          new. cpal + nnnoiseless, no matrix-sdk,
                               no libwebrtc, builds in seconds.
  gate.rs                      ported from the spike
  devices.rs                   the catalogue, behind a trait
  capture.rs                   ported and extended
  playback.rs                  new: the test tone
  settings.rs                  what gets persisted
  thread.rs                    the !Send island and its command channel
app/src-tauri/src/commands.rs  audio_devices, audio_settings,
                               set_audio_settings, audio_test_*
app/src/components/Settings*   the modal and its sections
```

The crate split is the spike's, for the spike's reason: the tuning loop is
iterate-by-ear, and it must not sit behind a matrix-sdk rebuild.

**The audio backend goes behind a trait.** CI has no sound card, so
`cpal::default_host()` in the middle of a function is a function no test can
run. `EventSink` in `events.rs` already establishes this pattern in this
codebase: a trait so that everything holding one stays testable, with the real
implementation thin enough to exclude from coverage. Audio gets the same
treatment, and `CpalDevices` joins `keyring_store.rs` in the coverage regex.

## Phases

Each phase is red first. The test that cannot be written without hardware is
the test that says so and gets deferred to the by-hand phase at the end.

### Phase 1: the gate, ported, configured rather than environed (done)

**Red.** Port `tests/scaling.rs` unchanged, since the i16-range trap is the one
mistake most likely to be reintroduced. Then the hysteresis tests the spike
never wrote:

- a single frame above `open_at` does not open the gate
- `attack_frames` consecutive frames above `open_at` does
- a frame below `close_at` does not close it while hold time remains
- it closes once `hold_ms` has elapsed below `close_at`
- `opened` and `closed` fire on the edges only, never twice
- probability is passed through untouched by hysteresis
- `denoise: false` still gates identically and only the samples differ
- the first frame is dropped, because RNNoise's first output is an artifact

**Green.** Port `gate.rs`. Delete `from_env`. `GateConfig` gains `Serialize`
and `Deserialize`.

This phase is pure arithmetic over a fixed model, so it should reach 100% and
carry the crate's coverage while the hardware-facing parts cannot.

### Phase 2: the device catalogue, behind a trait (done)

**Red.** Against a fake catalogue, never cpal:

- a saved device that is present is the one chosen
- a saved device that is gone falls back to the default and reports that it
  did, so the UI can say so rather than quietly listening to the wrong thing
- no devices at all is a named error, not an empty list treated as success
- duplicate names are deduplicated while keeping host order, because ALSA
  reports one card under several plugin wrappers and the first is the likely
  default
- an empty saved name is treated as unset rather than as a device named ""

**Green.** `trait AudioDevices` with `inputs()` and `outputs()`, a pure
`resolve()` over its output, and a `CpalDevices` implementation that is
excluded from coverage.

**Known weakness, stated rather than solved.** cpal 0.18 removed
`Device::name()` and offers only `Display`, so a name is all the identity
there is. Two identical capture cards are indistinguishable. Accepted for now.

### Phase 3: settings that survive a restart (done)

**Red.**

- round-trips through serde unchanged
- a missing file yields defaults and is not an error, since first run is the
  common case
- an unknown field is ignored rather than fatal, so a settings file from a
  later version does not brick an earlier one
- a corrupt file yields defaults, warns, and does **not** delete or overwrite
  the user's file until they change something
- writing is atomic, so a crash mid-write cannot leave a truncated file

**Green.** `settings.json` in `app_data_dir`, beside `session.json`, written
through the existing `atomic::write_private`. Not because thresholds are
secret, but because the helper is already correct about fsync ordering and
writing a second, worse one would be silly.

### Phase 4: the thread that owns the audio (done)

**Red.** Against the fake backend, over the command channel:

- start delivers frames
- stop stops them
- switching device while running tears down the old stream before the new one
- stop with nothing running is a no-op, not a panic
- dropping the handle stops the thread and the stream
- a device that fails to open reports the error back rather than killing the
  thread

**Green.** A thread, an mpsc of commands, and an `AudioHandle` holding only the
sender. `AppState` holds the handle.

### Phase 5: the meter reaches the webview (done)

**Red.** The gate runs at 100 frames a second. 100 Tauri events a second, each
a JSON round trip, to move a bar that redraws at 60 Hz at best, is waste.

- a batch of frames becomes one event at roughly 20 Hz
- the event carries the **peak** level in the batch, not the last, or a
  transient vanishes between samples
- it carries the **maximum** probability in the batch, for the same reason
- gate-open is sticky across a batch: open at any point in the window reads as
  open, because a bar that flickers off mid-word looks broken
- no frames means no event, rather than a stream of zeroes

**Green.** `AppEvent::AudioLevel`, with `is_worth_keeping` returning
**false**. A level is not state. Replaying the last one to a remounting webview
would draw a moving bar for a stream that stopped minutes ago, which is the
opposite of what that replay mechanism exists for.

Commands: `audio_devices`, `audio_settings`, `set_audio_settings`,
`audio_test_start`, `audio_test_stop`.

### Phase 6: the modal (done)

**Red, frontend first.** The modal is a new primitive here and gets tested as
one before anything is put inside it:

- opens on the settings icon, closes on Escape
- closes on overlay click, but not on a click inside the panel
- `role="dialog"` and `aria-modal="true"`
- focus moves into the panel on open
- focus returns to the settings icon on close, not to the body
- Tab from the last focusable element wraps to the first
- the rest of the app is `inert` or `aria-hidden` while it is open

Then the panel:

- the settings icon has an accessible name, since an icon alone has none
- Voice & Video lists inputs and outputs from `audio_devices`
- choosing one calls `set_audio_settings` with that device
- the meter runs only while Voice & Video is showing
- leaving the section or closing the modal stops it, and unmounting does too
- a saved device that is gone renders the fallback notice from Phase 2
- no devices at all renders a real message, not an empty select

**Green.** `Settings.tsx`, `SettingsModal.tsx`, `VoiceVideoSection.tsx`,
`LevelMeter.tsx`.

**Sign out has to go somewhere.** Discord puts it in User Settings under My
Account, and the icon replacing it is the whole point of this phase. Proposing
a My Account section holding the user ID, the device ID, and Log Out. Worth
confirming, since it makes signing out two clicks instead of one.

### Phase 7: proving the output device

An input can be verified by talking. An output cannot be verified by anything
already in this plan, because nothing plays audio yet. Without this phase, the
output picker is a control with no feedback, which is a control nobody can
trust.

**Red.** Playback against the fake backend: the tone starts, stops, targets the
chosen device, and stops when the modal closes.

**Green.** A short tone through the selected output, behind a button. Discord
calls it "Let's Check".

### Phase 8: by hand, at real hardware

The part CI cannot do:

- every input in the list, in turn, talking each time and watching the meter
  follow the right one
- unplug the selected device while the meter runs
- restart with a device unplugged and confirm the fallback notice
- a device that cannot do 48 kHz, if one is available, and confirm it fails
  with the reason rather than silently
- the test tone out of each output
- talk, and tune `open_at` and `close_at` from what the meter shows

## Risks

**CI has no audio device.** The whole reason for the trait. If cpal calls leak
outside `CpalDevices`, the test suite stops running on the runner and the
failure will look like a hang, not an error.

**Coverage.** The hardware implementations are untestable and are real lines.
They join `keyring_store.rs` in `--ignore-filename-regex`. If the pure logic
does not carry the crate on its own, the 90% floor fails, and the fix is more
logic behind the trait rather than a lower floor.

**Device names are weak identity.** Stated in Phase 2 and accepted.

**48 kHz is a hard requirement.** Correct, and it will eventually meet a device
that cannot do it. The UI must say which device and why, not just fail.

**Event rate.** Phase 5 exists because of it. Worth watching in the dev build
regardless.

**Disk.** `/home` is at 98% with 11G free. This phase adds cpal and
nnnoiseless, which are small, and needs no libwebrtc at all. It is safe to
start. The join phase after it is not, and that is when the 13G in
`target/llvm-cov-target` has to go.

## Found while building it

**ALSA's device list is mostly not devices.** This machine reports 21 inputs and
27 outputs. Twelve of each are plugin wrappers: three rate converters, a Speex
DSP chain, channel upmix and downmix, and the null sink. A picker that long,
mostly plumbing, is a picker people scroll past instead of read.

Filtering them is a decision, so it lives in `devices.rs` behind tests rather
than in the cpal file. The filter matches how ALSA names its wrappers rather
than any use of the word, because dropping somebody's actual microphone would
be much worse than leaving a resampler in the list.

That takes the input list from 21 to 14, and the rest is a judgement call best
made looking at the real UI:

- `Yeti Stereo Microphone, USB Audio` and `Yeti Stereo Microphone` are the same
  hardware under two ALSA names. Both are real and neither is obviously the one
  to keep.
- `JACK Audio Connection Kit` and `Open Sound System` are listed on a machine
  running neither, so selecting them would fail. Hiding them would be wrong for
  somebody who does run JACK.
- `Default ALSA Output (currently PipeWire Media Server)` appears in the
  **input** list, which reads as a mistake even though it is not.

Left alone for now, and worth deciding once Phase 6 makes it visible.

**Most of the devices ALSA lists cannot actually be opened.** Found by pointing
the capture path at this machine's real hardware, one device at a time:

| Device | What happens |
| --- | --- |
| `PipeWire Sound Server` | Opens every time, real audio, sensible noise floor |
| `PulseAudio Sound Server` | Opens |
| `Yeti Stereo Microphone` | `The requested audio device is not available` |
| `Yeti Stereo Microphone, USB Audio` | Opens sometimes, otherwise `unable to open slave` |
| `Default Audio Device` | Opens, then delivers digital silence, or refuses as busy |

The pattern is that the raw ALSA hardware entries go through `dsnoop` to a card
PipeWire is already holding, so they fail or hand back nothing. The sound-server
entries work.

The awkward part is the last row. `Default Audio Device` is what cpal reports as
the host default, so it is what a first run would choose, and on this machine it
is the worst of the fourteen. That collides with the fourth thing this plan says
has to be true by the end: something sensible is already selected on first run.

**Decided: no heuristic. The host's default is the default, everywhere.**

Two reasons, and the first is that the premise was wrong. `Default Audio Device`
delivers silence on this machine because EasyEffects is already holding the
input and running its own noise model over it. That is one desktop's
configuration, not a fact about defaults, and a ranked preference list written
to route around it would be Arch-shaped code shipped to Windows and macOS, where
the host default is simply correct and is what every other application uses.

The second is that `Selection::name_to_open` already covers the part that
generalises. Nothing saved means the backend is asked for *its* default rather
than for the name that default currently has, so the choice follows the machine:
plug in a headset on Windows and the microphone moves, which is what somebody
who never opened this screen expects. A saved name is a photograph; the host's
default is a live answer.

What is built alongside it is the reporting, because opening a device fails
often on a real desktop and that is a state to draw rather than an exception: a
failed open carries the backend's own words, and `Substituted` names the device
that went away. Both have tests.

**Decided: list only devices we have confirmed we can open.**

The first real machine to open this screen offered twenty audio outputs,
including a webcam and two microphones, and fourteen inputs including four
HDMI monitors. None of that was invented here. cpal's ALSA backend takes each
device's direction from the `IOID` field of its PCM hint, and cpal's own source
notes beside that code that a hint leaves the field NULL to mean either, so
"this is an output" is a declaration and not a fact. On a PipeWire desktop the
declarations are wrong often enough to be embarrassing.

So `enumerate` now asks each candidate what it supports and keeps only those
offering something at 48 kHz in the direction being listed. That is the same
requirement `AudioCapture::open` enforces a moment later, which is the point: a
picker should never offer a device that choosing it would fail on. Measured on
that machine it took inputs from fourteen to eight and outputs from twenty to
fourteen, and every entry that went was one that could not have been opened.

Stated as a rule rather than as a platform branch, and deliberately. WASAPI and
CoreAudio enumerate real endpoints and lose nothing to this; the check costs
them a cheap query. ALSA pays about 300 ms for a whole namespace because
answering means opening each device, which is also why the picker now shows
what was chosen straight away instead of waiting for the list to come back.

The one thing it gives up: a device held exclusively by another application can
report nothing and be dropped from the list. That is the right trade, because
while it is held, choosing it would not have worked either.

The list questions above stay open, and they are cosmetic. Nothing selects a
JACK entry on a machine without JACK unless a person picks it themselves.

## What this deliberately does not do

- No joining a call, and no publishing anywhere.
- No playback of remote participants, and so no mixer.
- No push to talk, no per-user volume, no input sensitivity automation.
- No video, despite the section being called Voice & Video. The name is
  Discord's and the video half stays empty.
- No settings beyond audio. Appearance, notifications and keybinds are a
  different piece of work that this modal makes possible rather than includes.
