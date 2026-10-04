//! Native video presentation: decoded BGRA frames are drawn into a Win32
//! child window of the main Tauri window through a D3D11 flip-model
//! (`FLIP_DISCARD`) swap chain. The webview never touches pixels.
//!
//! The UI tells us where the remote screen goes (`scrin_video_rect`, CSS px
//! of the canvas); the child window sits on top of the webview at that rect.
//! Because it covers the canvas, it also captures mouse and keyboard input
//! there and forwards it straight to the engine (no webview round trip).
//!
//! Everything Win32/D3D runs on one dedicated thread that owns the window,
//! pumps its messages and presents latest-frame-wins.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, mpsc};
#[cfg(windows)]
use std::time::Duration;

use scrin_engine::api::VideoFrame;
use tracing::warn;

use crate::bridge::UiInput;

/// Where the video goes, in physical pixels of the parent's client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Receives input captured over the video surface: `(session, input)`.
pub type InputSink = Arc<dyn Fn(String, UiInput) + Send + Sync>;

enum Msg {
    Rect(
        #[cfg_attr(
            not(windows),
            expect(dead_code, reason = "no native surface to place off Windows")
        )]
        Option<Rect>,
    ),
    Frame,
    Clear,
    Quit,
}

/// Thread-safe front of the presenter thread.
pub struct Presenter {
    tx: Mutex<Option<mpsc::Sender<Msg>>>,
    latest: Arc<Mutex<Option<Arc<VideoFrame>>>>,
    queued: Arc<AtomicBool>,
    session: Arc<Mutex<Option<String>>>,
    sink: Arc<OnceLock<InputSink>>,
}

impl std::fmt::Debug for Presenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Presenter").finish_non_exhaustive()
    }
}

impl Presenter {
    /// Starts the presenter thread for parent window `parent` (raw HWND).
    #[must_use]
    pub fn new(parent: isize) -> Self {
        let (tx, rx) = mpsc::channel();
        let latest: Arc<Mutex<Option<Arc<VideoFrame>>>> = Arc::default();
        let queued = Arc::new(AtomicBool::new(false));
        let session: Arc<Mutex<Option<String>>> = Arc::default();
        let sink: Arc<OnceLock<InputSink>> = Arc::default();
        let ctx = ThreadCtx {
            parent,
            rx,
            latest: latest.clone(),
            queued: queued.clone(),
            session: session.clone(),
            sink: sink.clone(),
        };
        let spawned = std::thread::Builder::new()
            .name("scrin-present".into())
            .spawn(move || platform::run(&ctx));
        if let Err(e) = spawned {
            warn!(error = %e, "could not start the video presenter");
        }
        Self {
            tx: Mutex::new(Some(tx)),
            latest,
            queued,
            session,
            sink,
        }
    }

    fn send(&self, m: Msg) {
        if let Some(tx) = self
            .tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            let _ = tx.send(m);
        }
    }

    /// Where input over the video goes. Set once, after the engine starts.
    pub fn set_input_sink(&self, sink: InputSink) {
        let _ = self.sink.set(sink);
    }

    /// Latest-frame-wins: an unpresented older frame is replaced.
    pub fn submit_for(&self, session: &str, frame: Arc<VideoFrame>) {
        {
            let mut s = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            if s.as_deref() != Some(session) {
                *s = Some(session.to_owned());
            }
        }
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(frame);
        if !self.queued.swap(true, Ordering::AcqRel) {
            self.send(Msg::Frame);
        }
    }

    /// Frame without a session (self-test pattern).
    pub fn submit(&self, frame: Arc<VideoFrame>) {
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(frame);
        if !self.queued.swap(true, Ordering::AcqRel) {
            self.send(Msg::Frame);
        }
    }

    /// Show at `rect`, or hide with `None`.
    pub fn set_rect(&self, rect: Option<Rect>) {
        self.send(Msg::Rect(rect));
    }

    /// Session over: hide and forget the last frame.
    pub fn clear(&self) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.send(Msg::Clear);
    }

    pub fn stop(&self) {
        self.send(Msg::Quit);
        self.tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
}

