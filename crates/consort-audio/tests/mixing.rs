// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Mixing several people into one sound card.
//!
//! The reason these are worth writing down is that the failure mode of this
//! code is not a crash. A mixer that clips wrongly, or drops the wrong end of
//! a queue, or stalls on somebody whose packets are late, produces a call that
//! works and sounds bad, which is the hardest kind of bug to be told about
//! usefully.

use std::collections::HashMap;

use consort_audio::{FRAME_SAMPLES, JITTER_SAMPLES, Mixing, Voices};

/// Fill one mono buffer of `frames` samples.
fn play(voices: &Voices, frames: usize) -> Vec<i16> {
    let mut mixing = Mixing::new(voices.clone(), 1);
    let mut out = vec![0i16; frames];
    mixing.fill_i16(&mut out);
    out
}

#[test]
fn one_person_comes_out_in_the_order_they_went_in() {
    let voices = Voices::new();
    voices.hear("alice", &[1, 2, 3, 4]);

    assert_eq!(play(&voices, 4), vec![1, 2, 3, 4]);
}

#[test]
fn what_is_played_is_taken_off_the_queue() {
    // Otherwise the same 10 ms plays until the queue overflows, which is a
    // stutter rather than silence and much harder to diagnose.
    let voices = Voices::new();
    voices.hear("alice", &[1, 2, 3, 4]);

    assert_eq!(play(&voices, 2), vec![1, 2]);
    assert_eq!(play(&voices, 2), vec![3, 4]);
    assert_eq!(voices.waiting("alice"), 0);
}

#[test]
fn two_people_talking_at_once_are_summed() {
    let voices = Voices::new();
    voices.hear("alice", &[100, 200]);
    voices.hear("bob", &[10, 20]);

    assert_eq!(play(&voices, 2), vec![110, 220]);
}

#[test]
fn a_sum_too_loud_for_the_range_clips_rather_than_wrapping() {
    // The one that matters. Summed in `i16` this wraps to full-scale in the
    // opposite direction, which is not a quiet artefact: it is a bang, at
    // maximum volume, in somebody's headphones.
    let voices = Voices::new();
    voices.hear("alice", &[30_000, -30_000]);
    voices.hear("bob", &[30_000, -30_000]);

    assert_eq!(play(&voices, 2), vec![i16::MAX, i16::MIN]);
}

#[test]
fn somebody_with_nothing_waiting_contributes_silence() {
    // Rather than stalling the device until their packets turn up. One
    // person's connection hiccuping must not take the whole call with it.
    let voices = Voices::new();
    voices.hear("alice", &[7, 7, 7, 7]);
    voices.hear("bob", &[]);

    assert_eq!(play(&voices, 4), vec![7, 7, 7, 7]);
}

#[test]
fn somebody_who_has_run_out_part_way_through_a_buffer_fills_what_they_have() {
    // The rest of the buffer is still everybody else, rather than being
    // abandoned because one person came up short.
    let voices = Voices::new();
    voices.hear("alice", &[5, 5, 5, 5]);
    voices.hear("bob", &[1, 1]);

    assert_eq!(play(&voices, 4), vec![6, 6, 5, 5]);
}

#[test]
fn nobody_at_all_is_silence_rather_than_whatever_the_buffer_held() {
    let voices = Voices::new();

    assert_eq!(play(&voices, 4), vec![0, 0, 0, 0]);
}

#[test]
fn a_queue_past_the_jitter_cap_drops_its_oldest_audio() {
    // Oldest, for the reason the outgoing queue gives: what is waiting is
    // audio that would be heard late, and late audio puts everything behind it
    // further behind. Dropping the newest would keep the backlog instead.
    let voices = Voices::new();
    let flood: Vec<i16> = (0..JITTER_SAMPLES as i32 + FRAME_SAMPLES as i32)
        .map(|nth| nth as i16)
        .collect();

    voices.hear("alice", &flood);

    assert_eq!(voices.waiting("alice"), JITTER_SAMPLES);
    let played = play(&voices, 1);
    assert_eq!(
        played[0], FRAME_SAMPLES as i16,
        "the oldest frame was the one dropped"
    );
}

