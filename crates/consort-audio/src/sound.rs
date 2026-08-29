// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The short sounds a call makes about itself.
//!
//! Somebody arriving in a voice channel with no sound is somebody who arrives
//! unnoticed, and the result is two people starting a sentence at once. The
//! only way to know is to be looking at the right corner of the screen at the
//! right moment, which nobody is.
//!
//! Distinct from [`crate::tone`], which is arithmetic: that one is the output
//! test, it has to be recognisably synthetic, and generating it means no file
//! to ship. These are recorded audio, because a rising fifth somebody chose
//! sounds like a product and a rising fifth a computer derived sounds like a
//! modem.
//!
//! ## Decoded once, and only if played
//!
//! The files are `include_bytes!`d rather than read from disk. A sound that
//! depends on an install path is a sound that is missing on somebody else's
//! machine, and these are four kilobytes each.
//!
//! Decoding happens on first use and the result is kept. Somebody who never
//! joins a call pays nothing, and somebody in a busy channel pays once rather
//! than once per arrival.
//!
//! ## The spoken half is wired and has nothing to say yet
//!
//! [`Phrase`] is the TeamSpeak half: a chime says something happened, a voice
//! says what. The mechanism is finished, switchable and tested. The three
//! files behind it are silence, because a recorded sentence has to be recorded
//! and a beep standing in for one would be worse than nothing: the listener
//! would hear two chimes and learn less than from one.
//!
//! Silence is therefore deliberate rather than a bug, and it is the one
//! placeholder that cannot mislead. Everything else about a phrase is already
//! true: it decodes, it is the length of the sentence it will be, it queues
//! behind the chime rather than over it, and its setting switches it. Dropping
//! three recordings into `assets/voice` is the whole of what is left, and the
//! test that says a sound is audible arrives with them.

use std::sync::OnceLock;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::gate::SAMPLE_RATE;

/// Somebody joined the voice channel this session is in.
const JOINED: &[u8] = include_bytes!("../assets/join.mp3");

/// Somebody left it.
const LEFT: &[u8] = include_bytes!("../assets/leave.mp3");

/// "Somebody has entered your channel." Silence, for now. See the header.
const SAYS_ENTERED: &[u8] = include_bytes!("../assets/voice/entered.mp3");

/// "Somebody has left your channel." Silence, for now.
const SAYS_LEFT: &[u8] = include_bytes!("../assets/voice/left.mp3");

/// "Welcome back." Silence, for now.
const SAYS_WELCOME_BACK: &[u8] = include_bytes!("../assets/voice/welcome-back.mp3");

/// Which sound.
///
/// An enum rather than a path or a name, so that a caller cannot ask for a
/// file that is not there. Everything shipped is decodable at build time by
/// construction, and everything else is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// Somebody arrived.
    Joined,
    /// Somebody left.
    Left,
}

impl Sound {
    /// The samples to play: mono PCM at [`SAMPLE_RATE`].
    ///
    /// Empty if the file will not decode, which cannot happen for the files
    /// this crate ships and is still not worth a panic. A missing chime is a
    /// call that is quieter than intended; a panic here is a call that ends.
    pub fn samples(self) -> &'static [i16] {
        static DECODED: [OnceLock<Vec<i16>>; 2] = [OnceLock::new(), OnceLock::new()];

        let (slot, bytes) = match self {
            Self::Joined => (&DECODED[0], JOINED),
            Self::Left => (&DECODED[1], LEFT),
        };

        cached(slot, bytes, self)
    }
}

/// Which sentence.
///
/// A second enum rather than three more variants of [`Sound`], because the two
/// are switched on and off separately and the type is what keeps the two
/// switches from being applied to the wrong one. A caller holding a `Phrase`
/// cannot accidentally consult the chime setting about it.
///
/// The phrases name nobody. That is what TeamSpeak's own default pack did, and
/// it is the only version that can ship: a name has to be spoken by a
/// synthesiser, which is a dependency, a licence and a startup cost, and
/// "somebody" is a word this codebase has already decided is enough elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phrase {
    /// "Somebody has entered your channel."
    Entered,
    /// "Somebody has left your channel."
    Left,
    /// "Welcome back", to the person who was away and is not any more.
    WelcomeBack,
}

impl Phrase {
    /// The samples to play: mono PCM at [`SAMPLE_RATE`].
    ///
    /// Currently silence for all three, on purpose. See the module header.
    pub fn samples(self) -> &'static [i16] {
        static DECODED: [OnceLock<Vec<i16>>; 3] =
            [OnceLock::new(), OnceLock::new(), OnceLock::new()];

        let (slot, bytes) = match self {
            Self::Entered => (&DECODED[0], SAYS_ENTERED),
            Self::Left => (&DECODED[1], SAYS_LEFT),
            Self::WelcomeBack => (&DECODED[2], SAYS_WELCOME_BACK),
        };

        cached(slot, bytes, self)
    }
}