struct ThreadCtx {
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "parent HWND; only the Windows presenter embeds a child window"
        )
    )]
    parent: isize,
    rx: mpsc::Receiver<Msg>,
    latest: Arc<Mutex<Option<Arc<VideoFrame>>>>,
    queued: Arc<AtomicBool>,
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "input over the video only exists with the Windows surface"
        )
    )]
    session: Arc<Mutex<Option<String>>>,
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "input over the video only exists with the Windows surface"
        )
    )]
    sink: Arc<OnceLock<InputSink>>,
}

/// Message-pump cadence while idle.
#[cfg(windows)]
const PUMP: Duration = Duration::from_millis(8);

/// Windows virtual-key → USB HID usage (keyboard page) for the keys a
/// support session needs. Layout-dependent for letters; physical scancode
/// mapping arrives with scrin-win's table.
#[must_use]
pub fn vk_to_hid(vk: u32) -> Option<u32> {
    Some(match vk {
        0x41..=0x5A => vk - 0x41 + 0x04,
        0x31..=0x39 => vk - 0x31 + 0x1E,
        0x30 => 0x27,
        0x0D => 0x28,
        0x1B => 0x29,
        0x08 => 0x2A,
        0x09 => 0x2B,
        0x20 => 0x2C,
        0xBD => 0x2D,
        0xBB => 0x2E,
        0xDB => 0x2F,
        0xDD => 0x30,
        0xDC => 0x31,
        0xBA => 0x33,
        0xDE => 0x34,
        0xC0 => 0x35,
        0xBC => 0x36,
        0xBE => 0x37,
        0xBF => 0x38,
        0x14 => 0x39,
        0x70..=0x7B => vk - 0x70 + 0x3A,
        0x2C => 0x46,
        0x2D => 0x49,
        0x24 => 0x4A,
        0x21 => 0x4B,
        0x2E => 0x4C,
        0x23 => 0x4D,
        0x22 => 0x4E,
        0x27 => 0x4F,
        0x25 => 0x50,
        0x28 => 0x51,
        0x26 => 0x52,
        0x11 | 0xA2 => 0xE0,
        0x10 | 0xA0 => 0xE1,
        0x12 | 0xA4 => 0xE2,
        0x5B => 0xE3,
        0xA3 => 0xE4,
        0xA1 => 0xE5,
        0xA5 => 0xE6,
        0x5C => 0xE7,
        _ => return None,
    })
}