#[test]
fn audio_arriving_in_pieces_is_appended_rather_than_replacing() {
    let voices = Voices::new();
    voices.hear("alice", &[1, 2]);
    voices.hear("alice", &[3, 4]);

    assert_eq!(play(&voices, 4), vec![1, 2, 3, 4]);
}

#[test]
fn forgetting_somebody_drops_what_they_had_waiting() {
    // Their stream stopped. Playing out the tail afterwards is a voice
    // continuing after its owner has left the channel.
    let voices = Voices::new();
    voices.hear("alice", &[9, 9]);
    voices.hear("bob", &[1, 1]);

    voices.forget("alice");

    assert_eq!(voices.everyone(), vec!["bob".to_owned()]);
    assert_eq!(play(&voices, 2), vec![1, 1]);
}

#[test]
fn silencing_everything_leaves_nobody_queued() {
    // What deafening does. The subscription pause stops more arriving, but it
    // travels to the SFU and back, and this is what stops the audio already
    // here from playing out underneath somebody who asked for quiet.
    let voices = Voices::new();
    voices.hear("alice", &[9, 9]);
    voices.hear("bob", &[9, 9]);

    voices.silence();

    assert!(voices.everyone().is_empty());
    assert_eq!(play(&voices, 2), vec![0, 0]);
}

#[test]
fn a_deafened_call_can_be_heard_again_without_being_rebuilt() {
    // Undeafening is meant to be instant. Nothing about silencing may leave
    // the mixer unable to take new audio.
    let voices = Voices::new();
    voices.hear("alice", &[9, 9]);
    voices.silence();

    voices.hear("alice", &[4, 4]);

    assert_eq!(play(&voices, 2), vec![4, 4]);
}

#[test]
fn every_channel_of_a_stereo_device_gets_the_same_sample() {
    // A call out of the left speaker only reads as a broken headphone.
    let voices = Voices::new();
    voices.hear("alice", &[11, 22]);

    let mut mixing = Mixing::new(voices, 2);
    let mut out = vec![0i16; 4];
    mixing.fill_i16(&mut out);

    assert_eq!(out, vec![11, 11, 22, 22]);
}

#[test]
fn a_buffer_that_is_not_a_whole_number_of_frames_is_still_filled() {
    // cpal chooses the buffer size and is under no obligation to make it
    // divide by the channel count. Dividing wrongly here would panic inside a
    // realtime callback.
    let voices = Voices::new();
    voices.hear("alice", &[11, 22]);

    let mut mixing = Mixing::new(voices, 2);
    let mut out = vec![0i16; 3];
    mixing.fill_i16(&mut out);

    assert_eq!(out, vec![11, 11, 22]);
}

#[test]
fn a_device_claiming_no_channels_at_all_does_not_divide_by_zero() {
    // Nothing should, but finding out inside a realtime callback is the worst
    // place in the program to find out.
    let voices = Voices::new();
    voices.hear("alice", &[5]);

    let mut mixing = Mixing::new(voices, 0);
    let mut out = vec![0i16; 1];
    mixing.fill_i16(&mut out);

    assert_eq!(out, vec![5]);
}

#[test]
fn float_output_is_scaled_into_the_range_cpal_asks_for() {
    let voices = Voices::new();
    voices.hear("alice", &[i16::MAX, i16::MIN, 0]);

    let mut mixing = Mixing::new(voices, 1);
    let mut out = vec![0f32; 3];
    mixing.fill_f32(&mut out);

    // Divided by 32768 rather than by `i16::MAX`, so the negative extreme
    // lands exactly on -1.0 and the positive one just inside it.
    assert!((out[0] - 0.999_97).abs() < 0.001, "{out:?}");
    assert_eq!(out[1], -1.0);
    assert_eq!(out[2], 0.0);
}

