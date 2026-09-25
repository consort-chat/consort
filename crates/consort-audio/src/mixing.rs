// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Everybody else's audio, on its way to the speakers.
//!
//! The other half of [`crate::capture`]: many producers, one device, and two
//! clocks that do not agree. Why a native client has to mix the call itself,
//! and how the queues and levels answer to that:
//! `docs/adr/0002-consort-mixes-the-call-itself.md`.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::gate::FRAME_SAMPLES;

/// How much of one person's audio may be waiting before the oldest is dropped.
///
/// Twelve frames is 120 ms: more than a healthy connection's jitter, and short
/// enough that recovering from a bad patch is not heard as lag.
pub const JITTER_FRAMES: usize = 12;

/// [`JITTER_FRAMES`] as a sample count, which is what the queue measures in.
pub const JITTER_SAMPLES: usize = JITTER_FRAMES * FRAME_SAMPLES;

/// Everyone in the call who can currently be heard, and what they are saying.
///
/// Cheap to clone: every clone is the same set of queues, one for the call
/// thread and one for the audio thread. Keyed by whatever the caller uses to
/// tell participants apart, so MatrixRTC stays a stranger to this crate.
#[derive(Clone)]
pub struct Voices {
    people: Arc<Mutex<HashMap<String, VecDeque<i16>>>>,
    /// Sounds this client makes about the call, rather than audio from anybody
    /// in it. Its own queue because the cap on a person's is 120 ms, which
    /// would deliver a chime as its own last 120 ms.
    sounds: Arc<Mutex<VecDeque<i16>>>,
    /// How loud everything leaving here should be, as a percentage. An atomic
    /// because it is read in the device callback and written by whoever moved
    /// a slider.
    output: Arc<AtomicU8>,
    /// The same, for chimes and spoken notifications, and multiplied under
    /// [`Self::output`] rather than applied beside it.
    notifications: Arc<AtomicU8>,
    /// How loud each person should be, keyed the way the queues are.
    ///
    /// Replaced wholesale by [`set_person_levels`](Self::set_person_levels),
    /// and held apart from the queues, which `forget` drops on a mute.
    people_levels: Arc<Mutex<HashMap<String, u8>>>,
}

/// Full volume, as these percentages count it.
pub const FULL_VOLUME: u8 = 100;

/// As loud as one person may be made.
///
/// One person only: the master and the notification level stop at
/// [`FULL_VOLUME`], because past that a summed mix distorts rather than gets
/// louder. Boosting one voice can reach full scale and clip (see [`clamp`]).
pub const MAX_PERSON_VOLUME: u8 = 250;

/// A percentage turned into something to multiply samples by.
///
/// Squared, because a slider linear in amplitude is not linear in anything a
/// person hears. One curve for all three levels, so the two lower ceilings are
/// kept where those levels are stored. See the module header's ADR.
pub fn gain(percent: u8) -> f32 {
    let fraction = f32::from(percent.min(MAX_PERSON_VOLUME)) / f32::from(FULL_VOLUME);
    fraction * fraction
}

impl Default for Voices {
    /// Everything at full volume. Hand-written rather than derived because a
    /// derived `AtomicU8` is zero, and zero here is a silent call.
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
/// Six seconds, because a chime plus a spoken sentence is over two on its own
/// and anything shorter cuts the sentence off.
pub const SOUND_SAMPLES: usize = 6 * crate::gate::SAMPLE_RATE as usize;

impl Voices {
    /// Nobody, yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a sound to play into the call.
    ///
    /// `samples` is mono PCM at [`crate::SAMPLE_RATE`]. Appended, and dropped
    /// past [`SOUND_SAMPLES`] from the *end*, unlike a voice queue: a
    /// truncated chime is the chime, one missing its start is a click.
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
    /// Applied at the mix, so it takes effect on the next buffer including for
    /// audio already queued.
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
    /// Wholesale, because these are keyed by membership and a membership is
    /// fresh on every join. Anybody left out plays at full volume.
    pub fn set_person_levels(&self, levels: HashMap<String, u8>) {
        *self.people_levels() = levels
            .into_iter()
            .map(|(who, percent)| (who, percent.min(MAX_PERSON_VOLUME)))
            .collect();
    }

