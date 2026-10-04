//! Keyboard and mouse injection with `SendInput`.
//!
//! Keys are sent as physical scancodes (layout independent, what games read); text goes through
//! `KEYEVENTF_UNICODE` (layout mismatch mode). Absolute mouse positions are normalised over one
//! display and mapped onto the virtual desktop, so multi-monitor layouts with negative origins
//! work. `SendInput` is subject to UIPI: injecting into elevated windows needs the agent to run
//! with the same integrity (the SYSTEM-launched agent, S7).

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE,
    MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
    MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{XBUTTON1, XBUTTON2};

use crate::coords::normalized_to_virtual_desk;
use crate::hid::hid_to_scancode;
use crate::{Error, InputInjector, MouseButton, Rect, Result};

/// Builds a keyboard `INPUT`.
fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Builds a mouse `INPUT`.
fn mouse_input(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// The `INPUT` records for a key transition by HID usage.
pub fn key_events(hid_usage: u32, down: bool) -> Result<INPUT> {
    let sc = hid_to_scancode(hid_usage).ok_or_else(|| {
        Error::InvalidInput(format!("HID usage {hid_usage:#x} has no Windows key"))
    })?;
    let up = if down {
        KEYBD_EVENT_FLAGS(0)
    } else {
        KEYEVENTF_KEYUP
    };
    let ext = if sc.extended {
        KEYEVENTF_EXTENDEDKEY
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    Ok(if sc.vk == 0 {
        key_input(0, sc.code, KEYEVENTF_SCANCODE | ext | up)
    } else {
        key_input(sc.vk, sc.code, ext | up)
    })
}

/// The `INPUT` records typing `text` (down+up per UTF-16 unit, surrogate pairs kept together).
#[must_use]
pub fn unicode_events(text: &str) -> Vec<INPUT> {
    let mut v = Vec::with_capacity(text.len() * 2);
    for unit in text.encode_utf16() {
        v.push(key_input(0, unit, KEYEVENTF_UNICODE));
        v.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
    }
    v
}

/// The `INPUT` record for a button transition.
#[must_use]
pub fn button_event(button: MouseButton, down: bool) -> INPUT {
    let (flags, data) = match (button, down) {
        (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (MouseButton::X1, true) => (MOUSEEVENTF_XDOWN, u32::from(XBUTTON1)),
        (MouseButton::X1, false) => (MOUSEEVENTF_XUP, u32::from(XBUTTON1)),
        (MouseButton::X2, true) => (MOUSEEVENTF_XDOWN, u32::from(XBUTTON2)),
        (MouseButton::X2, false) => (MOUSEEVENTF_XUP, u32::from(XBUTTON2)),
    };
    mouse_input(0, 0, data, flags)
}

/// The `INPUT` records for a scroll (vertical then horizontal; zero axes are skipped).
#[must_use]
pub fn wheel_events(dx: i32, dy: i32) -> Vec<INPUT> {
    let mut v = Vec::with_capacity(2);
    // mouseData is a signed delta carried in a DWORD.
    if dy != 0 {
        v.push(mouse_input(0, 0, dy.cast_unsigned(), MOUSEEVENTF_WHEEL));
    }
    if dx != 0 {
        v.push(mouse_input(0, 0, dx.cast_unsigned(), MOUSEEVENTF_HWHEEL));
    }
    v
}

/// `SendInput` injector bound to one display.
#[derive(Debug, Clone)]
pub struct SendInputInjector {
    display: Rect,
    desktop: Rect,
}

impl SendInputInjector {
    /// Binds to a display (absolute moves are normalised over it) on a virtual desktop.
    #[must_use]
    pub fn new(display: Rect, desktop: Rect) -> Self {
        Self { display, desktop }
    }

    /// Binds to the primary display.
    pub fn primary() -> Result<Self> {
        let displays = super::capture_dxgi::list_displays()?;
        let primary = displays
            .iter()
            .find(|d| d.primary)
            .or(displays.first())
            .ok_or(Error::Unsupported("no display attached"))?;
        Ok(Self::new(
            primary.bounds,
            super::capture_dxgi::virtual_desktop()?,
        ))
    }

    /// Re-targets absolute moves at another display.
    pub fn set_display(&mut self, display: Rect, desktop: Rect) {
        self.display = display;
        self.desktop = desktop;
    }

    /// The absolute-move record for a normalised position on the bound display.
    #[must_use]
    pub fn abs_move_event(&self, x: f32, y: f32) -> INPUT {
        let (cx, cy) = normalized_to_virtual_desk(x, y, &self.display, &self.desktop);
        mouse_input(
            cx,
            cy,
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        )
    }

    fn send(inputs: &[INPUT]) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }
        let size = i32::try_from(size_of::<INPUT>()).unwrap_or(i32::MAX);
        // SAFETY: `inputs` is a valid slice of fully initialised INPUT records of `size` bytes.
        let mut sent = unsafe { SendInput(inputs, size) };
        // The input desktop changed (lock screen, elevation prompt): attach
        // to it and try once more. Only possible when started by scrin-service.
        if sent as usize != inputs.len() && super::desktop::follow_input_desktop() {
            // SAFETY: as above.
            sent = unsafe { SendInput(inputs, size) };
        }
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            let e = windows::core::Error::from_thread();
            Err(super::os_err(
                "SendInput (blocked by UIPI or the secure desktop?)",
                &e,
            ))
        }
    }
}

