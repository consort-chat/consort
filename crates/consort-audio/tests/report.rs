// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What the settings screen is told about the devices.
//!
//! A picker needs three things and no more: what there is, which one is being
//! used, and whether that is the one that was asked for. The third is the one
//! easy to leave out, and leaving it out is what turns "my headset is selected"
//! into "why is everyone hearing my laptop fan".

use consort_audio::{Device, DeviceList};

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

#[test]
fn a_report_names_the_device_actually_in_use() {
    let available = vec![default_device("Built-in"), device("Yeti")];

    let report = DeviceList::of(available.clone(), Some("Yeti"));

    assert_eq!(report.selected.as_deref(), Some("Yeti"));
    assert_eq!(report.missing, None);
    assert_eq!(report.devices, available, "the whole list is still offered");
}

#[test]
fn nothing_chosen_reports_the_host_default_as_the_one_in_use() {
    let report = DeviceList::of(vec![default_device("Built-in"), device("Yeti")], None);

    assert_eq!(
        report.selected.as_deref(),
        Some("Built-in"),
        "a picker showing nothing selected while audio is flowing is lying"
    );
    assert_eq!(report.missing, None);
}

#[test]
fn a_device_that_has_been_unplugged_is_named_as_missing() {
    let report = DeviceList::of(vec![default_device("Built-in")], Some("Yeti"));

    assert_eq!(report.selected.as_deref(), Some("Built-in"));
    assert_eq!(
        report.missing.as_deref(),
        Some("Yeti"),
        "the screen has to be able to say which device went away"
    );
}

#[test]
fn a_machine_with_nothing_plugged_in_selects_nothing() {
    let report = DeviceList::of(Vec::new(), Some("Yeti"));

    assert_eq!(report.selected, None);
    assert_eq!(
        report.missing, None,
        "with no devices at all there is no substitution to report, only an \
         empty machine"
    );
    assert!(report.devices.is_empty());
}

#[test]
fn the_report_is_camel_case_because_the_frontend_reads_it() {
    let report = DeviceList::of(vec![default_device("Built-in")], Some("Yeti"));

    let json = serde_json::to_string(&report).expect("serialise");

    assert!(json.contains("\"isDefault\""), "got {json}");
    assert!(json.contains("\"missing\""), "got {json}");
    assert!(
        !json.contains("_"),
        "no snake_case should reach the wire: {json}"
    );
}

mod both_directions {
    use consort_audio::{AudioDeviceReport, AudioDevices, Device, Direction};

    struct Fake;

    impl AudioDevices for Fake {
        fn enumerate(&self, direction: Direction) -> Vec<Device> {
            let name = match direction {
                Direction::Input => "Yeti",
                Direction::Output => "Headphones",
            };
            vec![Device {
                name: name.to_owned(),
                is_default: true,
            }]
        }
    }

    #[test]
    fn a_report_asks_the_host_once_per_direction_and_keeps_them_apart() {
        let report = AudioDeviceReport::of(&Fake, Some("Yeti"), None);

        assert_eq!(report.input.selected.as_deref(), Some("Yeti"));
        assert_eq!(report.output.selected.as_deref(), Some("Headphones"));
    }

    #[test]
    fn each_direction_resolves_against_its_own_saved_choice() {
        // Getting these crossed would silently record from the speakers.
        let report = AudioDeviceReport::of(&Fake, Some("Headphones"), Some("Yeti"));

        assert_eq!(report.input.missing.as_deref(), Some("Headphones"));
        assert_eq!(report.output.missing.as_deref(), Some("Yeti"));
    }
}
