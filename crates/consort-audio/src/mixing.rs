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
#[derive(Clone, Default)]
pub struct Voices(Arc<Mutex<HashMap<String, VecDeque<i16>>>>);

impl Voices {
    /// Nobody, yet.
    pub fn new() -> Self {
        Self::default()
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
        let mut voices = self.voices();
        for waiting in voices.values_mut() {
            // `drain` on the shorter of the two, so a queue with less than a
            // full buffer in it contributes what it has and the rest stays
            // silent rather than the whole voice being skipped.
            let taking = waiting.len().min(sum.len());
            for (slot, sample) in sum.iter_mut().zip(waiting.drain(..taking)) {
                *slot += i32::from(sample);
            }
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
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
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
