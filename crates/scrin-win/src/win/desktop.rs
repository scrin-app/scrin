//! Following the input desktop (Default ↔ Winlogon / secure desktop).
//!
//! Desktop duplication and `SendInput` act on the desktop the calling thread
//! is attached to. When Windows switches to the lock screen or an elevation
//! prompt, the capture and input threads must attach to the new input desktop
//! before re-duplicating. That only succeeds for a SYSTEM process in the
//! user's session (started by scrin-service); otherwise it fails harmlessly
//! and capture resumes when the user's desktop returns.

#![allow(unsafe_code)] // Win32 FFI; each block has a SAFETY comment

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, GetThreadDesktop,
    GetUserObjectInformationW, HDESK, OpenInputDesktop, SetThreadDesktop, UOI_NAME,
};
use windows::Win32::System::Threading::GetCurrentThreadId;

/// `GENERIC_ALL` on the desktop object: switch, read and write input.
const ACCESS: DESKTOP_ACCESS_FLAGS = DESKTOP_ACCESS_FLAGS(0x1000_0000);

/// Name of the desktop `desk` (`Default`, `Winlogon`, …).
fn name_of(desk: HDESK) -> Option<String> {
    let mut buf = [0u16; 64];
    let mut needed = 0u32;
    // SAFETY: `buf` is a live buffer of the stated byte size; `needed` is a live local.
    let ok = unsafe {
        GetUserObjectInformationW(
            HANDLE(desk.0),
            UOI_NAME,
            Some(buf.as_mut_ptr().cast()),
            u32::try_from(size_of_val(&buf)).unwrap_or(0),
            Some(&raw mut needed),
        )
    };
    ok.ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Name of the desktop that currently receives input, if it can be opened.
#[must_use]
pub fn input_desktop_name() -> Option<String> {
    // SAFETY: plain call; the handle is closed below.
    let desk = unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, ACCESS) }.ok()?;
    let name = name_of(desk);
    // SAFETY: handle from OpenInputDesktop, closed once.
    unsafe {
        let _ = CloseDesktop(desk);
    }
    name
}

/// Attaches the calling thread to the input desktop when it differs from the
/// thread's current one. Returns `true` if the thread switched.
///
/// The thread must not own windows or hooks (true for the capture and input
/// threads). The previous desktop handle is left open: the system owns the
/// thread's initial desktop, and later ones are reclaimed at thread exit.
pub fn follow_input_desktop() -> bool {
    // SAFETY: plain call; the handle is either kept (attached) or closed.
    let Ok(input) = (unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, ACCESS) }) else {
        return false;
    };
    // SAFETY: plain call for the current thread.
    let current = unsafe { GetThreadDesktop(GetCurrentThreadId()) }.ok();
    if current.and_then(name_of) == name_of(input) {
        // SAFETY: handle from OpenInputDesktop, closed once.
        unsafe {
            let _ = CloseDesktop(input);
        }
        return false;
    }
    // SAFETY: valid desktop handle; the thread has no windows or hooks.
    if unsafe { SetThreadDesktop(input) }.is_ok() {
        tracing::info!(desktop = ?name_of(input), "following the input desktop");
        true
    } else {
        // SAFETY: not attached, so ours to close.
        unsafe {
            let _ = CloseDesktop(input);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_input_desktop_name_on_an_interactive_machine() {
        // CI agents without a desktop return None; both are fine, it must not panic.
        if let Some(name) = input_desktop_name() {
            assert_ne!(name, "");
        }
    }

    #[test]
    fn following_the_current_desktop_is_a_no_op() {
        // In a normal test run the thread already sits on the input desktop.
        if input_desktop_name().as_deref() == Some("Default") {
            assert!(!follow_input_desktop());
        }
    }
}
