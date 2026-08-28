// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Mixing several people into one sound card.
//!
//! The reason these are worth writing down is that the failure mode of this
//! code is not a crash. A mixer that clips wrongly, or drops the wrong end of
//! a queue, or stalls on somebody whose packets are late, produces a call that
//! works and sounds bad, which is the hardest kind of bug to be told about
//! usefully.

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