#[test]
fn float_output_clips_at_the_ends_of_the_range_too() {
    let voices = Voices::new();
    voices.hear("alice", &[30_000]);
    voices.hear("bob", &[30_000]);

    let mut mixing = Mixing::new(voices, 1);
    let mut out = vec![0f32; 1];
    mixing.fill_f32(&mut out);

    assert!(out[0] <= 1.0, "{out:?}");
    assert!(out[0] > 0.99, "{out:?}");
}

#[test]
fn every_clone_is_the_same_set_of_queues() {
    // One goes to the call thread and one to the audio thread. If a clone were
    // a copy, the call would fill one and the sound card would drain the
    // other.
    let voices = Voices::new();
    let calling = voices.clone();

    calling.hear("alice", &[3, 3]);

    assert_eq!(play(&voices, 2), vec![3, 3]);
}

/// Sounds this client makes about the call, rather than audio from anybody in
/// it.
///
/// A separate queue with separate rules, and the rules are the reason these
/// are worth writing down: a voice queue drops its oldest samples when it
/// overflows, which for a chime would silently turn a recognisable sound into
/// its own last fragment.
mod sounds {
    use super::*;
    use consort_audio::{SAMPLE_RATE, SOUND_SAMPLES};

    #[test]
    fn a_sound_plays_out_of_the_call_s_own_output() {
        let voices = Voices::new();
        voices.play(&[7, 7, 7, 7]);

        assert_eq!(play(&voices, 4), vec![7, 7, 7, 7]);
    }

    #[test]
    fn a_sound_mixes_with_whoever_is_talking() {
        // Not instead of them. Somebody arriving while a person is mid-word
        // must not mute that word, and it must not clip either: both go into
        // the same accumulator and are clamped once at the end.
        let voices = Voices::new();
        voices.hear("alice", &[100, 100]);
        voices.play(&[5, 5]);

        assert_eq!(play(&voices, 2), vec![105, 105]);
    }

    #[test]
    fn two_sounds_queue_rather_than_overlapping() {
        // Two people arriving at once is two sounds one after another. Summed
        // on top of each other it would be one sound at twice the amplitude,
        // which is a different sound and a louder one.
        let voices = Voices::new();
        voices.play(&[1, 2]);
        voices.play(&[3, 4]);

        assert_eq!(play(&voices, 4), vec![1, 2, 3, 4]);
    }

    #[test]
    fn what_is_played_is_taken_off_the_queue() {
        let voices = Voices::new();
        voices.play(&[1, 2, 3, 4]);

        assert_eq!(play(&voices, 2), vec![1, 2]);
        assert_eq!(voices.sound_waiting(), 2);
        assert_eq!(play(&voices, 2), vec![3, 4]);
        assert_eq!(voices.sound_waiting(), 0);
    }

    #[test]
    fn a_sound_is_not_capped_at_the_jitter_buffer() {
        // The whole reason this is not just another entry in the voice map.
        // `JITTER_SAMPLES` is 120 ms, which is right for speech and would cut
        // all but the tail off anything long enough to recognise.
        let voices = Voices::new();
        let chime = vec![9i16; JITTER_SAMPLES * 3];

        voices.play(&chime);

        assert_eq!(voices.sound_waiting(), chime.len());
    }

    #[test]
    fn a_burst_of_arrivals_cannot_queue_forever() {
        // Somebody rejoining a busy channel must not sit through a minute of
        // chiming for people who were already there.
        let voices = Voices::new();

        for _ in 0..100 {
            voices.play(&vec![1i16; SOUND_SAMPLES]);
        }

        assert_eq!(voices.sound_waiting(), SOUND_SAMPLES);
    }

