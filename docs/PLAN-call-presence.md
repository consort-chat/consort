# Plan: what a call sounds like and who is in it

Status: **sounds and away are done. Remote deafen is still
built-but-unconfirmed. Sections 4 and 5 are asked for and not started.**

Three additions to a call that is otherwise built.
[PLAN-voice-presence.md](PLAN-voice-presence.md) drew who is sitting in a voice
channel and said, correctly, that "who is speaking, muted, or deafened" needed
the SFU connection and belonged to the connect half. This is that half.

None of it blocks [PLAN-call-encryption.md](PLAN-call-encryption.md), and that
one blocks the release. This is what makes a call feel like a call rather than
a connection that happens to carry audio.

## 1. Sounds when somebody arrives and leaves

A voice channel with no sound on arrival makes people talk over each other,
because the only way to know somebody joined is to be looking at the right
corner of the screen when they do.

### Where they play

Not through `AudioPlayback::play`. That opens a second output stream beside the
call's, which is documented as allowed and is right for the settings-screen
chime, but a join sound fires often and opening a device costs tens of
milliseconds. Doing that repeatedly under a live call is asking a backend to
glitch the thing it is glitching around.

They mix into the call's own output instead, which is exactly what `Voices`
already does for people. `Voices::mix` sums every queue into one buffer and the
caller clamps once at the end, so a sound is structurally another voice.

It cannot be *literally* another voice, because a person's queue is capped at
`JITTER_SAMPLES` (12 frames, 120 ms) and drops the oldest when full. That cap is
right for speech, where late audio is worthless, and wrong for a 400 ms sound,
which would arrive as its own last 120 ms. So `Voices` grows a second queue
beside the people:

- `Voices::play(samples)` appends to it. Appending rather than replacing, so two
  people arriving at once are two sounds in sequence rather than one sound
  played twice on top of itself.
- Capped at two seconds. A burst of arrivals must not queue a minute of chiming.
- `mix` sums it alongside everybody, into the same `i32` accumulator.

A sound therefore plays out of the device the call is already coming out of,
follows the same output-device setting, and needs no second stream.

### What they are

`crates/consort-audio/src/sound.rs`. Decoded PCM, mono, 48 kHz `i16`, which is
what `Voices` takes.

MP3, decoded with `symphonia` (`default-features = false, features = ["mp3"]`),
pure Rust and MPL-2.0. Decoded lazily on first play into a `OnceLock<Arc<[i16]>>`
and shared thereafter, because the same three files are decoded on every launch
otherwise for no reason.

Files are `include_bytes!`d rather than read from disk. A sound that depends on
an install path is a sound that is missing on somebody else's machine, and these
are a few kilobytes each.

Resampling is linear and only runs when the file is not already 48 kHz. The
shipped files are 48 kHz mono so it never runs for them; it exists because
somebody replacing a file with their own should get a sound rather than a
chipmunk.

### What triggers them

The call thread already emits `CallEvent::Connected { participants }` on every
roster change. The diff against the previous roster is the trigger: somebody in
the new list who was not in the old is an arrival, and the reverse is a
departure.

Three rules that are not obvious and are each a bug if missed:

- **The first `Connected` of a call announces nobody.** Joining a channel with
  four people in it must not play four arrival sounds.
- **This session's own arrival and departure are not announced.** The person who
  pressed the button knows.
- **A channel switch is a leave and a join, not a roster diff.** The old call's
  participants are not people who left; they are people this session stopped
  being with.

### Muting them

A setting, in the existing call settings block, defaulting to on. Somebody in a
channel with a lot of churn will want it off within an hour.

Deafening silences them too. Deafening means "I have stopped listening to this
call", and a chime is part of the call.

## 2. Deafen, visible to other people

Mostly built already. `crates/consort-call/src/notices.rs` carries a `Notice`
over a LiveKit data message on the `consort.self_audio` topic, `Deafened` records
who is currently deafened, `LiveKitSession::announce` publishes it on every
roster change as well as on the button, and `ChannelList` draws a `DeafenedIcon`.

