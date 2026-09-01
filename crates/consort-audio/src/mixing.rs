// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Everybody else's audio, on its way to the speakers.
//!
//! ## Why this has to exist at all
//!
//! In a browser, a LiveKit room plays itself: the SDK attaches a subscribed
//! track to an `<audio>` element and the page makes noise without anybody
//! asking. Natively there is no such thing. A subscribed track is a decoder
//! producing PCM into a stream, and if nothing pulls that stream the frames
//! are decoded and dropped. Every layer reports success the whole way down,
//! and the call is silent.
//!
//! So this is the other half of [`crate::capture`], and the shape is the
//! capture path in reverse: many producers, one device, two clocks that do not
//! agree.
//!
//! ## The two clocks
//!
//! Frames arrive from the network, one per participant, paced by whenever the
//! packets carrying them turned up. They leave through a sound card that asks
//! for a buffer on its own schedule and will not wait. Neither side can block
//! the other: a producer stalled on the device would stall the whole call
//! thread, and a device callback stalled on the network is a click in
//! somebody's headphones.
//!
//! Hence a queue per person, the same bargain [`consort_call::Microphone`]
//! makes going the other way, and for the same reasons written down there. A
//! queue that has grown past [`JITTER_FRAMES`] drops from the **front**: what
//! is waiting is audio that would be heard late, and late audio in a
//! conversation is worse than missing audio, because everything behind it is
//! late too and stays late.
//!
//! An empty queue produces silence rather than waiting. A participant whose
//! packets are late is one person briefly dropping out, and stalling the
//! device for them would take everybody else out too.
//!
//! [`consort_call::Microphone`]: https://docs.rs/consort-call

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::gate::FRAME_SAMPLES;

/// How much of one person's audio may be waiting before the oldest is dropped.
///
/// Twelve frames is 120 ms. Comfortably more than the jitter of a healthy
/// connection, and short enough that recovering from a bad patch costs less
/// delay than a person notices as a lag in a conversation.
pub const JITTER_FRAMES: usize = 12;

/// [`JITTER_FRAMES`] as a sample count, which is what the queue measures in.
pub const JITTER_SAMPLES: usize = JITTER_FRAMES * FRAME_SAMPLES;

/// Everyone in the call who can currently be heard, and what they are saying.
///
/// Cheap to clone: every clone is the same set of queues. One goes to the call
/// thread, which only ever [`hear`](Self::hear)s and [`forget`](Self::forget)s,
/// and one to the audio thread, which only ever [`mix`](Self::mix)es.
///
/// Keyed by whatever the caller uses to tell participants apart. Nothing here
/// looks inside the key, so the call layer's `member_id` is what ends up in it
/// without this crate having to know that MatrixRTC exists.
#[derive(Clone)]
pub struct Voices {
    people: Arc<Mutex<HashMap<String, VecDeque<i16>>>>,
    /// Sounds this client is making about the call, rather than audio from
    /// anybody in it.
    ///
    /// A queue of its own, and not a reserved key in the map above, for one
    /// concrete reason: a person's queue is capped at [`JITTER_SAMPLES`] and
    /// drops the oldest when it overflows. That is right for speech, where
    /// late audio is worthless, and wrong for a sound half a second long,
    /// which would arrive as its own last 120 milliseconds.
    sounds: Arc<Mutex<VecDeque<i16>>>,
    /// How loud everything leaving here should be, as a percentage.
    ///
    /// An atomic rather than a field the audio thread is handed a copy of,
    /// because the two ends are different threads: this is read inside the
    /// device callback and written by whoever moved a slider. A percentage
    /// rather than the multiplier it becomes, so that what is stored is what
    /// was chosen and the curve stays in one place.
    output: Arc<AtomicU8>,
    /// The same, for the chimes and spoken notifications only.
    ///
    /// Underneath [`Self::output`] rather than beside it: this says how loud a
    /// notification is *relative to the call*, which is the thing anybody
    /// actually wants to set. A notification level that ignored the master
    /// would get louder every time somebody turned the call down.
    notifications: Arc<AtomicU8>,
    /// How loud each person should be, keyed the way the queues are.
    ///
    /// Replaced wholesale rather than edited, by
    /// [`set_person_levels`](Self::set_person_levels), because the keys are
    /// memberships and a membership is fresh on every join: a map that was only
    /// ever added to would grow for the lifetime of the process and hold levels
    /// against people who left an hour ago.
    ///
    /// Separate from the queue map rather than a second field on each queue,
    /// because the two have different lifetimes. `forget` drops a queue the
    /// moment somebody's stream stops, and a level that went with it would be
    /// lost every time a person muted.
    people_levels: Arc<Mutex<HashMap<String, u8>>>,
}

