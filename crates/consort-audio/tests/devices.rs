// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Matching a saved device choice against what is actually plugged in.
//!
//! None of this touches a sound card. The host is a trait, so every awkward
//! case here (the microphone that was unplugged between runs, the machine with
//! no devices at all, ALSA reporting one card four times) is a fixture rather
//! than something that needs the right hardware to reproduce.

use consort_audio::devices::{AudioDevices, Device, Direction, Selection, catalogue, choose};

/// A host with exactly the devices a test says it has.
struct Fake {
    inputs: Vec<Device>,
    outputs: Vec<Device>,
}

impl Fake {
    fn with_inputs(inputs: &[Device]) -> Self {
        Self {
            inputs: inputs.to_vec(),
            outputs: Vec::new(),
        }
    }
}

impl AudioDevices for Fake {
    fn enumerate(&self, direction: Direction) -> Vec<Device> {
        match direction {
            Direction::Input => self.inputs.clone(),
            Direction::Output => self.outputs.clone(),
        }
    }
}

fn device(name: &str) -> Device {
    Device {
        name: name.to_owned(),
        is_default: false,
    }
}

fn default_device(name: &str) -> Device {
    Device {
        name: name.to_owned(),
        is_default: true,
    }
}

mod choosing {
    use super::*;

    #[test]
    fn a_saved_device_that_is_plugged_in_is_the_one_chosen() {
        let available = [default_device("Built-in"), device("Yeti")];

        let chosen = choose(&available, Some("Yeti"));

        assert_eq!(chosen, Selection::Saved(device("Yeti")));
    }

    #[test]
    fn a_saved_device_that_is_gone_falls_back_and_says_which_one_went() {
        let available = [default_device("Built-in")];

        let chosen = choose(&available, Some("Yeti"));

        assert_eq!(
            chosen,
            Selection::Substituted {
                wanted: "Yeti".to_owned(),
                using: default_device("Built-in"),
            },
            "silently listening to a different microphone than the one the \
             settings screen says is the worst of the available outcomes"
        );
    }

    #[test]
    fn nothing_saved_uses_the_host_default() {
        let available = [device("Yeti"), default_device("Built-in")];

        let chosen = choose(&available, None);

        assert_eq!(
            chosen,
            Selection::Default(default_device("Built-in")),
            "the default is whichever device the host flagged, not the first"
        );
    }

    #[test]
    fn nothing_saved_and_no_default_flagged_uses_the_first_device() {
        let available = [device("Yeti"), device("Built-in")];

        let chosen = choose(&available, None);

        assert_eq!(chosen, Selection::Default(device("Yeti")));
    }

    #[test]
    fn no_devices_at_all_is_its_own_answer() {
        assert_eq!(choose(&[], None), Selection::Nothing);
        assert_eq!(
            choose(&[], Some("Yeti")),
            Selection::Nothing,
            "there is nothing to substitute, so this is not a substitution"
        );
    }

    #[test]
    fn an_empty_saved_name_is_treated_as_nothing_saved() {
        let available = [default_device("Built-in")];

        assert_eq!(
            choose(&available, Some("")),
            Selection::Default(default_device("Built-in")),
            "an empty string is a settings file that has been edited by hand, \
             not a device named \"\" that has been unplugged"
        );
        assert_eq!(
            choose(&available, Some("   ")),
            Selection::Default(default_device("Built-in"))
        );
    }

    #[test]
    fn matching_is_exact_rather_than_by_substring() {
        let available = [default_device("Yeti Stereo Microphone")];

        let chosen = choose(&available, Some("Yeti"));

        assert_eq!(
            chosen,
            Selection::Substituted {
                wanted: "Yeti".to_owned(),
                using: default_device("Yeti Stereo Microphone"),
            },
            "the saved name came from this same list, so a near miss is a \
             device that changed rather than a device to guess at"
        );
    }

    #[test]
    fn the_chosen_device_is_reachable_whichever_way_it_was_chosen() {
        let built_in = default_device("Built-in");

        assert_eq!(
            Selection::Saved(device("Yeti")).device(),
            Some(&device("Yeti"))
        );
        assert_eq!(
            Selection::Default(built_in.clone()).device(),
            Some(&built_in)
        );
        assert_eq!(
            Selection::Substituted {
                wanted: "Yeti".to_owned(),
                using: built_in.clone(),
            }
            .device(),
            Some(&built_in)
        );
        assert_eq!(Selection::Nothing.device(), None);
    }
}

