// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Choosing a rendering path WebKitGTK can actually draw on.
//!
//! WebKitGTK composites through a DMABUF buffer it allocates with GBM, and on
//! NVIDIA's driver that allocation fails: `Failed to create GBM buffer of size
//! 1100x720: Invalid argument`, twice, and then a window with a title bar and
//! nothing inside it. Still true on webkit2gtk 2.52.6.
//!
//! What makes it expensive is that nothing else reports it. The process stays
//! up, the frontend is served, and the Rust half logs a textbook boot: session
//! restored, twenty-one channels, key backup state read. Every check a person
//! would think to run says the application is fine while they are looking at an
//! empty rectangle.
//!
//! `WEBKIT_DISABLE_DMABUF_RENDERER=1` picks the older path and costs some
//! compositing performance. This was a command-line workaround for a while, on
//! the reasoning that baking it in would slow the fast path down for everybody
//! to paper over one machine. That reasoning does not survive contact with an
//! installed build: somebody launching from the applications menu has no
//! command line to put it on, and the machine was never the special part. The
//! driver is.
//!
//! So it is applied here, and only on the case that actually fails.

use std::path::Path;

/// What WebKitGTK reads to take the older compositing path.
const DISABLE_DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

/// Present exactly when NVIDIA's kernel module is loaded.
///
/// Both `nvidia` and `nvidia-open` register under this name, and `nouveau`
/// does not, which is the distinction that matters: nouveau goes through Mesa's
/// GBM and allocates the buffer without complaint.
const NVIDIA_MODULE: &str = "/sys/module/nvidia";

/// Set the environment WebKitGTK is about to read.
///
/// # Safety
///
/// Call before starting any thread. `set_var` is unsound alongside a concurrent
/// reader, and the process is still single-threaded this early in `run`.
pub unsafe fn configure() {
    let already_set = std::env::var_os(DISABLE_DMABUF).is_some();

    if !wants_dmabuf_disabled(already_set, Path::new(NVIDIA_MODULE).exists()) {
        return;
    }

    tracing::info!(
        "NVIDIA's driver cannot allocate the buffer WebKitGTK composites through, \
         so the older rendering path is selected. Set {DISABLE_DMABUF}=0 to override."
    );
    // SAFETY: the caller promises this runs before any thread starts.
    unsafe { std::env::set_var(DISABLE_DMABUF, "1") };
}

/// Whether to turn the DMABUF renderer off.
///
/// Anything already in the environment wins, in either direction. Somebody who
/// set it to `0` has a working DMABUF path and is saying so, and quietly
/// putting it back to `1` would leave them with no way to say it.
fn wants_dmabuf_disabled(already_set: bool, nvidia_loaded: bool) -> bool {
    !already_set && nvidia_loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_nvidia_machine_gets_the_older_renderer() {
        assert!(wants_dmabuf_disabled(false, true));
    }

    #[test]
    fn everything_else_keeps_the_fast_path() {
        assert!(!wants_dmabuf_disabled(false, false));
    }

    #[test]
    fn an_explicit_setting_is_never_overridden() {
        // Including the case where it disagrees with us. `WEBKIT_..._RENDERER=0`
        // on an NVIDIA machine is somebody reporting that their driver stack
        // works, and the point of an override is that it overrides.
        assert!(!wants_dmabuf_disabled(true, true));
        assert!(!wants_dmabuf_disabled(true, false));
    }
}