    #[test]
    fn one_arrival_fits_its_chime_and_its_sentence() {
        // The reason the cap moved from two seconds to six. A chime is about a
        // third of a second and a spoken notification is about a second and a
        // half, and they queue rather than overlap, so the old cap cut the end
        // off the sentence for a single person walking in. The failure would
        // have been a voice that stops mid-word, which reads as a broken
        // recording rather than as a queue that is too short.
        let voices = Voices::new();
        let chime = vec![1i16; SAMPLE_RATE as usize / 3];
        let sentence = vec![2i16; SAMPLE_RATE as usize * 3 / 2];

        voices.play(&chime);
        voices.play(&sentence);

        assert_eq!(
            voices.sound_waiting(),
            chime.len() + sentence.len(),
            "the sentence was truncated by the cap"
        );
    }

    #[test]
    fn the_overflow_is_dropped_from_the_end() {
        // The opposite of what a voice queue does, and deliberately. A
        // truncated chime is still recognisably the chime; one missing its
        // beginning is a click.
        let voices = Voices::new();
        voices.play(&vec![1i16; SOUND_SAMPLES - 2]);

        voices.play(&[2, 2, 3, 3]);

        assert_eq!(voices.sound_waiting(), SOUND_SAMPLES);
        let played = play(&voices, SOUND_SAMPLES);
        assert_eq!(
            &played[SOUND_SAMPLES - 2..],
            &[2, 2],
            "the tail of the queue is not the start of the newest sound"
        );
    }

    #[test]
    fn deafening_drops_whatever_had_not_played_yet() {
        // Undeafening otherwise replays a burst of arrivals for people who
        // have been in the channel for a minute by then.
        let voices = Voices::new();
        voices.play(&[1, 2, 3, 4]);

        voices.silence();

        assert_eq!(voices.sound_waiting(), 0);
        assert_eq!(play(&voices, 4), vec![0, 0, 0, 0]);
    }

    #[test]
    fn every_clone_shares_the_sound_queue_too() {
        // The call thread queues and the audio thread drains, exactly as for
        // voices. A clone that copied would chime into a buffer nobody plays.
        let voices = Voices::new();
        let calling = voices.clone();

        calling.play(&[8, 8]);

        assert_eq!(play(&voices, 2), vec![8, 8]);
    }
}

/// How loud everything is, which is three separate questions with one answer.
///
/// The failure these guard against is the quiet one. Every level here is a
/// multiply that either happens or does not, and a level applied in the wrong
/// place produces a call that works: it is simply at the wrong volume, and
/// nobody can tell whether that is the setting or the person.
mod levels {
    use super::*;
    use consort_audio::{FULL_VOLUME, gain};

    /// A level that is unmistakable in a sample value, and not so low that
    /// rounding is what is being measured.
    const HALF: u8 = 50;

    #[test]
    fn nobody_who_has_chosen_nothing_is_attenuated() {
        // The case that has to be exact rather than merely close. Every call
        // anybody has ever made runs through this path, and a curve that
        // returned 0.999 at the top would quietly resample every sample in
        // every call for nothing.
        let voices = Voices::new();
        voices.hear("alice", &[1000, -1000, 32767, -32768]);

        assert_eq!(play(&voices, 4), vec![1000, -1000, 32767, -32768]);
    }

    #[test]
    fn the_output_level_turns_everybody_down_together() {
        let voices = Voices::new();
        voices.set_output_level(HALF);
        voices.hear("alice", &[1000, 1000]);
        voices.hear("bob", &[1000, 1000]);

        let played = play(&voices, 2);
        let expected = (2000.0 * f64::from(gain(HALF))).round() as i16;
        assert_eq!(played, vec![expected, expected]);
    }

    #[test]
    fn the_curve_is_not_proportional() {
        // Half a slider is not half an amplitude, and this is the assertion
        // that keeps it that way. A proportional control spends its bottom half
        // on changes nobody can hear much of; squaring puts the middle of the
        // travel near the middle of what somebody is listening for.
        assert!(
            gain(HALF) < 0.5,
            "half the slider should be under half the amplitude"
        );
        assert_eq!(gain(FULL_VOLUME), 1.0);
        assert_eq!(gain(0), 0.0);
    }