/// Decode `bytes` once and keep the result in `slot`.
///
/// Shared by both enums so that a sound and a sentence cannot end up with
/// different ideas about what a failed decode does. It renders as silence
/// either way: a missing chime is a call that is quieter than intended, and a
/// panic here is a call that ends.
fn cached(
    slot: &'static OnceLock<Vec<i16>>,
    bytes: &'static [u8],
    what: impl std::fmt::Debug,
) -> &'static [i16] {
    slot.get_or_init(|| {
        decode(bytes).unwrap_or_else(|| {
            tracing::warn!(sound = ?what, "a call sound would not decode");
            Vec::new()
        })
    })
}

/// Turn an MP3 into mono PCM at [`SAMPLE_RATE`].
///
/// `None` for anything that will not decode, which the caller renders as
/// silence. Everything here is infallible for the files this crate ships; the
/// error path exists because `include_bytes!` does not check that the bytes
/// are audio, and a future replacement should fail quietly rather than at a
/// `.unwrap()` inside somebody's call.
fn decode(bytes: &'static [u8]) -> Option<Vec<i16>> {
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;

    // Cloned rather than borrowed, because `track` borrows the reader and the
    // loop below needs it mutably. Two owned numbers and a codec id.
    let track = format.default_track(TrackType::Audio)?;
    let track_id = track.id;
    let params = track.codec_params.as_ref()?.audio()?.clone();
    let rate = params.sample_rate.unwrap_or(SAMPLE_RATE);

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .ok()?;

    let mut samples = Vec::new();
    let mut interleaved: Vec<i16> = Vec::new();
    // A decode error mid-file ends the sound rather than discarding it. What
    // has been decoded so far is still the beginning of the right chime, and
    // stopping early is less noticeable than not playing at all.
    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            break;
        };

        let channels = decoded.spec().channels().count().max(1);
        decoded.copy_to_vec_interleaved(&mut interleaved);
        push_mono(&interleaved, channels, &mut samples);
    }

    // A file that probed and then decoded to nothing is as useless as one that
    // would not probe at all, and the caller has one answer for both.
    (!samples.is_empty()).then(|| resample(samples, rate))
}

/// Fold interleaved frames down to one channel.
///
/// Averaged rather than taking the first channel. A stereo file that put the
/// sound only in the right channel would vanish entirely, and one whose two
/// channels are out of phase would cancel on the average, which is at least a
/// failure somebody can hear and diagnose rather than silence.
fn push_mono(interleaved: &[i16], channels: usize, out: &mut Vec<i16>) {
    for frame in interleaved.chunks(channels) {
        let total: i32 = frame.iter().copied().map(i32::from).sum();
        out.push((total / frame.len().max(1) as i32) as i16);
    }
}