/// Full volume, as these percentages count it.
pub const FULL_VOLUME: u8 = 100;

/// As loud as one person may be made.
///
/// One person, and only one person. The master and the notification level
/// still stop at [`FULL_VOLUME`], because turning the whole call up past full
/// scale turns it into distortion rather than volume: everything is already
/// summed by then, so there is nothing left that could be raised on its own.
///
/// A single stream is a different question, and the reason this exists.
/// Somebody on a laptop microphone three feet away arrives quiet against
/// everybody else, and the repair is to bring that one voice up rather than to
/// bring the rest of the room down to meet it. Above unity the sum can reach
/// full scale and clip (see [`clamp`]), which is the cost and is the right
/// shape of cost: it lands on the moments the boosted person is actually loud
/// rather than on every call all the time.
pub const MAX_PERSON_VOLUME: u8 = 250;

/// A percentage turned into something to multiply samples by.
///
/// Squared rather than proportional, because a slider that is linear in
/// amplitude is not linear in anything a person hears. Half amplitude is about
/// six decibels down, which the ear takes as roughly two thirds as loud, so a
/// proportional slider spends its bottom half on changes nobody can hear much
/// of and its top half on almost nothing. Squaring puts the middle of the
/// slider near the middle of the range somebody is listening for.
///
/// The squaring applies above a hundred as well, which is why the number on a
/// person's slider has never been an amplitude and is not one here either.
/// Half the travel is already a quarter of the amplitude, so a top of 250 is
/// a little over six times, and the control stays one continuous curve instead
/// of changing character at the point somebody crosses full volume.
///
/// Clamped at [`MAX_PERSON_VOLUME`], the highest anything is allowed to ask
/// for. The lower ceiling on the master and the notifications is kept where
/// those two are stored, so this stays one curve rather than three.
pub fn gain(percent: u8) -> f32 {
    let fraction = f32::from(percent.min(MAX_PERSON_VOLUME)) / f32::from(FULL_VOLUME);
    fraction * fraction
}

impl Default for Voices {
    /// Everything at full volume, which is what somebody who has never touched
    /// a slider should hear.
    ///
    /// Hand-written rather than derived for one reason: a derived `AtomicU8` is
    /// zero, and zero here is silence. A call that played nothing until the
    /// settings were read would be the worst possible default.
    fn default() -> Self {
        Self {
            people: Arc::default(),
            sounds: Arc::default(),
            output: Arc::new(AtomicU8::new(FULL_VOLUME)),
            notifications: Arc::new(AtomicU8::new(FULL_VOLUME)),
            people_levels: Arc::default(),
        }
    }
}

/// How much sound may be queued before the rest is dropped.
///
/// Six seconds. It was two, which was right when the only thing that queued
/// here was a chime a third of a second long, and became wrong the moment a
/// spoken notification could follow one: a chime plus a sentence is over two
/// seconds on its own, so a single arrival would have had its sentence cut off
/// at the end by a cap meant to stop a backlog of several.
///
/// Still short enough for the thing the cap is for. Somebody rejoining a busy
/// channel hears the first few arrivals and not the next minute of them, which
/// is what a cap on a queue that drops from the end buys.
pub const SOUND_SAMPLES: usize = 6 * crate::gate::SAMPLE_RATE as usize;

impl Voices {
    /// Nobody, yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a sound to play into the call.
    ///
    /// Appended rather than replacing what is already queued, so two people
    /// arriving at once are two sounds in sequence rather than one sound
    /// played on top of itself.
    ///
    /// `samples` is mono PCM at [`crate::SAMPLE_RATE`], like everything else
    /// here. Dropped past [`SOUND_SAMPLES`], and dropped from the *end* rather
    /// than the start, which is the opposite of what a voice queue does: a
    /// truncated chime is still recognisably the chime, while a chime missing
    /// its beginning is a click.
    pub fn play(&self, samples: &[i16]) {
        let mut sounds = self.sounds();
        let room = SOUND_SAMPLES.saturating_sub(sounds.len());
        sounds.extend(samples.iter().copied().take(room));
    }

    /// How much sound is waiting to play.
    pub fn sound_waiting(&self) -> usize {
        self.sounds().len()
    }

    /// Set how loud everything leaving here should be, as a percentage.
    ///
    /// Takes effect on the next buffer, including for audio already queued.
    /// That is the point of applying it at the mix rather than on the way in: a
    /// slider that only affected what arrived after it was moved would do
    /// nothing at all for the hundred milliseconds somebody is listening to
    /// while they move it.
    pub fn set_output_level(&self, percent: u8) {
        self.output
            .store(percent.min(FULL_VOLUME), Ordering::Relaxed);
    }

