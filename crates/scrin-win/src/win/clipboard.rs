//! Clipboard sync: `AddClipboardFormatListener` on a message-only window running on its own
//! thread, reading/writing `CF_UNICODETEXT`, the registered `PNG` format and `CF_DIB`.
//!
//! The window thread only forwards clipboard sequence numbers; content is read on the caller's
//! thread. Echo suppression: after [`ClipboardWatcher::set`] we remember the sequence number our
//! write produced and drop the matching update, so remote content never bounces back.

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT,
    WPARAM,
};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, HWND_MESSAGE,
    MSG, PostMessageW, PostQuitMessage, RegisterClassExW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLIPBOARDUPDATE, WM_CLOSE, WM_DESTROY, WNDCLASSEXW,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::OsContext;
use crate::{ClipboardContent, ClipboardWatcher, Error, Result};

const CLASS: PCWSTR = w!("scrin-clipboard-listener");

thread_local! {
    static SINK: RefCell<Option<Sender<u32>>> = const { RefCell::new(None) };
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            // SAFETY: no arguments; reads a global counter.
            let seq = unsafe { GetClipboardSequenceNumber() };
            SINK.with(|s| {
                if let Some(tx) = s.borrow().as_ref() {
                    let _ = tx.send(seq);
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: `hwnd` is this thread's window; DestroyWindow must run on the owning thread.
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: default processing for every other message, with the arguments we received.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Runs the listener window; sends the HWND (as isize) once ready.
fn listener_thread(seq_tx: Sender<u32>, ready: &Sender<Result<isize>>) {
    SINK.with(|s| *s.borrow_mut() = Some(seq_tx));
    let created = (|| -> Result<HWND> {
        // SAFETY: handle of the current module, never freed.
        let instance = unsafe { GetModuleHandleW(None) }.ctx("GetModuleHandleW")?;
        let class = WNDCLASSEXW {
            cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(u32::MAX),
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..WNDCLASSEXW::default()
        };
        // SAFETY: `class` is fully initialised; the class name is a static wide string.
        if unsafe { RegisterClassExW(&raw const class) } == 0 {
            // SAFETY: reads the calling thread's last-error value.
            let err = unsafe { GetLastError() };
            if err != ERROR_CLASS_ALREADY_EXISTS {
                return Err(super::os_err(
                    "RegisterClassExW",
                    &windows::core::Error::from(err),
                ));
            }
        }
        // SAFETY: message-only window of the class registered above, owned by this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                w!("scrin clipboard"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
        }
        .ctx("CreateWindowExW")?;
        // SAFETY: `hwnd` is a live window of this thread.
        unsafe { AddClipboardFormatListener(hwnd) }.ctx("AddClipboardFormatListener")?;
        Ok(hwnd)
    })();
    let hwnd = match created {
        Ok(h) => h,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(hwnd.0 as isize));
    let mut msg = MSG::default();
    // SAFETY: standard message loop for this thread's window; `msg` is a live out-parameter.
    while unsafe { GetMessageW(&raw mut msg, None, 0, 0) }.0 > 0 {
        // SAFETY: dispatches a message retrieved by GetMessageW.
        unsafe { DispatchMessageW(&raw const msg) };
    }
    // SAFETY: the window is destroyed by now; removing the listener is best effort.
    let _ = unsafe { RemoveClipboardFormatListener(hwnd) };
    SINK.with(|s| *s.borrow_mut() = None);
}

/// RAII for `OpenClipboard`/`CloseClipboard`, retrying while another process holds it.
struct Opened;

impl Opened {
    fn new(hwnd: HWND) -> Result<Self> {
        let mut last = None;
        for _ in 0..20 {
            // SAFETY: `hwnd` is our live listener window (or the call simply fails).
            match unsafe { OpenClipboard(Some(hwnd)) } {
                Ok(()) => return Ok(Self),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(last.map_or(Error::Unsupported("clipboard busy"), |e| {
            super::os_err("OpenClipboard", &e)
        }))
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: paired with a successful OpenClipboard.
        let _ = unsafe { CloseClipboard() };
    }
}

/// Copies the bytes of a clipboard format, if present. Clipboard must be open.
fn read_bytes(format: u32) -> Option<Vec<u8>> {
    // SAFETY: plain query; the clipboard is open.
    unsafe { IsClipboardFormatAvailable(format) }.ok()?;
    // SAFETY: the handle stays owned by the clipboard; we only lock/copy/unlock it.
    let handle = unsafe { GetClipboardData(format) }.ok()?;
    let h = HGLOBAL(handle.0);
    // SAFETY: `h` is a global memory handle from the clipboard.
    let size = unsafe { GlobalSize(h) };
    // SAFETY: as above; locked until GlobalUnlock below.
    let ptr = unsafe { GlobalLock(h) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: GlobalLock returned a pointer to `size` readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), size) }.to_vec();
    // SAFETY: paired with GlobalLock; "not locked anymore" errors are expected and ignored.
    let _ = unsafe { GlobalUnlock(h) };
    Some(bytes)
}

fn write_bytes(format: u32, bytes: &[u8]) -> Result<()> {
    // SAFETY: plain allocation; ownership passes to the clipboard on success.
    let h = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) }.ctx("GlobalAlloc")?;
    // SAFETY: freshly allocated movable block of at least `bytes.len()` bytes.
    let ptr = unsafe { GlobalLock(h) };
    if ptr.is_null() {
        // SAFETY: we still own `h`.
        let _ = unsafe { GlobalFree(Some(h)) };
        return Err(Error::Unsupported("GlobalLock failed"));
    }
    // SAFETY: `ptr` is valid for `bytes.len()` bytes and does not overlap `bytes`.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len()) };
    // SAFETY: paired with GlobalLock above.
    let _ = unsafe { GlobalUnlock(h) };
    // SAFETY: the clipboard is open and emptied by us; on success the system owns `h`.
    if let Err(e) = unsafe { SetClipboardData(format, Some(HANDLE(h.0))) } {
        // SAFETY: ownership did not transfer.
        let _ = unsafe { GlobalFree(Some(h)) };
        return Err(super::os_err("SetClipboardData", &e));
    }
    Ok(())
}

fn utf16_to_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// Clipboard listener + reader/writer.
#[derive(Debug)]
pub struct WinClipboard {
    hwnd: isize,
    rx: Receiver<u32>,
    thread: Option<JoinHandle<()>>,
    png_format: u32,
    own_seq: Option<u32>,
}

impl WinClipboard {
    /// Starts the listener thread.
    pub fn new() -> Result<Self> {
        let (seq_tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("scrin-clipboard".into())
            .spawn(move || listener_thread(seq_tx, &ready_tx))
            .map_err(|e| Error::InvalidInput(format!("clipboard thread: {e}")))?;
        let hwnd = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| Error::Unsupported("clipboard listener did not start"))??;
        // SAFETY: registers (or looks up) a named clipboard format; the string outlives the call.
        let png_format = unsafe { RegisterClipboardFormatW(&HSTRING::from("PNG")) };
        Ok(Self {
            hwnd,
            rx,
            thread: Some(thread),
            png_format,
            own_seq: None,
        })
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut core::ffi::c_void)
    }

    /// Reads the current clipboard content.
    pub fn read(&self) -> Result<ClipboardContent> {
        let _open = Opened::new(self.hwnd())?;
        Ok(ClipboardContent {
            text: read_bytes(u32::from(CF_UNICODETEXT.0)).map(|b| utf16_to_string(&b)),
            png: (self.png_format != 0)
                .then(|| read_bytes(self.png_format))
                .flatten(),
            dib: read_bytes(u32::from(CF_DIB.0)),
        })
    }
}

