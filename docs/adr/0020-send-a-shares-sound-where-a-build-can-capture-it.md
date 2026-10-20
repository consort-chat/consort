# ADR-0020: Send a share's sound on its own track, where a build can capture it

**Date**: 2026-10-08
**Status**: accepted
**Deciders**: tominal, with Claude Code

Numbered 20 rather than 16 because #198, #199, #200 and #201 hold 16 through
19 and none of them is on main yet.

## Context

Issue #193: a shared screen or window goes out without its sound, and there is
no control for sharing sound at all. Two asks, and the second depends on the
first: a switch for a capture a platform cannot do is a switch somebody turns
on and watches do nothing.

So the question to settle first is what each platform can actually capture, and
whether a second audio track can be published at all.

### A share's sound cannot go on the microphone's track

This is the constraint everything else follows from. Mixed into the microphone,
a share's sound can never be separated again by anybody receiving it:

- a listener who turns the sharer down, or mutes them, loses the share with
  them;
- the sharer's own voice gate chops a shared video into fragments, because the
  gate is tuned for speech and denoises everything it passes;
- the microphone is in the same room as the speakers, so what goes out comes
  back in and goes out again.

MatrixRTC already has the right shape for this. `MediaStreamKind::ScreenShareAudio`
is a stream kind of its own, and matrix-rtc-livekit's `publish` already maps it
to `TrackSource::ScreenshareAudio` on the same branch it publishes a microphone
on. Nothing upstream needed changing: `PublishOptions` carries the kind and an
`AudioSourceConfig`, and a session may hold both audio publications at once.

### What each platform can capture

**Windows: yes, and for free.** cpal's WASAPI backend sets
`AUDCLNT_STREAMFLAGS_LOOPBACK` whenever it builds an *input* stream on an
endpoint that renders. So opening an output device for input is a loopback
capture of everything that endpoint is playing, with no new dependency and no
raw COM. Two details that are not obvious: `supported_input_configs` on a
renderer is empty by construction, so the format has to come from
`default_output_config`, which is the endpoint's mix format; and a shared-mode
loopback client is initialised with that mix format or not at all.

It is the **whole endpoint**, not one process. Per-application loopback on
Windows exists, through `ActivateAudioInterfaceAsync` with
`VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`, but cpal does not expose it and
reaching it means hand-written COM. That is separate work, and it is what
somebody sharing one window would expect, so it is worth doing later.

Windows.Graphics.Capture carries no audio at all. `wgc_host.rs` is not where
any of this belongs.

**Linux: no, not through anything already here.** X11 screen capture has no
audio in it, which is not a surprise. The surprise is that the audio side has
none either. cpal's only Linux host is ALSA, and PulseAudio and PipeWire
monitor sources are not ALSA PCMs. Measured on an ordinary PipeWire desktop
with four monitor sources listed by `pactl`, cpal's input device list contains
none of them:

```
$ pactl list short sources | grep monitor
alsa_output.pci-0000_01_00.1.hdmi-stereo.monitor
alsa_output.pci-0000_73_00.6.analog-stereo.monitor
easyeffects_sink.monitor
...
$ # cpal's input devices, same machine
Discard all samples, PipeWire Sound Server, PulseAudio Sound Server,
Default ALSA Output, HD-Audio Generic ALC897 Analog, ...
```

The `pulse` PCM cpal does list opens the default *source*, which is the
microphone. Its source can be redirected with `PULSE_SOURCE`, but that is
process-wide and would hijack the microphone with it.

So Linux share audio needs a client of PipeWire or libpulse, which is a second
audio backend rather than a branch in this one. It is also the platform where
per-application capture is the easier half: PipeWire can link to one node's
monitor ports without disturbing anybody's routing, where PulseAudio can only
do it by moving a stream to a null sink.

**macOS: moot.** `Screens` is `NoScreens` there, so there is no share to carry
sound for. cpal does have a CoreAudio loopback device, for whenever that
changes.

## Decision

Send a share's sound as a second audio publication, of kind
`ScreenShareAudio`, fed by a capture seam with one implementation.

- `consort_audio::ShareSound` is the seam, with `NoShareSound` as the answer
  everywhere but Windows. It reports `available()` before anything is drawn,
  and refuses an open by naming the build rather than a missing device.
- `CpalShareSound` is the Windows one, in `cpal_host.rs` beside the
  microphone, and excluded from coverage with the rest of that file.
- `VideoSettings::share_sound` is the switch, off by default, and the settings
  screen draws it only where `share_sound_available` says yes.
- A sound capture that will not open is reported as a share without sound, not
  as a share that failed. `SelfScreen::sound` carries what actually happened.

Receiving one is not platform-gated and is not optional: `hearing::Sound` is
the two incoming audio streams as a value, and `listen` makes a pass for each,
the way `watch` already does for the two pictures. A share gets a mixer key of
its own rather than its sender's, because one queue is a jitter buffer and two
streams interleaved into it play as noise. Only the voice key is attributed to
a person, which is what keeps a shared screen's sound off the per-person volume
and out of who-is-talking.

## Consequences

Somebody on Windows can share a window with its sound, and everybody in the
call hears it on a track they can turn down separately from the sharer's voice.
Somebody on Linux sees no switch, and that is the honest answer rather than a
control that does nothing.

Receiving works on every platform from the moment this lands, including from an
Element Call peer sharing a tab with audio, which is how it is hand-verified on
a Linux machine where nothing can be captured.

Two pieces of work are named rather than done:

- a PipeWire or libpulse backend, so Linux can send one at all;
- per-process loopback on Windows, so sharing one window sends that window
  rather than the whole output. Until then the switch says so.

System audio is also not the only thing a share could carry. Nothing here
changes the microphone, and the two go out together as they always did.
