// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Reading screens and windows on Windows, through Windows.Graphics.Capture.
//!
//! Excluded from coverage for the reason `x11_host` is: CI has no desktop.
//! What Win32 reports about a window is interpreted in [`crate::win32`] and
//! ordered in [`crate::screens`], both tested as data. Why this API rather than
//! DXGI Desktop Duplication or a crate over either:
//! `docs/adr/0011-capture-on-windows.md`.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{CloseHandle, HMODULE, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MONITORINFOEXW, MonitorFromWindow,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetShellWindow, GetWindow, GetWindowLongPtrW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, Interface, PWSTR};

use crate::capture::{FrameSink, Resolution};
use crate::com::Com;
use crate::pixels::{Picture, from_bgra};
use crate::screens::{ScreenCapture, ShareError, ShareSource, ShareStream, screen, shareable};
use crate::win32::{Rect, TopLevel, monitor_name, seen};

/// How often a share delivers a frame, for the reason `x11_host` gives.
const CAPTURE_FPS: u32 = 15;

/// What the frame pool hands over: BGRA, which is what
/// [`from_bgra`] reads.
const FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;

/// Two buffers, so the compositor has somewhere to put the next frame while
/// this thread is copying the last one.
const BUFFERS: i32 = 2;

/// Screen capture through Windows.Graphics.Capture.
#[derive(Clone, Copy, Debug, Default)]
pub struct WgcHost;

impl ScreenCapture for WgcHost {
    fn sources(&self) -> Result<Vec<ShareSource>, ShareError> {
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            return Err(ShareError::Unsupported);
        }

        let mut offered: Vec<ShareSource> = monitors()
            .iter()
            .map(|monitor| screen(&monitor.name, monitor.rect.width(), monitor.rect.height()))
            .collect();
        offered.extend(shareable(seen(windows()), std::process::id()));
        Ok(offered)
    }

    /// Start capturing `id` on a thread of its own.
    ///
    /// Resolved against a fresh listing first, so a window that closed while
    /// the picker was open is a refusal rather than a capture of nothing. The
    /// thread owns every Direct3D and WinRT object, and answers once the
    /// capture has started so a failure to start arrives here.
    fn open(&self, id: &str, mut on_frame: FrameSink) -> Result<Box<dyn ShareStream>, ShareError> {
        let source = self
            .sources()?
            .into_iter()
            .find(|source| source.id == id)
            .ok_or_else(|| ShareError::Gone {
                source: id.to_owned(),
            })?;

        let stop = Arc::new(AtomicBool::new(false));
        let ours = Arc::clone(&stop);
        let target = id.to_owned();
        let (told, answer) = sync_channel(1);

        let thread = std::thread::Builder::new()
            .name("consort-share".to_owned())
            .spawn(move || {
                let _com = match Com::start() {
                    Ok(com) => com,
                    Err(error) => {
                        let _ = told.send(Err(backend(error)));
                        return;
                    }
                };
                let mut capture = match Capture::start(&target) {
                    Ok(capture) => capture,
                    Err(error) => {
                        let _ = told.send(Err(error));
                        return;
                    }
                };
                let _ = told.send(Ok(capture.size));

                let interval = Duration::from_micros(1_000_000 / u64::from(CAPTURE_FPS));
                let started = Instant::now();
                // Windows only hands over a frame when something on screen
                // changed. The last one is sent again in between, at the same
                // steady rate X11 produces, so somebody joining a call while
                // the shared screen sits still is shown it rather than nothing.
                let mut latest: Option<Picture> = None;

                while !ours.load(Ordering::Relaxed) {
                    let at = Instant::now();
                    if !still_there(&target) {
                        tracing::debug!(%target, "what was being shared is gone");
                        break;
                    }
                    match capture.next() {
                        Ok(Some(picture)) => latest = Some(picture),
                        Ok(None) => {}
                        Err(error) => {
                            tracing::debug!(%error, "the share stopped producing frames");
                            break;
                        }
                    }
                    // Asked again for the reason `x11_host` asks: a stop can
                    // land inside a copy, and the frame it was holding must
                    // not go out after somebody said no more.
                    if ours.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Some(picture) = &latest {
                        on_frame(Picture {
                            timestamp_us: started.elapsed().as_micros() as i64,
                            ..picture.clone()
                        });
                    }
                    if let Some(left) = interval.checked_sub(at.elapsed()) {
                        std::thread::sleep(left);
                    }
                }
                capture.close();
            })
            .map_err(|error| ShareError::Backend(error.to_string()))?;

        let size = match answer.recv() {
            Ok(Ok(size)) => size,
            Ok(Err(error)) => {
                let _ = thread.join();
                return Err(error);
            }
            Err(_) => {
                let _ = thread.join();
                return Err(ShareError::Backend(
                    "the capture thread ended before it said anything".to_owned(),
                ));
            }
        };

        Ok(Box::new(WgcShare {
            source,
            resolution: Resolution {
                width: size.Width as u32,
                height: size.Height as u32,
                fps: CAPTURE_FPS,
            },
            stop,
            thread: Some(thread),
        }))
    }
}