impl InputInjector for SendInputInjector {
    fn key(&mut self, hid_usage: u32, down: bool) -> Result<()> {
        Self::send(&[key_events(hid_usage, down)?])
    }

    fn unicode(&mut self, text: &str) -> Result<()> {
        Self::send(&unicode_events(text))
    }

    fn mouse_move_abs(&mut self, x: f32, y: f32) -> Result<()> {
        Self::send(&[self.abs_move_event(x, y)])
    }

    fn mouse_move_rel(&mut self, dx: i32, dy: i32) -> Result<()> {
        // Relative moves are subject to pointer acceleration, like a real mouse.
        Self::send(&[mouse_input(dx, dy, 0, MOUSEEVENTF_MOVE)])
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<()> {
        Self::send(&[button_event(button, down)])
    }

    fn wheel(&mut self, dx: i32, dy: i32) -> Result<()> {
        Self::send(&wheel_events(dx, dy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::virtual_desk_to_pixel;

    // Reading the active union member is the point of these tests.
    fn ki(i: &INPUT) -> KEYBDINPUT {
        assert_eq!(i.r#type, INPUT_KEYBOARD);
        // SAFETY: type is INPUT_KEYBOARD, so `ki` is the initialised member.
        unsafe { i.Anonymous.ki }
    }

    fn mi(i: &INPUT) -> MOUSEINPUT {
        assert_eq!(i.r#type, INPUT_MOUSE);
        // SAFETY: type is INPUT_MOUSE, so `mi` is the initialised member.
        unsafe { i.Anonymous.mi }
    }

    #[test]
    fn scancode_key_events() {
        let a = ki(&key_events(0x04, true).expect("A"));
        assert_eq!((a.wVk.0, a.wScan), (0, 0x1E));
        assert_eq!(a.dwFlags, KEYEVENTF_SCANCODE);
        let up = ki(&key_events(0x50, false).expect("Left"));
        assert_eq!(up.wScan, 0x4B);
        assert_eq!(
            up.dwFlags,
            KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
        );
        let pause = ki(&key_events(0x48, true).expect("Pause"));
        assert_eq!(pause.wVk.0, 0x13);
        assert_eq!(pause.dwFlags.0 & KEYEVENTF_SCANCODE.0, 0);
        assert!(key_events(0x01, true).is_err());
    }

    #[test]
    fn unicode_keeps_surrogate_pairs() {
        let ev = unicode_events("aă😀");
        assert_eq!(ev.len(), 8); // a, ă, high, low; each down+up
        let units: Vec<u16> = ev.iter().step_by(2).map(|i| ki(i).wScan).collect();
        assert_eq!(units, "aă😀".encode_utf16().collect::<Vec<_>>());
        assert_eq!(ki(&ev[1]).dwFlags, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP);
    }

    #[test]
    fn buttons_and_wheel() {
        let x2 = mi(&button_event(MouseButton::X2, true));
        assert_eq!((x2.dwFlags, x2.mouseData), (MOUSEEVENTF_XDOWN, 2));
        assert_eq!(
            mi(&button_event(MouseButton::Middle, false)).dwFlags,
            MOUSEEVENTF_MIDDLEUP
        );
        let w = wheel_events(30, -120);
        assert_eq!(w.len(), 2);
        assert_eq!(
            (mi(&w[0]).dwFlags, mi(&w[0]).mouseData.cast_signed()),
            (MOUSEEVENTF_WHEEL, -120)
        );
        assert_eq!(
            (mi(&w[1]).dwFlags, mi(&w[1]).mouseData.cast_signed()),
            (MOUSEEVENTF_HWHEEL, 30)
        );
        assert!(wheel_events(0, 0).is_empty());
    }

    #[test]
    fn absolute_move_targets_the_bound_display() {
        let desk = Rect {
            left: -1920,
            top: 0,
            right: 2560,
            bottom: 1440,
        };
        let left = Rect {
            left: -1920,
            top: 360,
            right: 0,
            bottom: 1440,
        };
        let inj = SendInputInjector::new(left, desk);
        let m = mi(&inj.abs_move_event(1.0, 0.0));
        assert_eq!(
            m.dwFlags,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK
        );
        assert_eq!(virtual_desk_to_pixel(m.dx, desk.left, desk.width()), -1);
        assert_eq!(virtual_desk_to_pixel(m.dy, desk.top, desk.height()), 360);
    }
}
