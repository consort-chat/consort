// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! COM on the current thread, for the two Windows hosts.
//!
//! Excluded from coverage with them, for the same reason: it only runs where a
//! camera or a desktop does.

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

/// COM on this thread, uninitialised on drop only if this is what started it.
pub(crate) struct Com {
    owned: bool,
}

impl Com {
    /// Join the multithreaded apartment.
    ///
    /// A thread that is already in the single-threaded one, as a Tauri
    /// command on the main thread is, refuses with `RPC_E_CHANGED_MODE`. COM
    /// is usable there all the same, and the call that failed must not be
    /// balanced, so that is not an error.
    pub(crate) fn start() -> windows::core::Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self { owned: false });
        }
        result.ok()?;
        Ok(Self { owned: true })
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.owned {
            unsafe { CoUninitialize() };
        }
    }
}