/// A running capture.
struct WgcShare {
    source: ShareSource,
    resolution: Resolution,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ShareStream for WgcShare {
    fn source(&self) -> &ShareSource {
        &self.source
    }

    fn resolution(&self) -> Resolution {
        self.resolution
    }
}

impl Drop for WgcShare {
    /// Stop capturing, and wait until it has, for the reason `X11Share` waits.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Everything one capture holds, on the thread that made it.
struct Capture {
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt: IDirect3DDevice,
    /// What the pool was made for, which a resized window outgrows.
    size: SizeInt32,
    /// A CPU-readable copy of the last frame, kept while the size holds.
    staging: Option<(ID3D11Texture2D, u32, u32)>,
}

impl Capture {
    fn start(id: &str) -> Result<Self, ShareError> {
        let item = item(id)?;
        let size = item.Size().map_err(backend)?;
        let (device, context) = direct3d()?;
        let dxgi: IDXGIDevice = device.cast().map_err(backend)?;
        let winrt: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
            .and_then(|inspectable| inspectable.cast())
            .map_err(backend)?;

        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt, FORMAT, BUFFERS, size)
            .map_err(backend)?;
        let session = pool.CreateCaptureSession(&item).map_err(backend)?;

        // The yellow border Windows draws is left on. It is the one thing on
        // screen that says what is leaving the machine, and turning it off
        // would be Consort deciding somebody does not need to know.
        session.StartCapture().map_err(backend)?;

        Ok(Self {
            item,
            pool,
            session,
            device,
            context,
            winrt,
            size,
            staging: None,
        })
    }

    /// The newest frame, if the screen changed since the last one.
    fn next(&mut self) -> Result<Option<Picture>, ShareError> {
        // An empty pool is an error from WinRT's point of view and nothing
        // from this one: it means the screen has not changed.
        let Ok(frame) = self.pool.TryGetNextFrame() else {
            return Ok(None);
        };
        let content = frame.ContentSize().map_err(backend)?;
        let picture = self.read(&frame, content);
        let _ = frame.Close();

        // A resized window keeps arriving in the old buffer size, cropped or
        // padded, until the pool is told the new one.
        if content != self.size {
            self.pool
                .Recreate(&self.winrt, FORMAT, BUFFERS, content)
                .map_err(backend)?;
            self.size = content;
        }

        picture.map(Some)
    }

    /// Copy `frame` somewhere the CPU can read it, and convert it.
    fn read(
        &mut self,
        frame: &Direct3D11CaptureFrame,
        content: SizeInt32,
    ) -> Result<Picture, ShareError> {
        let surface = frame.Surface().map_err(backend)?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(backend)?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(backend)?;

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        let staging = self.staging_for(&desc)?;

        unsafe { self.context.CopyResource(&staging, &texture) };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(backend)?;

        // The content can be smaller than the buffer it arrived in, for the
        // frame or two after a window shrinks.
        let width = (content.Width.max(0) as u32).min(desc.Width);
        let height = (content.Height.max(0) as u32).min(desc.Height);
        let stride = mapped.RowPitch as usize;
        // SAFETY: a mapped subresource is `RowPitch` bytes for each of the
        // texture's rows until `Unmap`, and `height` is at most that many.
        let bytes = unsafe {
            std::slice::from_raw_parts(mapped.pData.cast::<u8>(), stride * height as usize)
        };
        let picture = from_bgra(width, height, stride, bytes);
        unsafe { self.context.Unmap(&staging, 0) };

        picture.map_err(|error| ShareError::Backend(error.to_string()))
    }

    /// The staging texture for frames shaped like `desc`, made when the shape
    /// changes and kept otherwise.
    fn staging_for(&mut self, desc: &D3D11_TEXTURE2D_DESC) -> Result<ID3D11Texture2D, ShareError> {
        if let Some((texture, width, height)) = &self.staging
            && (*width, *height) == (desc.Width, desc.Height)
        {
            return Ok(texture.clone());
        }

        let readable = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..*desc
        };
        let mut texture = None;
        unsafe {
            self.device
                .CreateTexture2D(&readable, None, Some(&mut texture))
        }
        .map_err(backend)?;
        let texture =
            texture.ok_or_else(|| ShareError::Backend("no staging texture".to_owned()))?;
        self.staging = Some((texture.clone(), desc.Width, desc.Height));
        Ok(texture)
    }

