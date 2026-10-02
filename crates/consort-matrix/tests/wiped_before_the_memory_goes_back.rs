// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! H8: secrets are overwritten before their memory is handed back.
//!
//! Its own binary because it installs a global allocator, and because the
//! watch below cannot tell one test's frees from another's.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use consort_matrix::{Credentials, StoreKey};

/// The bytes looked for in a block on its way back to the allocator.
///
/// Long enough not to occur by chance, and the same length as the key so one
/// needle serves both.
const NEEDLE: [u8; 32] = *b"hunter2-hunter2-hunter2-hunter2-";

/// The needle as the secret store would hold it.
fn encoded_needle() -> String {
    STANDARD.encode(NEEDLE)
}

static WATCHING: AtomicBool = AtomicBool::new(false);
static FOUND: AtomicBool = AtomicBool::new(false);

/// The watch is one global and these tests run on several threads, so a
/// needle one test is still holding would otherwise be seen by another.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Reads each block as it is freed, which is the last moment the bytes are
/// still there to read and the memory is still ours to read.
struct Watch;

thread_local! {
    /// Blocks this thread has taken. `const` init so that reading it from
    /// inside the allocator cannot itself allocate.
    static TAKEN: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Watch {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = TAKEN.try_with(|taken| taken.set(taken.get() + 1));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // Only blocks the size of what was put in them, so that nothing with
        // an uninitialised tail is ever read.
        if WATCHING.load(Ordering::Relaxed) && layout.size() == NEEDLE.len() {
            let block = unsafe { std::slice::from_raw_parts(pointer, layout.size()) };
            if block == NEEDLE {
                FOUND.store(true, Ordering::Relaxed);
            }
        }
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Watch = Watch;

/// Run `body` with no other test in this binary holding a needle.
fn alone<T>(body: impl FnOnce() -> T) -> T {
    let _held = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|held| held.into_inner());
    body()
}

/// How many blocks `go` took on this thread.
fn blocks_taken(go: impl FnOnce()) -> usize {
    let before = TAKEN.with(Cell::get);
    go();
    TAKEN.with(Cell::get) - before
}

/// Run `go`, and say whether the needle was in any block freed while it ran.
fn freed_holding_the_secret(go: impl FnOnce()) -> bool {
    FOUND.store(false, Ordering::Relaxed);
    WATCHING.store(true, Ordering::Relaxed);
    go();
    WATCHING.store(false, Ordering::Relaxed);
    FOUND.load(Ordering::Relaxed)
}

#[test]
fn the_watch_itself_sees_a_secret_nobody_wiped() {
    // Without this the three below prove only that the allocator is deaf.
    alone(|| {
        let unwiped = String::from_utf8(NEEDLE.to_vec()).expect("the needle is text");

        assert!(freed_holding_the_secret(move || drop(unwiped)));
    });
}

#[test]
fn a_store_key_is_wiped_before_its_memory_goes_back() {
    alone(|| {
        let key = Box::new(StoreKey::decode(&encoded_needle()).expect("32 bytes"));

        assert!(!freed_holding_the_secret(move || drop(key)));
    });
}

#[test]
fn decoding_a_key_puts_no_copy_of_it_on_the_heap() {
    // Counted rather than watched on the way out. A decode buffer is sized
    // from an estimate, so it is a block with a tail nothing wrote, and a
    // watch that read it would be reading uninitialised memory.
    alone(|| {
        let encoded = encoded_needle();
        let mut decoded = None;

        let taken = blocks_taken(|| decoded = StoreKey::decode(&encoded));

        assert!(decoded.is_some(), "a key this crate just encoded");
        assert_eq!(taken, 0, "decode put the key on the heap");
    });
}

#[test]
fn a_password_is_wiped_before_its_memory_goes_back() {
    alone(|| {
        let credentials = Credentials {
            server: "example.org".to_owned(),
            username: "ada".to_owned(),
            password: String::from_utf8(NEEDLE.to_vec()).expect("the needle is text"),
        };

        assert!(!freed_holding_the_secret(move || drop(credentials)));
    });
}