    #[test]
    fn nothing_can_be_made_louder_than_it_was() {
        // The mixer clips rather than ducking, deliberately, so a control that
        // could push the sum past full scale would distort the whole call to
        // make one part of it louder.
        assert_eq!(gain(200), 1.0);
        assert_eq!(gain(u8::MAX), 1.0);
    }

    #[test]
    fn a_notification_is_measured_against_the_output_and_not_beside_it() {
        // The whole reason the notification level is a percentage of the master
        // rather than its own absolute. Set beside it, turning a call down
        // would leave the chimes where they were, so every arrival would get
        // louder relative to the call the quieter somebody made it.
        let voices = Voices::new();
        voices.set_output_level(HALF);
        voices.set_notification_level(HALF);
        voices.play(&[1000, 1000]);

        let played = play(&voices, 2);
        let both = f64::from(gain(HALF)) * f64::from(gain(HALF));
        let expected = (1000.0 * both).round() as i16;
        assert_eq!(played, vec![expected, expected]);
    }

    #[test]
    fn the_notification_level_leaves_the_people_alone() {
        // And the other half of that: these are two controls, and turning the
        // chimes down must not turn the conversation down with them.
        let voices = Voices::new();
        voices.set_notification_level(0);
        voices.hear("alice", &[1000, 1000]);
        voices.play(&[500, 500]);

        assert_eq!(play(&voices, 2), vec![1000, 1000]);
    }

    #[test]
    fn one_person_can_be_turned_down_without_the_rest() {
        // What the menu beside somebody's name is for. Anybody not named plays
        // at full volume, so the map holds only the people who have been
        // adjusted rather than an entry per person in the call.
        let voices = Voices::new();
        voices.set_person_levels(HashMap::from([("alice".to_owned(), HALF)]));
        voices.hear("alice", &[1000, 1000]);
        voices.hear("bob", &[1000, 1000]);

        let quiet = (1000.0 * f64::from(gain(HALF))).round() as i16;
        assert_eq!(play(&voices, 2), vec![quiet + 1000, quiet + 1000]);
    }

    #[test]
    fn a_persons_level_survives_them_going_quiet_and_coming_back() {
        // The bug this arrangement exists to avoid. Levels are kept apart from
        // the queues because `forget` drops a queue the moment somebody's
        // stream stops, which happens every time a person mutes, and a level
        // stored alongside would be lost with it.
        let voices = Voices::new();
        voices.set_person_levels(HashMap::from([("alice".to_owned(), HALF)]));
        voices.hear("alice", &[1000]);
        let _ = play(&voices, 1);

        voices.forget("alice");
        voices.hear("alice", &[1000]);

        let quiet = (1000.0 * f64::from(gain(HALF))).round() as i16;
        assert_eq!(play(&voices, 1), vec![quiet]);
    }

    #[test]
    fn replacing_the_levels_forgets_whoever_is_no_longer_named() {
        // Wholesale rather than incremental, because these are keyed by
        // membership and a membership is fresh on every join: a map that was
        // only ever added to would grow for the lifetime of the process.
        let voices = Voices::new();
        voices.set_person_levels(HashMap::from([("alice".to_owned(), 0)]));
        voices.set_person_levels(HashMap::new());
        voices.hear("alice", &[1000]);

        assert_eq!(play(&voices, 1), vec![1000]);
    }

    #[test]
    fn silence_is_reachable_at_the_bottom_of_every_slider() {
        // A volume control that cannot reach zero is a volume control somebody
        // drags to the end of and then goes looking for a mute button.
        let voices = Voices::new();
        voices.set_output_level(0);
        voices.hear("alice", &[32767, -32768]);
        voices.play(&[32767, -32768]);

        assert_eq!(play(&voices, 2), vec![0, 0]);
    }
}