    fn close(self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
        drop(self.item);
    }
}

/// A Direct3D 11 device to receive frames on.
///
/// Hardware first, then WARP, Windows' software rasteriser. A virtual machine
/// or a remote desktop session frequently has no hardware device, and a slow
/// share beats none.
fn direct3d() -> Result<(ID3D11Device, ID3D11DeviceContext), ShareError> {
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let mut context = None;
        let made = unsafe {
            D3D11CreateDevice(
                None::<&IDXGIAdapter>,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        };
        if let (Ok(()), Some(device), Some(context)) = (made, device, context) {
            return Ok((device, context));
        }
    }
    Err(ShareError::Backend(
        "no Direct3D 11 device, not even WARP".to_owned(),
    ))
}

/// The capture item a share id names.
fn item(id: &str) -> Result<GraphicsCaptureItem, ShareError> {
    let gone = || ShareError::Gone {
        source: id.to_owned(),
    };
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(backend)?;

    if let Some(name) = id.strip_prefix("screen:") {
        let monitor = monitors()
            .into_iter()
            .find(|monitor| monitor.name == name)
            .ok_or_else(gone)?;
        return unsafe { interop.CreateForMonitor(monitor.handle) }.map_err(backend);
    }

    let handle: u32 = id
        .strip_prefix("window:")
        .and_then(|number| number.parse().ok())
        .ok_or_else(gone)?;
    let window = hwnd(handle);
    if !unsafe { IsWindow(Some(window)) }.as_bool() {
        return Err(gone());
    }
    unsafe { interop.CreateForWindow(window) }.map_err(|_| gone())
}

/// Whether what `id` names is still there to capture.
///
/// Asked every frame rather than left to the item's `Closed` event, which a
/// window killed with its process did not raise: measured, the share went on
/// sending that window's last frame for as long as it was left running. A
/// frozen picture of something that has gone is the lie #163 was about.
fn still_there(id: &str) -> bool {
    if let Some(name) = id.strip_prefix("screen:") {
        return monitors().iter().any(|monitor| monitor.name == name);
    }
    id.strip_prefix("window:")
        .and_then(|number| number.parse().ok())
        .is_some_and(|handle| unsafe { IsWindow(Some(hwnd(handle))) }.as_bool())
}

/// One monitor, as Windows names and places it.
struct Monitor {
    handle: HMONITOR,
    name: String,
    rect: Rect,
}

/// Every monitor attached to the desktop, in the order Windows lists them.
fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        into: LPARAM,
    ) -> BOOL {
        // SAFETY: `into` is the `Vec` below, alive for the whole enumeration.
        unsafe { &mut *(into.0 as *mut Vec<HMONITOR>) }.push(monitor);
        TRUE
    }

    let mut handles: Vec<HMONITOR> = Vec::new();
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut handles as *mut Vec<HMONITOR> as isize),
        )
    };

    handles
        .into_iter()
        .filter_map(|handle| {
            let mut info = MONITORINFOEXW {
                monitorInfo: MONITORINFO {
                    cbSize: size_of::<MONITORINFOEXW>() as u32,
                    ..Default::default()
                },
                ..Default::default()
            };
            if !unsafe { GetMonitorInfoW(handle, &mut info.monitorInfo) }.as_bool() {
                return None;
            }
            let device = wide(&info.szDevice);
            Some(Monitor {
                handle,
                name: monitor_name(&device).to_owned(),
                rect: rect(info.monitorInfo.rcMonitor),
            })
        })
        .collect()
}

/// Every top-level window Windows lists, topmost first, read into the facts
/// [`seen`] decides with.
///
/// Invisible windows are dropped here rather than there, although `seen`
/// would drop them too: there are hundreds, and each one kept would cost a
/// process lookup to name.
fn windows() -> Vec<TopLevel> {
    unsafe extern "system" fn collect(window: HWND, into: LPARAM) -> BOOL {
        // SAFETY: `into` is the `Vec` below, alive for the whole enumeration.
        unsafe { &mut *(into.0 as *mut Vec<HWND>) }.push(window);
        TRUE
    }

    let mut handles: Vec<HWND> = Vec::new();
    let _ = unsafe {
        EnumWindows(
            Some(collect),
            LPARAM(&mut handles as *mut Vec<HWND> as isize),
        )
    };
    // The desktop itself is a visible, unowned, titled window. Sharing it is
    // what the Screens tab is for.
    let desktop = unsafe { GetShellWindow() };

    handles
        .into_iter()
        .filter(|window| *window != desktop && unsafe { IsWindowVisible(*window) }.as_bool())
        .filter_map(top_level)
        .collect()
}