#[cfg(windows)]
mod platform {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, PoisonError, mpsc};

    use scrin_engine::api::VideoFrame;
    use tracing::{debug, warn};
    use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
        ID3D11DeviceContext, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::{
        DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
        DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2,
        IDXGISwapChain1,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, HWND_TOP,
        IDC_ARROW, LoadCursorW, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, SW_HIDE,
        SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, TranslateMessage, WM_ERASEBKGND,
        WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_NOPARENTNOTIFY,
    };
    use windows::core::{Interface, PCWSTR, w};

    use super::{Msg, PUMP, Rect, ThreadCtx, vk_to_hid};
    use crate::bridge::UiInput;

    const CLASS: PCWSTR = w!("ScrinVideoSurface");

    struct InputCtx {
        session: Arc<std::sync::Mutex<Option<String>>>,
        sink: Arc<std::sync::OnceLock<super::InputSink>>,
    }

    thread_local! {
        static INPUT: RefCell<Option<InputCtx>> = const { RefCell::new(None) };
    }

    fn forward(input: UiInput) {
        INPUT.with_borrow(|ctx| {
            let Some(ctx) = ctx else { return };
            let Some(sink) = ctx.sink.get() else { return };
            let session = ctx
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(s) = session {
                sink(s, input);
            }
        });
    }

    // LOWORD/HIWORD unpacking: the low 32 bits carry two signed 16-bit values.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
    fn words(v: isize) -> (i16, i16) {
        let v = v as usize;
        (
            (v & 0xFFFF) as u16 as i16,
            ((v >> 16) & 0xFFFF) as u16 as i16,
        )
    }

    #[expect(clippy::cast_precision_loss)] // client sizes are small
    fn normalized(hwnd: HWND, lp: LPARAM) -> Option<(f32, f32)> {
        let mut rc = RECT::default();
        // SAFETY: `hwnd` is our live child window; `rc` is a valid out-pointer.
        unsafe { GetClientRect(hwnd, &raw mut rc) }.ok()?;
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        if w <= 0 || h <= 0 {
            return None;
        }
        let (x, y) = words(lp.0);
        Some((f32::from(x) / w as f32, f32::from(y) / h as f32))
    }

    /// Window procedure of the video surface: input → engine, no painting
    /// (D3D owns the pixels).
    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        const WM_KEYDOWN: u32 = 0x100;
        const WM_KEYUP: u32 = 0x101;
        const WM_SYSKEYDOWN: u32 = 0x104;
        const WM_SYSKEYUP: u32 = 0x105;
        const WM_MOUSEMOVE: u32 = 0x200;
        const WM_LBUTTONDOWN: u32 = 0x201;
        const WM_LBUTTONUP: u32 = 0x202;
        const WM_RBUTTONDOWN: u32 = 0x204;
        const WM_RBUTTONUP: u32 = 0x205;
        const WM_MBUTTONDOWN: u32 = 0x207;
        const WM_MBUTTONUP: u32 = 0x208;
        const WM_MOUSEWHEEL: u32 = 0x20A;
        const WM_MOUSEHWHEEL: u32 = 0x20E;
        let button = |button: i32, down: bool| {
            if down {
                // SAFETY: plain calls on our own window from its owning thread.
                unsafe {
                    let _ = SetFocus(Some(hwnd));
                    SetCapture(hwnd);
                }
            } else {
                // SAFETY: releasing a capture this thread may hold is always valid.
                let _ = unsafe { ReleaseCapture() };
            }
            forward(UiInput::MouseButton { button, down });
        };
        match msg {
            WM_ERASEBKGND => return LRESULT(1),
            WM_MOUSEMOVE => {
                if let Some((x, y)) = normalized(hwnd, lp) {
                    forward(UiInput::MouseMove {
                        display_id: 0,
                        x,
                        y,
                    });
                }
            }
            WM_LBUTTONDOWN => button(1, true),
            WM_LBUTTONUP => button(1, false),
            WM_RBUTTONDOWN => button(2, true),
            WM_RBUTTONUP => button(2, false),
            WM_MBUTTONDOWN => button(3, true),
            WM_MBUTTONUP => button(3, false),
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                #[expect(clippy::cast_possible_wrap)] // HIWORD of wParam is a signed delta
                let delta = i32::from(words(wp.0 as isize).1);
                let (dx, dy) = if msg == WM_MOUSEWHEEL {
                    (0, delta)
                } else {
                    (delta, 0)
                };
                forward(UiInput::Wheel { dx, dy });
            }
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                if let Some(hid_usage) = u32::try_from(wp.0).ok().and_then(vk_to_hid) {
                    forward(UiInput::Key {
                        hid_usage,
                        down,
                        modifiers: 0,
                        text: None,
                    });
                }
                // Keep Alt/F10 from opening the parent's system menu.
                return LRESULT(0);
            }
            _ => {}
        }
        // SAFETY: default handling for a window we created.
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    fn create_window(parent: isize) -> windows::core::Result<HWND> {
        // SAFETY: a null module name asks for the current executable's handle.
        let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
        let class = WNDCLASSEXW {
            cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).unwrap_or(0),
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            // SAFETY: loading a stock system cursor.
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        // SAFETY: `class` is fully initialised and outlives the call. A second
        // registration (tests, restart) fails harmlessly; creation still works.
        unsafe { RegisterClassExW(&raw const class) };
        // SAFETY: valid class, parent HWND from the live Tauri window; no
        // WM_PARENTNOTIFY so creation never waits on the UI thread.
        unsafe {
            CreateWindowExW(
                WS_EX_NOPARENTNOTIFY,
                CLASS,
                w!("scrin video"),
                WS_CHILD | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                Some(HWND(parent as *mut c_void)),
                None,
                Some(hinstance.into()),
                None,
            )
        }
    }

    fn create_device() -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
        let mut last = None;
        for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
            match try_device(driver) {
                Ok(d) => return Ok(d),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(windows::core::Error::empty))
    }

    fn try_device(
        driver: D3D_DRIVER_TYPE,
    ) -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
        let mut device = None;
        let mut context = None;
        // SAFETY: out-pointers are valid locals; no adapter, default levels.
        unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                Some(&raw mut context),
            )
        }?;
        match (device, context) {
            (Some(d), Some(c)) => Ok((d, c)),
            _ => Err(windows::core::Error::empty()),
        }
    }

    /// The window plus its D3D objects, created on first use.
    pub(super) struct Surface {
        hwnd: HWND,
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        swap: Option<IDXGISwapChain1>,
        size: (u32, u32),
        rect: Option<Rect>,
        visible: bool,
    }

    impl Surface {
        pub(super) fn new(parent: isize) -> windows::core::Result<Self> {
            let hwnd = create_window(parent)?;
            let (device, context) = create_device()?;
            Ok(Self {
                hwnd,
                device,
                context,
                swap: None,
                size: (0, 0),
                rect: None,
                visible: false,
            })
        }

        fn factory(&self) -> windows::core::Result<IDXGIFactory2> {
            let dxgi: IDXGIDevice = self.device.cast()?;
            // SAFETY: valid DXGI device; GetParent of its adapter is the factory.
            unsafe { dxgi.GetAdapter()?.GetParent() }
        }

        fn ensure_swap(&mut self, w: u32, h: u32) -> windows::core::Result<&IDXGISwapChain1> {
            if self.swap.is_some() && self.size != (w, h) {
                if let Some(s) = &self.swap {
                    // SAFETY: no back-buffer references are held between frames.
                    unsafe {
                        s.ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))
                    }?;
                }
                self.size = (w, h);
            }
            if self.swap.is_none() {
                let desc = DXGI_SWAP_CHAIN_DESC1 {
                    Width: w,
                    Height: h,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                    AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                    ..Default::default()
                };
                let factory = self.factory()?;
                // SAFETY: device and window are alive; desc is valid.
                let swap = unsafe {
                    factory.CreateSwapChainForHwnd(
                        &self.device,
                        self.hwnd,
                        &raw const desc,
                        None,
                        None,
                    )
                }?;
                self.swap = Some(swap);
                self.size = (w, h);
            }
            self.swap.as_ref().ok_or_else(windows::core::Error::empty)
        }

        pub(super) fn present(&mut self, f: &VideoFrame) -> windows::core::Result<()> {
            if f.width == 0 || f.height == 0 || f.bgra.len() < (f.stride * f.height) as usize {
                return Ok(());
            }
            let swap = self.ensure_swap(f.width, f.height)?.clone();
            // SAFETY: flip-discard buffer 0 is the current back buffer.
            let tex: ID3D11Texture2D = unsafe { swap.GetBuffer(0) }?;
            // SAFETY: `bgra` holds `stride * height` bytes (checked above) in
            // the back buffer's BGRA8 format; the copy is synchronous.
            unsafe {
                self.context
                    .UpdateSubresource(&tex, 0, None, f.bgra.as_ptr().cast(), f.stride, 0);
            }
            drop(tex);
            // SAFETY: valid swap chain; vsync-locked present.
            unsafe { swap.Present(1, DXGI_PRESENT(0)) }.ok()?;
            self.show();
            Ok(())
        }

        pub(super) fn set_rect(&mut self, rect: Option<Rect>) {
            self.rect = rect;
            match rect {
                Some(_) if self.size != (0, 0) => self.show(),
                Some(_) => {}
                None => self.hide(),
            }
        }

        fn show(&mut self) {
            let Some(r) = self.rect else { return };
            // SAFETY: our child window; z-order top so it covers the webview.
            let _ = unsafe {
                SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOP),
                    r.x,
                    r.y,
                    r.width.max(1),
                    r.height.max(1),
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                )
            };
            self.visible = true;
        }

        pub(super) fn hide(&mut self) {
            if self.visible {
                // SAFETY: our child window.
                let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
                self.visible = false;
            }
        }
    }

    impl Drop for Surface {
        fn drop(&mut self) {
            self.swap = None;
            // SAFETY: destroying our own window on its owning thread.
            let _ = unsafe { DestroyWindow(self.hwnd) };
        }
    }

    fn pump() {
        let mut msg = MSG::default();
        // SAFETY: standard message loop over this thread's queue.
        while unsafe { PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            // SAFETY: `msg` was filled by PeekMessageW.
            unsafe {
                let _ = TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
        }
    }

    pub(super) fn run(ctx: &ThreadCtx) {
        INPUT.set(Some(InputCtx {
            session: ctx.session.clone(),
            sink: ctx.sink.clone(),
        }));
        let mut surface: Option<Surface> = None;
        let mut failed = false;
        loop {
            pump();
            let msg = match ctx.rx.recv_timeout(PUMP) {
                Ok(m) => m,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if matches!(msg, Msg::Quit) {
                break;
            }
            if surface.is_none() && !failed && matches!(msg, Msg::Rect(Some(_)) | Msg::Frame) {
                match Surface::new(ctx.parent) {
                    Ok(s) => surface = Some(s),
                    Err(e) => {
                        failed = true;
                        warn!(error = %e, "native video surface unavailable");
                    }
                }
            }
            let Some(s) = surface.as_mut() else {
                ctx.queued.store(false, Ordering::Release);
                continue;
            };
            match msg {
                Msg::Rect(r) => s.set_rect(r),
                Msg::Frame => {
                    ctx.queued.store(false, Ordering::Release);
                    let frame = ctx
                        .latest
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .clone();
                    if let Some(f) = frame
                        && let Err(e) = s.present(&f)
                    {
                        debug!(error = %e, "present failed; recreating the swap chain");
                        s.swap = None;
                    }
                }
                Msg::Clear => s.hide(),
                Msg::Quit => {}
            }
        }
        drop(surface);
        INPUT.set(None);
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::Win32::UI::WindowsAndMessaging::{
            WINDOW_EX_STYLE, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
        };

        /// A hidden top-level window stands in for the Tauri main window.
        fn parent() -> HWND {
            let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }.expect("module");
            let class = WNDCLASSEXW {
                cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).expect("size"),
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: w!("ScrinTestParent"),
                ..Default::default()
            };
            unsafe { RegisterClassExW(&raw const class) };
            unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WINDOW_EX_STYLE(0),
                    w!("ScrinTestParent"),
                    w!("parent"),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    320,
                    240,
                    None,
                    None,
                    Some(hinstance.into()),
                    None,
                )
            }
            .expect("parent window")
        }

        #[test]
        fn presents_bgra_frames_into_a_flip_discard_child() {
            let p = parent();
            let mut s = Surface::new(p.0 as isize).expect("surface (hardware or WARP)");
            s.set_rect(Some(Rect {
                x: 0,
                y: 0,
                width: 160,
                height: 90,
            }));
            for n in 0..3u64 {
                let mut bgra = Vec::new();
                scrin_engine::backend::render_pattern(64, 36, n, &mut bgra);
                s.present(&VideoFrame {
                    width: 64,
                    height: 36,
                    stride: 64 * 4,
                    bgra,
                    frame_id: 0,
                })
                .expect("present");
            }
            assert_eq!(s.size, (64, 36));
            // A size change resizes the buffers instead of failing.
            let mut bgra = Vec::new();
            scrin_engine::backend::render_pattern(80, 40, 0, &mut bgra);
            s.present(&VideoFrame {
                width: 80,
                height: 40,
                stride: 80 * 4,
                bgra,
                frame_id: 1,
            })
            .expect("present after resize");
            assert_eq!(s.size, (80, 40));
            drop(s);
            unsafe { DestroyWindow(p) }.expect("destroy");
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::sync::PoisonError;
    use std::sync::atomic::Ordering;

    use super::{Msg, ThreadCtx};

    /// No native surface off Windows yet: frames are released as they come
    /// (latest-frame-wins), and input never originates here.
    pub(super) fn run(ctx: &ThreadCtx) {
        while let Ok(msg) = ctx.rx.recv() {
            match msg {
                Msg::Quit => return,
                Msg::Rect(_) => {}
                Msg::Frame | Msg::Clear => {
                    ctx.latest
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take();
                    ctx.queued.store(false, Ordering::Release);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::vk_to_hid;

    #[test]
    fn vk_mapping_covers_letters_digits_and_modifiers() {
        assert_eq!(vk_to_hid(0x41), Some(0x04)); // A
        assert_eq!(vk_to_hid(0x5A), Some(0x1D)); // Z
        assert_eq!(vk_to_hid(0x30), Some(0x27)); // 0
        assert_eq!(vk_to_hid(0x7B), Some(0x45)); // F12
        assert_eq!(vk_to_hid(0x11), Some(0xE0)); // Ctrl
        assert_eq!(vk_to_hid(0xFF), None);
    }
}