/// Stretch or squash `samples` to [`SAMPLE_RATE`].
///
/// Linear, and a no-op for anything already at the right rate, which the files
/// this crate ships are. It exists for the file somebody drops in to replace
/// one of them: the whole mixer is 48 kHz, and handing it 44.1 kHz audio plays
/// the sound nine percent fast and a semitone sharp rather than failing, which
/// is exactly the kind of bug nobody reports because it merely sounds odd.
fn resample(samples: Vec<i16>, from: u32) -> Vec<i16> {
    if from == SAMPLE_RATE || samples.is_empty() {
        return samples;
    }

    let ratio = f64::from(SAMPLE_RATE) / f64::from(from);
    let wanted = ((samples.len() as f64) * ratio) as usize;

    (0..wanted)
        .map(|at| {
            let source = at as f64 / ratio;
            let left = source as usize;
            let right = (left + 1).min(samples.len() - 1);
            let between = source - left as f64;

            let a = f64::from(samples[left.min(samples.len() - 1)]);
            let b = f64::from(samples[right]);
            (a + (b - a) * between) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_shipped_sounds_decode() {
        // The one thing `include_bytes!` cannot check. It will happily embed a
        // text file, and the only symptom would be a call that never chimes.
        for sound in [Sound::Joined, Sound::Left] {
            assert!(
                !sound.samples().is_empty(),
                "{sound:?} decoded to nothing at all"
            );
        }
    }

    #[test]
    fn a_sound_is_long_enough_to_hear_and_short_enough_not_to_intrude() {
        // Between a tenth of a second, below which it is a click, and one
        // second, above which it is talking over somebody.
        for sound in [Sound::Joined, Sound::Left] {
            let seconds = sound.samples().len() as f64 / f64::from(SAMPLE_RATE);
            assert!(
                (0.1..=1.0).contains(&seconds),
                "{sound:?} is {seconds:.2}s long"
            );
        }
    }

    #[test]
    fn a_sound_is_actually_audible() {
        // A file that decodes to the right number of zeroes passes every other
        // test here and plays nothing.
        for sound in [Sound::Joined, Sound::Left] {
            let loudest = sound.samples().iter().map(|s| s.abs()).max().unwrap_or(0);
            assert!(loudest > 1000, "{sound:?} peaks at {loudest}");
        }
    }

    #[test]
    fn a_sound_is_quiet_enough_not_to_startle() {
        // These fire whenever anybody walks in. Something at full scale in
        // headphones, several times an evening, is the reason people turn
        // these off in every client that has them.
        for sound in [Sound::Joined, Sound::Left] {
            let loudest = sound.samples().iter().map(|s| s.abs()).max().unwrap_or(0);
            assert!(loudest < 12_000, "{sound:?} peaks at {loudest}");
        }
    }

    #[test]
    fn arriving_and_leaving_do_not_sound_the_same() {
        // Two sounds that a person cannot tell apart are one sound that fires
        // twice as often and says nothing.
        assert_ne!(Sound::Joined.samples(), Sound::Left.samples());
    }

    #[test]
    fn decoding_happens_once() {
        // Same slice, not merely an equal one. A busy channel asks for this on
        // every arrival, and decoding an MP3 per person walking in would be a
        // hitch in the audio thread's feeder every time somebody joined.
        assert!(std::ptr::eq(
            Sound::Joined.samples(),
            Sound::Joined.samples()
        ));
    }

    #[test]
    fn something_that_is_not_audio_decodes_to_silence_rather_than_a_panic() {
        assert_eq!(decode(b"this is not an mp3 file at all"), None);
    }

    mod phrases {
        use super::*;

        #[test]
        fn every_phrase_decodes() {
            // The one thing `include_bytes!` cannot check. It will happily
            // embed a text file, and silence and an unreadable file are
            // indistinguishable once played, which is exactly why this is
            // worth asserting while the files are silent.
            for phrase in [Phrase::Entered, Phrase::Left, Phrase::WelcomeBack] {
                assert!(
                    !phrase.samples().is_empty(),
                    "{phrase:?} decoded to nothing at all"
                );
            }
        }

        #[test]
        fn a_phrase_is_the_length_of_a_sentence() {
            // Wider than the chimes on purpose, and for the opposite reason. A
            // chime has to be over before it intrudes; a sentence has to be
            // long enough to be one. The bounds are set so that a recording
            // dropped in later fits without anybody having to come back here.
            for phrase in [Phrase::Entered, Phrase::Left, Phrase::WelcomeBack] {
                let seconds = phrase.samples().len() as f64 / f64::from(SAMPLE_RATE);
                assert!(
                    (0.5..=3.0).contains(&seconds),
                    "{phrase:?} is {seconds:.2}s long"
                );
            }
        }

        #[test]
        fn the_phrases_are_placeholders_until_somebody_records_them() {
            // Here so the swap cannot happen quietly. Silence is the honest
            // stand-in for a sentence, and it is also the state in which every
            // other test in this file would pass while the feature said
            // nothing at all.
            //
            // When this fails, a recording has landed. Replace it with the two
            // assertions the chimes have and this one was standing in for:
            // that a phrase is audible, and that entering and leaving do not
            // sound the same.
            for phrase in [Phrase::Entered, Phrase::Left, Phrase::WelcomeBack] {
                let loudest = phrase.samples().iter().map(|s| s.abs()).max().unwrap_or(0);
                assert_eq!(
                    loudest, 0,
                    "{phrase:?} has audio in it now, so this test has done its job"
                );
            }
        }

        #[test]
        fn decoding_happens_once() {
            // Same slice, not merely an equal one. These are longer than the
            // chimes, so decoding one per arrival would be a bigger hitch in
            // the feeder than the chimes would have been.
            assert!(std::ptr::eq(
                Phrase::Entered.samples(),
                Phrase::Entered.samples()
            ));
        }
    }

    mod resampling {
        use super::*;

        #[test]
        fn audio_already_at_the_right_rate_is_left_alone() {
            let samples = vec![1, 2, 3, 4];

            assert_eq!(resample(samples.clone(), SAMPLE_RATE), samples);
        }

        #[test]
        fn halving_the_rate_doubles_the_length() {
            let samples = vec![0i16; 100];

            assert_eq!(resample(samples, SAMPLE_RATE / 2).len(), 200);
        }

        #[test]
        fn a_forty_four_one_file_comes_out_at_forty_eight() {
            // The realistic case, and the one that would otherwise play a
            // semitone sharp: every other client's assets are 44.1 kHz.
            let samples = vec![0i16; 44_100];

            let out = resample(samples, 44_100);

            assert_eq!(out.len(), SAMPLE_RATE as usize);
        }

        #[test]
        fn nothing_resamples_to_nothing() {
            assert!(resample(Vec::new(), 44_100).is_empty());
        }

        #[test]
        fn a_ramp_stays_a_ramp() {
            // Linear interpolation, so a straight line has to come out
            // straight. Anything that reads past the end and wraps, or clamps
            // to the first sample, shows up here as a kink.
            let samples: Vec<i16> = (0..100).map(|at| at * 100).collect();

            let out = resample(samples, SAMPLE_RATE / 2);

            assert!(
                out.windows(2).all(|pair| pair[1] >= pair[0]),
                "the ramp is not monotonic: {out:?}"
            );
        }
    }
}