What is missing is confirmation. Neither half has been seen working against a
second live client: not this session's own icon, and not a second Consort
drawing it. Both are testable in one sitting with
`CONSORT_PROFILE=second WEBKIT_DISABLE_DMABUF_RENDERER=1 pnpm tauri dev`.

That is the whole of this item. It is listed because "built" and "confirmed" are
different claims and only one of them has been earned.

## 3. Away

TeamSpeak's away flag, which is the best small feature that program had: one
button, and everybody else sees a clock over your avatar and stops wondering why
you are not answering.

### What it is

A third self-audio state beside muted and deafened.

- It **mutes** the microphone. Somebody away from the computer is not talking,
  and a live microphone in an empty room is the thing away exists to prevent.
- It does **not** deafen. Audio keeps arriving and keeps playing. That is the
  entire difference from deafen, and it is the point: you can walk away, hear
  your name, and come back.
- It is **visible to everybody**, which is what makes it worth having at all. A
  purely local away flag is just mute with extra steps.

Orthogonal to `muted`, exactly as `deafened` already is. `SelfAudio::microphone_off`
becomes `muted || deafened || away`, and pressing the microphone button toggles
`muted` alone. Somebody away who presses unmute stays muted, and the button still
reads as off, which is the same bargain the deafen button already makes and is
already documented there.

### How it travels

`notices::Notice` gains `away: bool`. **Not a version bump.** `VERSION` stays
`1`, the field is `#[serde(default)]`, and `Notice` does not
`deny_unknown_fields`. So a build without this field reads a notice carrying it
and still gets `deafened` right, and a build with it reads an older notice as not
away. That is exactly what the `v` field's own documentation says it is for:
"so a future field can be added without older clients mis-reading it. Anything
else is ignored." Bumping to `v: 2` would instead make the two builds invisible
to each other, which is the opposite of the intent.

`Deafened` is renamed `Announced`, because a type that tracks two things must
not be named after one of them, and grows `away()` beside `deafened()`.

### How it is drawn

`Participant` in `rooms/dto.rs` gains `away: bool`, `#[serde(default)]`, beside
the `muted` and `deafened` that are already there, with a `with_away` builder to
match.

`ChannelList` draws an `AwayIcon`, a clock, at the same place the muted and
deafened icons go. The precedence is deafened, then away, then muted, most
specific first: somebody deafened and away is drawn deafened, because deafened is
the stronger claim about whether talking to them will work.

`CallPanel` gets a fourth button between deafen and hang up. Same shape, same
`aria-pressed`, same fixed-width label so the row does not jump.

## 4. The sounds themselves come from Element

The two MP3s in `crates/consort-audio/assets` are synthesised: a raised-cosine
envelope over a rising C5 to G5 for join and a falling G5 to C5 for leave, 48
kHz mono, encoded at 96k. They exist so the mechanism could be built and tested
against real bytes rather than against a stub.

They are placeholders. Element Desktop's own join and leave sounds are the ones
people already recognise, and matching them is the point: a sound nobody has to
learn is doing its whole job the first time it plays.

Nothing in the code cares which bytes are in those files. `sound.rs` decodes
whatever is there, resamples from whatever rate it is at, and the tests assert
properties (audible, not clipping, the right rough length, the two
distinguishable from each other) rather than a checksum. So this is a file swap
and a test re-run, not a change.

Worth keeping when the swap happens: the licence. Element Desktop is AGPL like
Consort, so the sounds can come across, but where they came from belongs in a
comment beside them rather than in a commit message nobody reads twice.

## 5. TeamSpeak's spoken notifications

Separate from section 1 and not a replacement for it. TeamSpeak said "user has
entered your channel", "user has left your channel" and "welcome back" out
loud, and that is a different thing from a chime: a chime tells you something
happened, a voice tells you what.

### Rules

**On by default, and separately switchable.** Two settings, not one. Somebody
who wants the chimes and not the voice must be able to have that, and so must
the reverse. `AudioSettings` grows a second `bool` with the same hand-written
`Default` treatment section 1's needed, and for the same reason: a derived
`false` would mean nobody hears them until they go looking.