    /// Set how loud the chimes and spoken notifications should be, as a
    /// percentage of the output level above.
    pub fn set_notification_level(&self, percent: u8) {
        self.notifications
            .store(percent.min(FULL_VOLUME), Ordering::Relaxed);
    }

    /// Replace every per-person level at once.
    ///
    /// Wholesale rather than one at a time, because these are keyed by
    /// membership and a membership is fresh on every join. Handing over the
    /// whole set is what keeps the map the size of the call rather than the
    /// size of everybody who has ever been in one.
    ///
    /// Anybody left out plays at full volume, which is also what somebody
    /// nobody has ever adjusted gets.
    pub fn set_person_levels(&self, levels: HashMap<String, u8>) {
        *self.people_levels() = levels
            .into_iter()
            .map(|(who, percent)| (who, percent.min(MAX_PERSON_VOLUME)))
            .collect();
    }

    /// Add what `who` just said to what is waiting to be played.
    ///
    /// `samples` is mono PCM at [`crate::SAMPLE_RATE`]. Never blocks on the
    /// device and never waits: called from the call thread, which is also
    /// servicing the SFU.
    ///
    /// Drops the oldest audio rather than the newest when the queue is full.
    /// See the header.
    pub fn hear(&self, who: &str, samples: &[i16]) {
        let mut voices = self.voices();
        // Asked about before being inserted into, rather than through `entry`,
        // which would clone the key on every single frame. This runs a hundred
        // times a second per participant and inserts on the first one only.
        if !voices.contains_key(who) {
            voices.insert(who.to_owned(), VecDeque::new());
        }
        let waiting = voices
            .get_mut(who)
            .expect("the queue was just inserted if it was missing");

        waiting.extend(samples.iter().copied());
        let over = waiting.len().saturating_sub(JITTER_SAMPLES);
        if over > 0 {
            waiting.drain(..over);
        }
    }

    /// Forget `who` entirely, dropping whatever they had waiting.
    ///
    /// Called when somebody's stream stops, which is not the same as somebody
    /// going quiet: a participant who is merely silent keeps their queue and
    /// keeps mixing to nothing.
    pub fn forget(&self, who: &str) {
        self.voices().remove(who);
    }

    /// Forget everybody.
    ///
    /// What deafening does to the audio already in flight. The subscription
    /// pause stops more arriving, but it travels to the SFU and back, and
    /// whatever is queued here would play out underneath somebody who has just
    /// asked for silence.
    pub fn silence(&self) {
        self.voices().clear();
        // The sounds too. Undeafening otherwise replays whatever chimed while
        // nobody was listening, which is a burst of arrivals for people who
        // have been in the channel for a minute by then.
        self.sounds().clear();
    }

    /// How many samples `who` has waiting.
    pub fn waiting(&self, who: &str) -> usize {
        self.voices().get(who).map_or(0, VecDeque::len)
    }

    /// Everyone currently queued, in no particular order.
    pub fn everyone(&self) -> Vec<String> {
        self.voices().keys().cloned().collect()
    }

    /// Add everybody's next samples into `sum`, consuming them.
    ///
    /// Accumulating rather than assigning, and into a width that cannot wrap.
    /// Summing four people who are each at three quarters of full scale
    /// overflows `i16` and would wrap to loud noise in the opposite direction,
    /// which is the worst possible failure to put into somebody's headphones.
    /// The caller clamps once, at the end, where the total is known.
    ///
    /// A person with nothing waiting contributes silence. See the header.
    pub fn mix(&self, sum: &mut [i32]) {
        // Read once for the whole buffer rather than per sample. Half of one
        // millisecond of somebody's own slider movement landing on the next
        // buffer instead of this one is not a thing anybody can hear, and a
        // level that changed underneath a buffer would be a discontinuity that
        // is.
        let output = gain(self.output.load(Ordering::Relaxed));

        let levels = self.people_levels();
        let mut voices = self.voices();
        for (who, waiting) in voices.iter_mut() {
            // Multiplied together rather than applied in two passes: the
            // per-person level says how loud somebody is *in* the call, so
            // turning the call down has to turn them down with it.
            let level = output * gain(levels.get(who).copied().unwrap_or(FULL_VOLUME));
            // `drain` on the shorter of the two, so a queue with less than a
            // full buffer in it contributes what it has and the rest stays
            // silent rather than the whole voice being skipped.
            let taking = waiting.len().min(sum.len());
            for (slot, sample) in sum.iter_mut().zip(waiting.drain(..taking)) {
                *slot += scaled(sample, level);
            }
        }
        drop(voices);
        drop(levels);

        // Into the same accumulator, so a sound that lands while four people
        // are talking is clamped once with everything else rather than
        // separately against a total it cannot see.
        let level = output * gain(self.notifications.load(Ordering::Relaxed));
        let mut sounds = self.sounds();
        let taking = sounds.len().min(sum.len());
        for (slot, sample) in sum.iter_mut().zip(sounds.drain(..taking)) {
            *slot += scaled(sample, level);
        }
    }