mod cataloguing {
    use super::*;

    #[test]
    fn duplicate_names_are_collapsed_keeping_host_order() {
        // ALSA reports the same card under several plugin wrappers, so the raw
        // list repeats itself. Host order matters because it puts the likely
        // default first.
        let host = Fake::with_inputs(&[
            device("Yeti"),
            device("Built-in"),
            device("Yeti"),
            device("Yeti"),
        ]);

        let listed = catalogue(&host, Direction::Input);

        assert_eq!(listed, vec![device("Yeti"), device("Built-in")]);
    }

    #[test]
    fn deduplication_keeps_the_entry_flagged_as_default() {
        // The wrappers are not all equal: one of the repeats is the one the
        // host would hand back as its default, and losing that flag would make
        // the picker say nothing is the default.
        let host = Fake::with_inputs(&[device("Yeti"), default_device("Yeti")]);

        let listed = catalogue(&host, Direction::Input);

        assert_eq!(listed, vec![default_device("Yeti")]);
    }

    #[test]
    fn a_host_with_nothing_plugged_in_lists_nothing() {
        let host = Fake::with_inputs(&[]);

        assert!(catalogue(&host, Direction::Input).is_empty());
    }

    #[test]
    fn the_two_directions_are_asked_separately() {
        let host = Fake {
            inputs: vec![device("Yeti")],
            outputs: vec![device("Headphones")],
        };

        assert_eq!(catalogue(&host, Direction::Input), vec![device("Yeti")]);
        assert_eq!(
            catalogue(&host, Direction::Output),
            vec![device("Headphones")]
        );
    }

    #[test]
    fn alsa_plumbing_is_not_something_anybody_can_pick() {
        // A real ALSA host on this machine offers 21 "input devices", of which
        // 12 are plugin wrappers. Nobody has ever wanted to be recorded by a
        // rate converter, and a picker 21 rows long where 12 are noise is a
        // picker people scroll past rather than read.
        let host = Fake::with_inputs(&[
            device("Discard all samples (playback) or generate zero samples (capture)"),
            device("Rate Converter Plugin Using Libav/FFmpeg Library"),
            device("Rate Converter Plugin Using Samplerate Library"),
            device("Rate Converter Plugin Using Speex Resampler"),
            device("Plugin using Speex DSP (resample, agc, denoise, echo, dereverb)"),
            device("Plugin for channel upmix (4,6,8)"),
            device("Plugin for channel downmix (stereo) with a simple spacialization"),
            device("Yeti Stereo Microphone"),
        ]);

        let listed = catalogue(&host, Direction::Input);

        assert_eq!(listed, vec![device("Yeti Stereo Microphone")]);
    }

    #[test]
    fn a_sound_server_survives_the_filter() {
        // These are the entries a PipeWire desktop most wants selected, so a
        // filter aggressive enough to catch them would be worse than no filter.
        let host = Fake::with_inputs(&[
            default_device("Default Audio Device"),
            device("PipeWire Sound Server"),
            device("PulseAudio Sound Server"),
            device("JACK Audio Connection Kit"),
            device("Default ALSA Output (currently PipeWire Media Server)"),
        ]);

        let listed = catalogue(&host, Direction::Input);

        assert_eq!(listed.len(), 5, "no sound server should have been dropped");
    }

    #[test]
    fn hardware_that_merely_mentions_a_plugin_word_is_kept() {
        // The filter matches how ALSA names its wrappers, not any use of the
        // word. Dropping somebody's actual microphone would be much worse than
        // leaving a rate converter in the list.
        let host = Fake::with_inputs(&[
            device("Plugin Audio Interface"),
            device("Speex Broadcast Mixer"),
        ]);

        assert_eq!(catalogue(&host, Direction::Input).len(), 2);
    }

    #[test]
    fn a_device_with_a_blank_name_is_dropped() {
        // cpal 0.18 has no fallible name lookup; a device whose backend cannot
        // name it displays as nothing. It cannot be saved, chosen or shown, so
        // listing it just puts an empty row in the picker.
        let host = Fake::with_inputs(&[device(""), device("  "), device("Yeti")]);

        assert_eq!(catalogue(&host, Direction::Input), vec![device("Yeti")]);
    }
}