impl ClipboardWatcher for WinClipboard {
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<ClipboardContent>> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let seq = match self.rx.recv_timeout(left) {
                Ok(s) => s,
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Error::Unsupported("clipboard listener stopped"));
                }
            };
            // Coalesce bursts: only the newest change matters.
            let mut newest = seq;
            while let Ok(s) = self.rx.try_recv() {
                newest = s;
            }
            if Some(newest) == self.own_seq {
                continue;
            }
            let content = self.read()?;
            if !content.is_empty() {
                return Ok(Some(content));
            }
        }
    }

    fn set(&mut self, content: &ClipboardContent) -> Result<()> {
        {
            let _open = Opened::new(self.hwnd())?;
            // SAFETY: the clipboard is open by us; makes our window the owner.
            unsafe { EmptyClipboard() }.ctx("EmptyClipboard")?;
            if let Some(t) = &content.text {
                let bytes: Vec<u8> = t
                    .encode_utf16()
                    .chain([0])
                    .flat_map(u16::to_le_bytes)
                    .collect();
                write_bytes(u32::from(CF_UNICODETEXT.0), &bytes)?;
            }
            if let (Some(p), true) = (&content.png, self.png_format != 0) {
                write_bytes(self.png_format, p)?;
            }
            if let Some(d) = &content.dib {
                write_bytes(u32::from(CF_DIB.0), d)?;
            }
        }
        // SAFETY: plain query after CloseClipboard (which bumps the sequence number).
        self.own_seq = Some(unsafe { GetClipboardSequenceNumber() });
        Ok(())
    }
}

impl Drop for WinClipboard {
    fn drop(&mut self) {
        // SAFETY: posting to our listener window; harmless if it is already gone.
        let _ = unsafe { PostMessageW(Some(self.hwnd()), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_decoding_stops_at_nul() {
        let bytes: Vec<u8> = "hé😀"
            .encode_utf16()
            .chain([0, 0x41])
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(utf16_to_string(&bytes), "hé😀");
    }

    #[test]
    fn set_read_and_echo_suppression() {
        let mut cb = match WinClipboard::new() {
            Ok(c) => c,
            Err(e) if super::super::headless() => return eprintln!("skipped: {e}"),
            Err(e) => panic!("clipboard: {e}"),
        };
        let saved = cb.read().ok();
        let marker = format!("scrin-test-{}-ăîșț", std::process::id());
        cb.set(&ClipboardContent {
            text: Some(marker.clone()),
            ..ClipboardContent::default()
        })
        .expect("set");
        assert_eq!(
            cb.read().expect("read").text.as_deref(),
            Some(marker.as_str())
        );
        // Our own write must not come back as a "remote" change.
        assert_eq!(
            cb.recv_timeout(Duration::from_millis(300)).expect("recv"),
            None
        );
        // Restore the user's text clipboard (best effort; images are not restored).
        if let Some(prev) = saved.filter(|c| c.text.is_some()) {
            let _ = cb.set(&ClipboardContent {
                text: prev.text,
                ..ClipboardContent::default()
            });
        }
    }
}