    /// The queues, recovering from a poisoned lock rather than spreading a
    /// panic.
    ///
    /// The same bargain `Microphone` makes on the way out, and for the same
    /// reason: one of the two callers is the audio thread, where a panic takes
    /// the sound card with it. Nothing inside a critical section here can
    /// panic, so the recovery is unreachable.
    fn voices(&self) -> MutexGuard<'_, HashMap<String, VecDeque<i16>>> {
        self.people.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The sound queue, on the same terms.
    ///
    /// Locked separately from the voices rather than under one guard, so a
    /// call thread queueing a chime never waits on the audio thread mixing a
    /// buffer. Nothing reads both at once except [`mix`](Self::mix), which
    /// takes them one after the other.
    fn sounds(&self) -> MutexGuard<'_, VecDeque<i16>> {
        self.sounds.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The per-person levels, on the same terms as the two above.
    fn people_levels(&self) -> MutexGuard<'_, HashMap<String, u8>> {
        self.people_levels
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// One sample at one level, in the width the accumulator uses.
///
/// Rounded rather than truncated. Truncation is a bias towards zero on every
/// sample, which on quiet speech at a low level is a small constant distortion
/// rather than a small constant error.
fn scaled(sample: i16, level: f32) -> i32 {
    // The common case by a wide margin, and exact: nobody has touched a slider.
    if level == 1.0 {
        return i32::from(sample);
    }
    (f32::from(sample) * level).round() as i32
}

/// [`Voices`] being handed to a device, one buffer at a time.
///
/// The mirror of [`crate::playback::Playing`], which does this for the test
/// chime, and structurally the same: spread mono across however many channels
/// the device negotiated, in whichever of two sample formats it asked for, in
/// buffers whose size it chose.
///
/// Separate from `Playing` rather than generic over a source, because the two
/// differ in the one place that matters. A chime ends, and `Playing` exists
/// largely to notice that and say so. A call does not end until somebody hangs
/// up, and there is nothing for this to announce.
pub struct Mixing {
    voices: Voices,
    channels: usize,
    /// The mixed buffer, kept between callbacks rather than allocated in one.
    ///
    /// A device asks for the same size buffer every time in practice, so this
    /// grows once and then never again.
    sum: Vec<i32>,
}

impl Mixing {
    /// `channels` is what the device negotiated.
    pub fn new(voices: Voices, channels: u16) -> Self {
        Self {
            voices,
            // Nothing should claim zero channels, but dividing by it would
            // panic inside a realtime callback, which is the worst place in the
            // program to find out. `Playing` guards the same thing.
            channels: usize::from(channels).max(1),
            sum: Vec::new(),
        }
    }

    /// Fill one buffer of `i16` samples, interleaved.
    pub fn fill_i16(&mut self, data: &mut [i16]) {
        let channels = self.channels;
        let mixed = self.mixed(data.len().div_ceil(channels));

        for (group, total) in data.chunks_mut(channels).zip(mixed) {
            // The same sample in every channel. Everything upstream of here is
            // mono, and putting a call into one ear only would read as a
            // broken headphone.
            group.fill(clamp(*total));
        }
    }

    /// Fill one buffer of `f32` samples, which cpal wants in `[-1.0, 1.0]`.
    pub fn fill_f32(&mut self, data: &mut [f32]) {
        let channels = self.channels;
        let mixed = self.mixed(data.len().div_ceil(channels));

        for (group, total) in data.chunks_mut(channels).zip(mixed) {
            // Divided by 32768 rather than by `i16::MAX`, because the range is
            // asymmetric and `i16::MIN` over `i16::MAX` is past -1.0. Same as
            // `Playing`.
            group.fill(f32::from(clamp(*total)) / 32_768.0);
        }
    }

    /// The next `frames` mono samples, everybody summed together.
    fn mixed(&mut self, frames: usize) -> &[i32] {
        self.sum.clear();
        self.sum.resize(frames, 0);
        self.voices.mix(&mut self.sum);
        &self.sum
    }
}

/// One mixed sample, brought back into the range a device can be handed.
///
/// Clipping rather than scaling everybody down. A limiter that ducked the
/// whole call whenever two people overlapped would be audible constantly; this
/// is audible only when the sum genuinely runs out of room, which with real
/// speech is rare and brief.
fn clamp(total: i32) -> i16 {
    total.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}
