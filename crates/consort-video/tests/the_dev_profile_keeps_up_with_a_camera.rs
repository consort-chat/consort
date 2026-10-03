// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The packages a camera frame is arithmetic inside, and what the dev profile
//! builds them at.
//!
//! At `opt-level = 0` a 720p MJPEG frame costs 159 ms of CPU, so a dev build
//! reads 5.5 frames a second from a camera offering 30 and holds a core at
//! 100% doing it. The measurements are in `docs/PERFORMANCE.md`.

use std::path::PathBuf;

/// Every package a captured frame is arithmetic inside.
///
/// `zune-jpeg` is the larger half of the cost and is a dependency, so the
/// `[profile.dev.package."*"]` block does not reach it: that one sets
/// debuginfo, which is not speed.
const ON_THE_HOT_PATH: [&str; 2] = ["consort-video", "zune-jpeg"];

/// What a dev build has to compile those at.
///
/// 2 rather than 1 because 1 keeps up with the camera but spends three times
/// the CPU doing it, and 2 is what the release build already costs.
const ENOUGH: u32 = 2;

#[test]
fn the_dev_profile_optimizes_every_package_a_camera_frame_passes_through() {
    for package in ON_THE_HOT_PATH {
        let level = dev_opt_level(package).unwrap_or_else(|| {
            panic!(
                "the dev profile sets no opt-level for {package}, so a dev build \
                 converts camera frames at opt-level 0"
            )
        });

        assert!(
            level >= ENOUGH,
            "the dev profile builds {package} at opt-level {level}, which costs \
             more CPU per frame than the release build spends"
        );
    }
}

/// The `opt-level` the dev profile sets for one package, if it sets one.
fn dev_opt_level(package: &str) -> Option<u32> {
    let manifest = workspace_manifest();
    let section = [
        format!("[profile.dev.package.{package}]"),
        format!("[profile.dev.package.\"{package}\"]"),
    ]
    .into_iter()
    .find_map(|header| section(&manifest, &header))?;

    value(&section, "opt-level")?.parse().ok()
}

/// The text of one section of a TOML file, up to the next header.
fn section(document: &str, header: &str) -> Option<String> {
    let after = document.split_once(&format!("\n{header}\n"))?.1;

    Some(match after.split_once("\n[") {
        Some((body, _)) => body.to_owned(),
        None => after.to_owned(),
    })
}

/// One `key = value` out of a TOML section, with any quotes taken off.
fn value(section: &str, key: &str) -> Option<String> {
    section
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(key)?.trim_start().strip_prefix('='))
        .map(|value| value.trim().trim_matches('"').to_owned())
}

/// The workspace manifest, which is where a profile can be set at all.
fn workspace_manifest() -> String {
    read("Cargo.toml")
}

/// One file in the repository, by its path from the root.
fn read(relative: &str) -> String {
    let path = repository().join(relative);

    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The repository root.
fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root is reachable from the manifest directory")
}
