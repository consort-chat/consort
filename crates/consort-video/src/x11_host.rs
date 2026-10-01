// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Reading screens and windows off an X11 server.
//!
//! Excluded from coverage, for the reason `v4l_host` is: CI has no display, so
//! every line in here is a line no test can reach. Everything that decides
//! anything is in [`crate::screens`] and is tested as data.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as RandrExt;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, ImageFormat, Window};
use x11rb::rust_connection::RustConnection;

use crate::capture::{FrameSink, Resolution};
use crate::pixels::{Picture, from_bgra};
use crate::screens::{
    ScreenCapture, SeenWindow, ShareError, ShareSource, ShareStream, screen, shareable,
};

/// How often a share captures.
///
/// Fifteen, not thirty. A shared screen is mostly still, and each frame is a
/// whole screen read over the X socket and converted: at 2560x1440 that is
/// 14 MB in and 5 MB out, so the rate is the whole of the cost. Thirty bought
/// nothing visible on text and doubled it.
const CAPTURE_FPS: u32 = 15;

/// Every plane of a pixel.
const ALL_PLANES: u32 = !0;

/// X11 screen capture.
#[derive(Clone, Copy, Debug, Default)]
pub struct X11Host;

impl ScreenCapture for X11Host {
    fn sources(&self) -> Result<Vec<ShareSource>, ShareError> {
        let (conn, root) = connect()?;

        let mut offered = monitors(&conn, root)?;
        offered.extend(shareable(windows(&conn, root)?, std::process::id()));
        Ok(offered)
    }

    fn open(&self, id: &str, mut on_frame: FrameSink) -> Result<Box<dyn ShareStream>, ShareError> {
        // Resolved and measured before a thread exists, so a window that has
        // already closed is a refusal the caller can report rather than a
        // capture that starts and produces nothing.
        let (conn, root) = connect()?;
        let source = find(&conn, root, id)?;
        let target = drawable(root, id)?;
        let Resolution { width, height, .. } = geometry(&conn, target)?;

        let stop = Arc::new(AtomicBool::new(false));
        let ours = Arc::clone(&stop);
        let interval = Duration::from_micros(1_000_000 / u64::from(CAPTURE_FPS));
        let started = Instant::now();

        // Its own connection, because this one is about to leave the thread
        // that made it. X11 connections are not built to be shared.
        let thread = std::thread::Builder::new()
            .name("consort-share".to_owned())
            .spawn(move || {
                let Ok((conn, _)) = connect() else { return };
                while !ours.load(Ordering::Relaxed) {
                    let at = Instant::now();
                    match grab(&conn, target, width, height, started) {
                        // Asked again, because a grab takes long enough for a
                        // stop to land inside one. Delivering the frame it was
                        // already holding would put a picture of somebody's
                        // screen into the call after they said no more.
                        Ok(_) if ours.load(Ordering::Relaxed) => return,
                        Ok(picture) => on_frame(picture),
                        Err(error) => {
                            // The window closed, or the server went away.
                            // Either way there is nothing to capture again, so
                            // the thread ends rather than retrying forever.
                            tracing::debug!(%error, "the share stopped producing frames");
                            return;
                        }
                    }
                    if let Some(left) = interval.checked_sub(at.elapsed()) {
                        std::thread::sleep(left);
                    }
                }
            })
            .map_err(|error| ShareError::Backend(error.to_string()))?;

        Ok(Box::new(X11Share {
            source,
            resolution: Resolution {
                width,
                height,
                fps: CAPTURE_FPS,
            },
            stop,
            thread: Some(thread),
        }))
    }
}

