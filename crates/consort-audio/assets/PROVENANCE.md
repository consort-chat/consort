# Where these sounds came from

Five files ship in the binary, embedded with `include_bytes!` from
`crates/consort-audio/src/sound.rs`. This is what each one is and where it came
from, because a shipped audio asset with no recorded origin is a licensing
question somebody has to answer later without the facts.

## `voice/entered.mp3`, `voice/left.mp3`

Synthetic speech, generated with ElevenLabs by the project owner on
2026-08-29. Mono, 44.1 kHz, about 1.6 seconds each.

They recreate the idea of the spoken join and leave notifications that
TeamSpeak's default sound pack used, which is the behaviour this feature is
modelled on. They are **not** TeamSpeak's files, and nothing was extracted,
sampled or transcoded from that product. They are new audio that says a similar
thing, in a different voice, made for this repository.

Worth keeping straight if the question ever comes up: the resemblance is to a
convention (a voice announcing arrivals, which predates TeamSpeak and is not
original to it), not to a recording. "TeamSpeak" is somebody else's trademark
and is used here only to describe what the feature imitates, never as a claim
of origin or endorsement.

## `voice/welcome-back.mp3`

One second of silence. A placeholder, and the only one left. There is no
recording for the sentence said to somebody returning from away, so the
mechanism runs and plays nothing. `sound.rs` has a test that fails the moment
this stops being silent, so it cannot be replaced quietly.

## `join.mp3`, `leave.mp3`

Placeholders, generated rather than recorded: a two-tone fifth, C5 to G5 rising
on arrival and falling on departure, 0.3 seconds each.

These are what phase 8 of `docs/PLAN-call-presence.md` is about replacing, and
`sound.rs` says why in its own header: a fifth a computer derived sounds like a
modem, and a fifth somebody chose sounds like a product. When they are replaced,
the replacement's origin belongs in this file beside the rest.

## What a replacement has to satisfy

Anything dropped into these paths is picked up by the next `cargo build`, with
no code change, because `include_bytes!` is tracked as a build dependency. The
decoder is deliberately forgiving: any sample rate (it resamples), mono or
stereo (it averages the channels), and a file that will not decode logs a
warning and plays silence rather than panicking inside somebody's call.

The tests in `sound.rs` hold the rest. A chime is 0.1 to 1.0 seconds and peaks
below 12000. A phrase is 0.5 to 3.0 seconds and is measured by RMS rather than
peak, because speech is mostly quiet with short loud consonants, and it has to
sit no more than twice the level of the chime that plays in front of it.
