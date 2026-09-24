// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The short sounds a call makes about itself.
//!
//! Recorded, not synthesised like [`crate::tone`]: a rising fifth a computer
//! derived sounds like a modem. `include_bytes!`d so they cannot go missing on
//! somebody else's machine, and decoded on first use. Sources are in
//! `assets/PROVENANCE.md`.

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

/// The sentence played when somebody arrives.
const SAYS_ENTERED: &[u8] = include_bytes!("../assets/voice/entered.mp3");

/// The sentence played when somebody goes.
const SAYS_LEFT: &[u8] = include_bytes!("../assets/voice/left.mp3");

/// "Welcome back." Decodes to silence, because it is the one sentence nobody
/// has recorded. A file dropped in here is the whole of what is left.
const SAYS_WELCOME_BACK: &[u8] = include_bytes!("../assets/voice/welcome-back.mp3");

/// Which sound.
///
/// An enum rather than a path, so a caller cannot ask for a file that is not
/// there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// Somebody arrived.
    Joined,
    /// Somebody left.
    Left,
}

impl Sound {
    /// The samples to play: mono PCM at [`SAMPLE_RATE`]. Empty if the file
    /// will not decode, because a quiet call beats a call that ends.
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
/// A second enum rather than more [`Sound`] variants, because the two are
/// switched on and off separately and the type is what stops a switch being
/// applied to the wrong one. The phrases name nobody: a name needs a
/// synthesiser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phrase {
    /// "Somebody has entered your channel."
    Entered,
    /// "Somebody has left your channel."
    Left,
    /// "Welcome back", to somebody who was away. Plays as silence until a
    /// recording exists.
    WelcomeBack,
}

impl Phrase {
    /// The samples to play: mono PCM at [`SAMPLE_RATE`].
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
/// Shared by both enums so a sound and a sentence cannot disagree about what a
/// failed decode does. It is silence either way.
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
/// `None` for anything that will not decode. The error path exists because
/// `include_bytes!` does not check that the bytes are audio.
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

    // Cloned because `track` borrows the reader and the loop below needs it
    // mutably. Two owned numbers and a codec id.
    let track = format.default_track(TrackType::Audio)?;
    let track_id = track.id;
    let params = track.codec_params.as_ref()?.audio()?.clone();
    let rate = params.sample_rate.unwrap_or(SAMPLE_RATE);

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .ok()?;

    let mut samples = Vec::new();
    let mut interleaved: Vec<i16> = Vec::new();
    // A decode error mid-file ends the sound rather than discarding it: what
    // decoded is still the beginning of the right chime.
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
/// Averaged rather than taking the first channel, which would lose a stereo
/// file that put the sound only on the right.
fn push_mono(interleaved: &[i16], channels: usize, out: &mut Vec<i16>) {
    for frame in interleaved.chunks(channels) {
        let total: i32 = frame.iter().copied().map(i32::from).sum();
        out.push((total / frame.len().max(1) as i32) as i16);
    }
}

/// Stretch or squash `samples` to [`SAMPLE_RATE`].
///
/// Linear, and a no-op for the files this crate ships. It is for a replacement
/// somebody drops in: 44.1 kHz through a 48 kHz mixer plays a semitone sharp
/// rather than failing, which is the kind of bug nobody reports.
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
        // These fire whenever anybody walks in, several times an evening, into
        // headphones.
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
        // every arrival.
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

        /// What the ear integrates, as opposed to what a peak meter shows.
        fn rms(samples: &[i16]) -> f64 {
            if samples.is_empty() {
                return 0.0;
            }
            let total: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
            (total / samples.len() as f64).sqrt()
        }

        #[test]
        fn every_phrase_decodes() {
            // The one thing `include_bytes!` cannot check: it will happily
            // embed a text file.
            for phrase in [Phrase::Entered, Phrase::Left, Phrase::WelcomeBack] {
                assert!(
                    !phrase.samples().is_empty(),
                    "{phrase:?} decoded to nothing at all"
                );
            }
        }

        #[test]
        fn a_phrase_is_the_length_of_a_sentence() {
            // Wider than the chimes, and set so a recording dropped in later
            // fits without anybody having to come back here.
            for phrase in [Phrase::Entered, Phrase::Left, Phrase::WelcomeBack] {
                let seconds = phrase.samples().len() as f64 / f64::from(SAMPLE_RATE);
                assert!(
                    (0.5..=3.0).contains(&seconds),
                    "{phrase:?} is {seconds:.2}s long"
                );
            }
        }

        #[test]
        fn a_recorded_phrase_is_actually_audible() {
            // A file that decodes to the right number of zeroes passes every
            // other test here and plays nothing.
            for phrase in [Phrase::Entered, Phrase::Left] {
                let loudest = phrase.samples().iter().map(|s| s.abs()).max().unwrap_or(0);
                assert!(loudest > 1000, "{phrase:?} peaks at {loudest}");
            }
        }

        #[test]
        fn entering_and_leaving_do_not_sound_the_same() {
            assert_ne!(Phrase::Entered.samples(), Phrase::Left.samples());
        }

        #[test]
        fn a_phrase_is_no_louder_than_the_chime_it_follows() {
            // RMS, not peak like the chimes. Speech is mostly quiet with
            // consonants several times its own average, so a peak that would
            // be alarming from a tone is ordinary from a voice.
            let chime = rms(Sound::Joined.samples());
            for phrase in [Phrase::Entered, Phrase::Left] {
                let level = rms(phrase.samples());
                assert!(
                    level < chime * 2.0,
                    "{phrase:?} sits at {level:.0} against a chime at {chime:.0}"
                );
            }
        }

        #[test]
        fn welcome_back_is_a_placeholder_until_somebody_records_it() {
            // When this fails the recording has landed, and the two tests
            // above should gain a third phrase rather than this one being
            // deleted on its own.
            let loudest = Phrase::WelcomeBack
                .samples()
                .iter()
                .map(|s| s.abs())
                .max()
                .unwrap_or(0);
            assert_eq!(
                loudest, 0,
                "WelcomeBack has audio in it now, so this test has done its job"
            );
        }

        #[test]
        fn decoding_happens_once() {
            // Same slice, not merely an equal one.
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