/// A running X11 capture.
struct X11Share {
    source: ShareSource,
    resolution: Resolution,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ShareStream for X11Share {
    fn source(&self) -> &ShareSource {
        &self.source
    }

    fn resolution(&self) -> Resolution {
        self.resolution
    }
}

impl Drop for X11Share {
    /// Stop capturing, and wait until it has.
    ///
    /// The wait is the point. Somebody pressing stop has said no more of their
    /// screen may leave this machine, and a thread still in `grab` would get
    /// one more frame out after that. It is bounded by one capture interval.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Open a connection and find the screen's root window.
fn connect() -> Result<(RustConnection, Window), ShareError> {
    let (conn, screen_num) = x11rb::connect(None).map_err(|_| ShareError::NoX11)?;
    let root = conn.setup().roots[screen_num].root;
    Ok((conn, root))
}

/// Every monitor RandR reports, as something to share.
fn monitors(conn: &RustConnection, root: Window) -> Result<Vec<ShareSource>, ShareError> {
    let monitors = conn
        .randr_get_monitors(root, true)
        .map_err(backend)?
        .reply()
        .map_err(backend)?;

    monitors
        .monitors
        .iter()
        .map(|monitor| {
            let name = atom_name(conn, monitor.name)?;
            Ok(screen(
                &name,
                u32::from(monitor.width),
                u32::from(monitor.height),
            ))
        })
        .collect()
}

/// Every window the window manager lists, in stacking order.
///
/// Bottom first, which is how `_NET_CLIENT_LIST_STACKING` reports it and what
/// [`shareable`] expects. A window that vanishes between being listed and being
/// read is skipped rather than failing the list.
fn windows(conn: &RustConnection, root: Window) -> Result<Vec<SeenWindow>, ShareError> {
    let listed = cardinals(conn, root, intern(conn, b"_NET_CLIENT_LIST_STACKING")?)?;

    Ok(listed
        .into_iter()
        .filter_map(|id| seen(conn, id).ok())
        .collect())
}

/// Read one window's properties.
fn seen(conn: &RustConnection, id: Window) -> Result<SeenWindow, ShareError> {
    let states = cardinals(conn, id, intern(conn, b"_NET_WM_STATE")?)?;
    let kinds = cardinals(conn, id, intern(conn, b"_NET_WM_WINDOW_TYPE")?)?;
    let fullscreen = intern(conn, b"_NET_WM_STATE_FULLSCREEN")?;
    let skip_taskbar = intern(conn, b"_NET_WM_STATE_SKIP_TASKBAR")?;
    let normal_type = intern(conn, b"_NET_WM_WINDOW_TYPE_NORMAL")?;
    let Resolution { width, height, .. } = geometry(conn, id)?;

    Ok(SeenWindow {
        id,
        title: title(conn, id)?,
        app: wm_class(conn, id)?,
        pid: cardinals(conn, id, intern(conn, b"_NET_WM_PID")?)?
            .first()
            .copied(),
        // An absent type means normal, which is what the spec says and what
        // plenty of applications rely on by never setting one.
        normal: kinds.is_empty() || kinds.contains(&normal_type),
        skip_taskbar: states.contains(&skip_taskbar),
        fullscreen: states.contains(&fullscreen),
        width,
        height,
    })
}

/// The id of a named atom, creating nothing.
fn intern(conn: &RustConnection, name: &[u8]) -> Result<Atom, ShareError> {
    Ok(conn
        .intern_atom(false, name)
        .map_err(backend)?
        .reply()
        .map_err(backend)?
        .atom)
}

/// What an atom is called, for naming a monitor.
fn atom_name(conn: &RustConnection, atom: Atom) -> Result<String, ShareError> {
    let reply = conn
        .get_atom_name(atom)
        .map_err(backend)?
        .reply()
        .map_err(backend)?;
    Ok(String::from_utf8_lossy(&reply.name).into_owned())
}

/// A property read as a list of 32-bit values, empty when it is not set.
///
/// Serves `_NET_CLIENT_LIST_STACKING`, `_NET_WM_STATE`, `_NET_WM_WINDOW_TYPE`
/// and `_NET_WM_PID`, which are all lists of window ids, atoms or cardinals
/// and all the same read.
fn cardinals(
    conn: &RustConnection,
    window: Window,
    property: Atom,
) -> Result<Vec<u32>, ShareError> {
    let reply = conn
        .get_property(false, window, property, AtomEnum::ANY, 0, u32::MAX / 4)
        .map_err(backend)?
        .reply()
        .map_err(backend)?;

    Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
}

/// A window's `_NET_WM_NAME`, falling back to `WM_NAME`.
///
/// Both, because `_NET_WM_NAME` is the UTF-8 one and the one modern toolkits
/// set, and `WM_NAME` is all an older application has.
fn title(conn: &RustConnection, window: Window) -> Result<String, ShareError> {
    let utf8 = intern(conn, b"UTF8_STRING")?;
    let modern = text(conn, window, intern(conn, b"_NET_WM_NAME")?, utf8)?;
    if !modern.is_empty() {
        return Ok(modern);
    }
    text(
        conn,
        window,
        AtomEnum::WM_NAME.into(),
        AtomEnum::STRING.into(),
    )
}

/// The class half of `WM_CLASS`, which names the application.
///
/// Two NUL-separated strings, instance then class. The class is the one that
/// is capitalised for a person to read.
fn wm_class(conn: &RustConnection, window: Window) -> Result<String, ShareError> {
    let raw = text(
        conn,
        window,
        AtomEnum::WM_CLASS.into(),
        AtomEnum::STRING.into(),
    )?;
    Ok(raw
        .split('\0')
        .rfind(|part| !part.is_empty())
        .unwrap_or_default()
        .to_owned())
}

/// A text property, empty when it is not set.
fn text(
    conn: &RustConnection,
    window: Window,
    property: Atom,
    kind: Atom,
) -> Result<String, ShareError> {
    let reply = conn
        .get_property(false, window, property, kind, 0, 1024)
        .map_err(backend)?
        .reply()
        .map_err(backend)?;

    // Trailing NULs and the separator in `WM_CLASS` both survive this, which
    // `wm_class` wants and `title` trims.
    Ok(String::from_utf8_lossy(&reply.value)
        .trim_end_matches('\0')
        .to_owned())
}

/// How large a drawable currently is.
fn geometry(conn: &RustConnection, target: Window) -> Result<Resolution, ShareError> {
    let reply = conn
        .get_geometry(target)
        .map_err(backend)?
        .reply()
        .map_err(|_| ShareError::Gone {
            source: format!("window:{target}"),
        })?;

    Ok(Resolution {
        width: u32::from(reply.width),
        height: u32::from(reply.height),
        fps: CAPTURE_FPS,
    })
}

/// Which drawable a source id names.
///
/// A screen is the root window, because RandR monitors are rectangles of it
/// and `GetImage` on the root is how a region of one is read. A window is its
/// own drawable.
fn drawable(root: Window, id: &str) -> Result<Window, ShareError> {
    if id.starts_with("screen:") {
        return Ok(root);
    }
    id.strip_prefix("window:")
        .and_then(|number| number.parse().ok())
        .ok_or_else(|| ShareError::Gone {
            source: id.to_owned(),
        })
}

/// The source `id` names, as it is right now.
///
/// Read again rather than carried from the picker, because the picker may have
/// been open for a while and the title is what the indicator will show.
fn find(conn: &RustConnection, root: Window, id: &str) -> Result<ShareSource, ShareError> {
    X11Host
        .sources_of(conn, root)?
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| ShareError::Gone {
            source: id.to_owned(),
        })
}

impl X11Host {
    /// [`ScreenCapture::sources`] against a connection that is already open.
    fn sources_of(
        self,
        conn: &RustConnection,
        root: Window,
    ) -> Result<Vec<ShareSource>, ShareError> {
        let mut offered = monitors(conn, root)?;
        offered.extend(shareable(windows(conn, root)?, std::process::id()));
        Ok(offered)
    }
}

/// Read one frame and convert it.
fn grab(
    conn: &RustConnection,
    target: Window,
    width: u32,
    height: u32,
    started: Instant,
) -> Result<Picture, ShareError> {
    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            target,
            0,
            0,
            width as u16,
            height as u16,
            ALL_PLANES,
        )
        .map_err(backend)?
        .reply()
        .map_err(|_| ShareError::Gone {
            source: format!("window:{target}"),
        })?;

    // X pads each row to a four-byte boundary, and at 32 bits a pixel that is
    // already the width. Derived from the reply rather than assumed, because a
    // 16-bit visual would not be.
    let stride = reply.data.len() / height.max(1) as usize;

    let mut picture = from_bgra(width, height, stride, &reply.data)
        .map_err(|error| ShareError::Backend(error.to_string()))?;
    // Monotonic and from this capture's own start. libwebrtc paces and orders
    // frames by this, so a constant makes its rate control guess.
    picture.timestamp_us = started.elapsed().as_micros() as i64;
    Ok(picture)
}

/// Anything the server or the connection said, as a failure worth logging.
fn backend<E: std::fmt::Display>(error: E) -> ShareError {
    ShareError::Backend(error.to_string())
}