    /// Add what `who` just said to what is waiting to be played.
    ///
    /// `samples` is mono PCM at [`crate::SAMPLE_RATE`]. Never blocks: the
    /// caller is the call thread, which is also servicing the SFU. A full
    /// queue drops its oldest audio, not its newest.
    pub fn hear(&self, who: &str, samples: &[i16]) {
        let mut voices = self.voices();
        // Not `entry`, which would clone the key on every frame. This runs a
        // hundred times a second per participant and inserts on the first.
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
    /// For a stream stopping, not for somebody going quiet: a silent
    /// participant keeps their queue and keeps mixing to nothing.
    pub fn forget(&self, who: &str) {
        self.voices().remove(who);
    }

    /// Forget everybody.
    ///
    /// What deafening does to the audio already in flight, since pausing the
    /// subscription travels to the SFU and back before it takes effect.
    pub fn silence(&self) {
        self.voices().clear();
        // The sounds too, or undeafening replays every arrival that chimed
        // while nobody was listening.
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
    /// `i32` because four people at three quarters of full scale overflow an
    /// `i16`, and a wrapped sum is loud noise in the opposite direction. The
    /// caller clamps once, at the end, where the total is known.
    pub fn mix(&self, sum: &mut [i32]) {
        // Read once per buffer. A level that changed underneath one would be a
        // discontinuity; half a millisecond of slider lag is not.
        let output = gain(self.output.load(Ordering::Relaxed));

        let levels = self.people_levels();
        let mut voices = self.voices();
        for (who, waiting) in voices.iter_mut() {
            // Multiplied, not applied in two passes: turning the call down has
            // to turn each person in it down with it.
            let level = output * gain(levels.get(who).copied().unwrap_or(FULL_VOLUME));
            // The shorter of the two, so a queue holding less than a buffer
            // contributes what it has rather than being skipped.
            let taking = waiting.len().min(sum.len());
            for (slot, sample) in sum.iter_mut().zip(waiting.drain(..taking)) {
                *slot += scaled(sample, level);
            }
        }
        drop(voices);
        drop(levels);

        // Into the same accumulator, so a chime landing while four people talk
        // is clamped once with them rather than against a total it cannot see.
        let level = output * gain(self.notifications.load(Ordering::Relaxed));
        let mut sounds = self.sounds();
        let taking = sounds.len().min(sum.len());
        for (slot, sample) in sum.iter_mut().zip(sounds.drain(..taking)) {
            *slot += scaled(sample, level);
        }
    }

    /// The queues, recovering from a poisoned lock rather than spreading a
    /// panic: one caller is the audio thread, where a panic takes the sound
    /// card with it. Nothing in a critical section here can panic.
    fn voices(&self) -> MutexGuard<'_, HashMap<String, VecDeque<i16>>> {
        self.people.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The sound queue, on the same terms, and locked separately so queueing a
    /// chime never waits on a buffer being mixed.
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
/// Rounded, not truncated: truncation biases every sample towards zero, which
/// on quiet speech is distortion rather than error.
fn scaled(sample: i16, level: f32) -> i32 {
    // The common case, and exact: nobody has touched a slider.
    if level == 1.0 {
        return i32::from(sample);
    }
    (f32::from(sample) * level).round() as i32
}

/// [`Voices`] being handed to a device, one buffer at a time.
///
/// The mirror of [`crate::playback::Playing`], and separate from it rather
/// than generic over a source because a chime ends and has to say so, while a
/// call has nothing to announce.
pub struct Mixing {
    voices: Voices,
    channels: usize,
    /// The mixed buffer, kept between callbacks rather than allocated inside
    /// one. A device asks for the same size every time, so this grows once.
    sum: Vec<i32>,
}

impl Mixing {
    /// `channels` is what the device negotiated.
    pub fn new(voices: Voices, channels: u16) -> Self {
        Self {
            voices,
            // Nothing should claim zero channels, but dividing by it panics
            // inside a realtime callback. `Playing` guards the same thing.
            channels: usize::from(channels).max(1),
            sum: Vec::new(),
        }
    }

    /// Fill one buffer of `i16` samples, interleaved.
    pub fn fill_i16(&mut self, data: &mut [i16]) {
        let channels = self.channels;
        let mixed = self.mixed(data.len().div_ceil(channels));

        for (group, total) in data.chunks_mut(channels).zip(mixed) {
            // The same sample in every channel: everything upstream is mono,
            // and a call in one ear reads as a broken headphone.
            group.fill(clamp(*total));
        }
    }

    /// Fill one buffer of `f32` samples, which cpal wants in `[-1.0, 1.0]`.
    pub fn fill_f32(&mut self, data: &mut [f32]) {
        let channels = self.channels;
        let mixed = self.mixed(data.len().div_ceil(channels));

        for (group, total) in data.chunks_mut(channels).zip(mixed) {
            // 32768, not `i16::MAX`: the range is asymmetric and `i16::MIN`
            // over `i16::MAX` is past -1.0. Same as `Playing`.
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
/// Clipping rather than a limiter, which would duck the whole call on every
/// overlap. See the module header's ADR.
fn clamp(total: i32) -> i16 {
    total.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}