/// What name, if any, gets handed to the audio backend.
///
/// The distinction the whole module turns on: asking a backend for "the
/// default" is not the same as asking it for the device that is currently the
/// default, by name. The first tracks the machine, the second is a photograph
/// of it. On a laptop where plugging in a headset moves the system default,
/// the second keeps recording from the lid.
mod what_to_open {
    use super::*;

    #[test]
    fn nothing_saved_means_the_hosts_own_default() {
        // The first-run case, and the one that has to be right without anybody
        // configuring anything. `None` reaches cpal as
        // `default_input_device()`, so the host decides with whatever it knows
        // about the machine, which is more than a name lookup here knows.
        let available = [default_device("Built-in"), device("Yeti")];

        let chosen = choose(&available, None);

        assert_eq!(chosen.name_to_open(), None);
    }

    #[test]
    fn a_saved_device_is_asked_for_by_name() {
        let available = [default_device("Built-in"), device("Yeti")];

        let chosen = choose(&available, Some("Yeti"));

        assert_eq!(chosen.name_to_open(), Some("Yeti"));
    }

    #[test]
    fn a_saved_device_that_is_also_the_default_is_left_to_the_host() {
        // Same hardware either way today. The difference shows up tomorrow: a
        // person who picked the device that happened to be the system default
        // meant "the usual one", and following the system is what keeps that
        // true when the system changes its mind.
        let available = [default_device("Built-in"), device("Yeti")];

        let chosen = choose(&available, Some("Built-in"));

        assert_eq!(chosen.name_to_open(), None);
    }

    #[test]
    fn a_saved_device_that_is_gone_falls_back_to_the_host_default() {
        // Not to the substituted device's name. `choose` already resolved the
        // fallback to whatever the host flagged, and asking for that by name
        // would just be a slower way of asking for the default.
        let available = [default_device("Built-in")];

        let chosen = choose(&available, Some("Yeti"));

        assert_eq!(chosen.name_to_open(), None);
    }

    #[test]
    fn a_fallback_to_something_the_host_did_not_flag_is_asked_for_by_name() {
        // A host that lists devices but flags none. `choose` falls back to the
        // first, and there is no host default to defer to: asking for one
        // would fail with "there is no audio input device" on a machine that
        // visibly has one.
        let available = [device("Built-in"), device("Yeti")];

        assert_eq!(choose(&available, None).name_to_open(), Some("Built-in"));
        assert_eq!(
            choose(&available, Some("Gone")).name_to_open(),
            Some("Built-in")
        );
    }

    #[test]
    fn an_empty_machine_asks_for_nothing() {
        assert_eq!(choose(&[], None).name_to_open(), None);
    }
}

/// Whether a device that gave a particular answer belongs in the picker.
///
/// The distinction that matters here is between "no" and "not now". A device
/// somebody else is using is the device most likely to be wanted, and dropping
/// it removes it from the list at exactly the moment it is in use.
mod what_to_list {
    use consort_audio::Answer;

    #[test]
    fn a_device_that_can_do_what_we_need_is_listed() {
        assert!(Answer::Yes.worth_listing());
    }

    #[test]
    fn a_device_that_answered_and_cannot_is_dropped() {
        // The whole point of asking. A host that declares a webcam an output
        // is corrected here rather than believed.
        assert!(!Answer::No.worth_listing());
    }

    #[test]
    fn a_device_somebody_else_is_using_is_still_listed() {
        // Busy is not an answer about the device, it is an answer about right
        // now. Consort holding the microphone itself is the ordinary case, and
        // hiding it from the picker that reports it would be absurd.
        assert!(Answer::Busy.worth_listing());
    }

    #[test]
    fn a_device_that_is_not_there_is_dropped() {
        assert!(!Answer::Absent.worth_listing());
    }

    #[test]
    fn a_device_we_are_not_allowed_to_open_is_dropped() {
        // It exists, and offering it would mean offering something that fails
        // on selection with a permissions error nothing on screen explains.
        assert!(!Answer::Forbidden.worth_listing());
    }

    #[test]
    fn an_answer_we_do_not_understand_leaves_the_device_listed() {
        // The two mistakes are not equal. A dud in the list is a bad moment;
        // a missing microphone is somebody concluding the application cannot
        // hear them. Anything unrecognised errs towards showing.
        assert!(Answer::Unclear.worth_listing());
    }
}