**They ride the same seam.** `Chime` becomes a larger enum, or gains a
companion, and `Heard::chime` keeps being the only thing `consort-call` knows
about sound. The roster diff in `arrivals.rs` already produces exactly the
events these need. What it does not produce yet is *who*, because today it
answers "did anybody arrive" rather than "who arrived", which is all a chime
needs. That is the one real change: `Arrivals::settle` returns the names, and
the caller decides whether it wants them.

**"Welcome back" is ours, not theirs.** It is the sound of coming back from
away, which TeamSpeak had and which Consort now has the flag for. It plays for
the person who was away, on their own return, and not for everybody else.

**Where the audio comes from is undecided and matters.** Shipping recorded
phrases means one voice and one language and a name that can never be spoken.
Speaking a name means a speech synthesiser, which is a dependency, a licence,
and a startup cost. The honest first version is recorded phrases with no name
in them, matching what TeamSpeak's default sound pack actually did, and
"somebody" is a word this codebase has already decided is enough (see
`trouble.rs`, which says it for the same reason).

## Phases

1. `Voices::play` and the second queue, with the cap. Pure, testable, no device.
2. `sound.rs`: decode, resample, cache. Testable against the shipped bytes.
3. The roster diff in the call thread, with the three rules above as three tests.
4. The setting, and deafen silencing it.
5. `Notice::away`, `Announced`, and the compatibility tests in both directions.
6. `Participant::away`, the icon, the button, the precedence.
7. Confirm all three against a second live client, which is the only thing that
   can actually fail in an interesting way.
8. Swap in Element's join and leave sounds, with their provenance beside them.
9. The spoken notifications: `Arrivals` returning names, the second setting,
   the phrases, and "welcome back" on the way out of away.

## Risks

**A roster diff is not a reliable arrival signal if `Connected` is re-emitted
for other reasons.** It is emitted on every roster change and carries `trouble`,
which changes independently. The diff has to be against the previous
*participant set*, not against the previous event, or a key failure mid-call
plays a chime.

**Two Consort builds in one call disagree about `away`.** Handled by the
`serde(default)` choice above, and worth an explicit test in both directions
rather than an assumption.

**The sound queue outliving the call.** `Voices::silence` already exists for
deafening and must clear the sound queue too, or undeafening replays whatever
chimed while nobody was listening.

## What actually happened

Four things worth recording.

**The away flag needed no version bump, which is what the version field was
for.** `Notice` does not deny unknown fields, so the new one is
`#[serde(default)]` at `v: 1`: a build that predates it reads a notice carrying
it and still gets `deafened` right, and this build reads an older notice as not
away. Bumping to `v: 2` would have made the two builds invisible to each other,
which is the opposite of the intent. Both directions are tested.

**The announcement had to move out of `set_deafened`.** It was folded into that
setter, which meant a second thing worth announcing had nowhere to go.
`CallSession::announce_self` takes the whole `SelfAudio`, and mute stays out of
it: the SFU already broadcasts a track mute, and a second source for one fact
is a disagreement waiting to happen.

**The roster diff needed to know which participant is us, and nothing could
tell it.** The plan assumed absorbing the first roster would cover our own
arrival. It does not: a join reports an empty roster for the moment between
publishing a membership and it coming back round, so our own entry routinely
lands in the *second* reading, which is the first one that gets diffed. A rule
that absorbed anything empty fixed that and broke something worse, because it
also swallowed the first person to walk into an empty channel. `Roster::me` is
the new trait method, "nobody has looked" and "the channel was empty" are
different states, and both cases are tested.

**The setting is the reason `AudioSettings` writes its own `Default`.** A
derived `bool` is false, and the whole point of these is that somebody who has
never opened the settings screen still hears that company arrived. It is read
through an atomic rather than the settings file, because the question is asked
on the call thread at the moment a roster changes and a file read there would
be a disk touch in the middle of a call for one boolean.
