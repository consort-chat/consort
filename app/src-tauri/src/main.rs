// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

// Keeps a console window from appearing behind the app on Windows release
// builds. Debug builds keep it, because that is where the logs go.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    consort_app_lib::run()
}