/// Read one window's facts. `None` for a window that vanished mid-read.
fn top_level(window: HWND) -> Option<TopLevel> {
    let mut bounds = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut bounds as *mut RECT).cast::<c_void>(),
            size_of::<RECT>() as u32,
        )
    }
    .ok()?;

    let mut cloaked = 0u32;
    let _ = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast::<c_void>(),
            size_of::<u32>() as u32,
        )
    };

    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    let extended = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32;

    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let on = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    let _ = unsafe { GetMonitorInfoW(on, &mut monitor) };

    let mut title = [0u16; 512];
    let length = unsafe { GetWindowTextW(window, &mut title) }.max(0) as usize;

    Some(TopLevel {
        handle: window.0 as usize as u32,
        title: String::from_utf16_lossy(&title[..length]),
        app: executable(pid).unwrap_or_default(),
        pid,
        visible: true,
        cloaked: cloaked != 0,
        minimised: unsafe { IsIconic(window) }.as_bool(),
        tool_window: extended & WS_EX_TOOLWINDOW.0 != 0,
        app_window: extended & WS_EX_APPWINDOW.0 != 0,
        owned: unsafe { GetWindow(window, GW_OWNER) }.is_ok_and(|owner| !owner.is_invalid()),
        bounds: rect(bounds),
        monitor: rect(monitor.rcMonitor),
    })
}

/// A process's executable name without its folder or extension, which is what
/// somebody would call the application.
fn executable(pid: u32) -> Option<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut path = [0u16; 1024];
    let mut length = path.len() as u32;
    let read = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    read.ok()?;

    let path = String::from_utf16_lossy(&path[..length as usize]);
    std::path::Path::new(&path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// A window handle out of the 32 bits a share id carries.
///
/// Sign-extended, because that is how Windows widens a handle that a 32-bit
/// process passed it, and so how it is guaranteed to come back.
fn hwnd(handle: u32) -> HWND {
    HWND(handle as i32 as isize as *mut c_void)
}

fn rect(from: RECT) -> Rect {
    Rect {
        left: from.left,
        top: from.top,
        right: from.right,
        bottom: from.bottom,
    }
}

/// A NUL-terminated UTF-16 buffer, as a string.
fn wide(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// Anything Windows said, as a failure worth logging.
fn backend<E: std::fmt::Display>(error: E) -> ShareError {
    ShareError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window closed mid-share ends the share rather than freezing on it.
    ///
    /// Opens Notepad, shares it, and closes its window. The item's `Closed`
    /// event did not fire for a window that went with its process, and the
    /// share went on sending that window's last frame; `still_there` is what
    /// this holds in place.
    ///
    /// The window is closed rather than the process killed: on Windows 11
    /// `notepad.exe` is a launcher that has already exited by the time its
    /// window appears.
    ///
    /// ```sh
    /// cargo test -p consort-video --lib -- --ignored a_closed_window
    /// ```
    #[test]
    #[ignore = "needs a desktop, and opens Notepad on it"]
    fn a_closed_window_ends_the_share_rather_than_freezing_on_it() {
        use std::process::Command;
        use std::sync::Mutex;

        use windows::Win32::Foundation::WPARAM;
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

        use crate::screens::ShareKind;

        let _ = Command::new("notepad.exe").spawn().expect("no Notepad");
        let deadline = Instant::now() + Duration::from_secs(5);
        let shared = loop {
            let found = WgcHost
                .sources()
                .expect("nothing listed")
                .into_iter()
                .find(|source| {
                    source.kind == ShareKind::Window && source.title.contains("Notepad")
                });
            if let Some(found) = found {
                break found;
            }
            assert!(Instant::now() < deadline, "Notepad never appeared");
            std::thread::sleep(Duration::from_millis(100));
        };

        let arrived = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&arrived);
        let share = WgcHost
            .open(
                &shared.id,
                Box::new(move |_| seen.lock().unwrap().push(Instant::now())),
            )
            .expect("Notepad would not share");
        std::thread::sleep(Duration::from_secs(1));

        let handle: u32 = shared.id["window:".len()..].parse().unwrap();
        unsafe { PostMessageW(Some(hwnd(handle)), WM_CLOSE, WPARAM(0), LPARAM(0)) }
            .expect("Notepad would not be asked to close");
        let closed = Instant::now();
        std::thread::sleep(Duration::from_secs(1));
        drop(share);

        let arrived = arrived.lock().unwrap();
        assert!(!arrived.is_empty(), "no frames arrived before the close");
        // A frame interval and a check of slack, then nothing.
        let late = arrived
            .iter()
            .filter(|at| **at > closed + Duration::from_millis(300))
            .count();
        assert_eq!(late, 0, "{late} frames of a closed window went out");
    }
}
