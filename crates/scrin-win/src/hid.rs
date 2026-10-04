//! USB HID usage → Windows "Set 1" scancode mapping.
//!
//! Usages are `page << 16 | id`. Page 0x07 (Keyboard/Keypad) covers the 104/105/106/107-key
//! layouts including the ISO and JIS extra keys; page 0x0C (Consumer) covers media keys. A bare
//! id without a page is treated as page 0x07 (what browsers' `KeyboardEvent.code` tables use).
//!
//! The scancode is what `SendInput` takes with `KEYEVENTF_SCANCODE`; `extended` sets
//! `KEYEVENTF_EXTENDEDKEY` (the 0xE0 prefix). Pause (`E1 1D 45`) and Num Lock (`45`, which the
//! keyboard driver flags as extended) cannot be expressed unambiguously as a single scancode, so
//! they carry a virtual-key override that the injector sends instead.

/// HID usage page for keyboards.
pub const PAGE_KEYBOARD: u32 = 0x07;
/// HID usage page for consumer controls (media keys).
pub const PAGE_CONSUMER: u32 = 0x0C;

/// A Windows scancode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Scancode {
    /// Set 1 make code (low byte).
    pub code: u16,
    /// Needs the 0xE0 prefix (`KEYEVENTF_EXTENDEDKEY`).
    pub extended: bool,
    /// Virtual key to send instead of the scancode (0 = use the scancode).
    pub vk: u16,
}

impl Scancode {
    const fn n(code: u16) -> Self {
        Self {
            code,
            extended: false,
            vk: 0,
        }
    }
    const fn e(code: u16) -> Self {
        Self {
            code,
            extended: true,
            vk: 0,
        }
    }
    /// Pause/Break (`VK_PAUSE`).
    pub const PAUSE: Self = Self {
        code: 0x45,
        extended: false,
        vk: 0x13,
    };
    /// Num Lock (`VK_NUMLOCK`, extended).
    pub const NUM_LOCK: Self = Self {
        code: 0x45,
        extended: true,
        vk: 0x90,
    };
}

/// Keyboard page (0x07) table indexed by usage id 0x00..=0xE7. `None` = no key.
const KEYBOARD: [Option<Scancode>; 0xE8] = {
    let mut t: [Option<Scancode>; 0xE8] = [None; 0xE8];
    // Letters a..z (0x04..=0x1D).
    let letters: [u16; 26] = [
        0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18,
        0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
    ];
    let mut i = 0;
    while i < 26 {
        t[0x04 + i] = Some(Scancode::n(letters[i]));
        i += 1;
    }
    // Digits 1..9, 0 (0x1E..=0x27) -> 0x02..=0x0B.
    let mut d: u16 = 0;
    while d < 10 {
        t[0x1E + d as usize] = Some(Scancode::n(0x02 + d));
        d += 1;
    }
    t[0x28] = Some(Scancode::n(0x1C)); // Enter
    t[0x29] = Some(Scancode::n(0x01)); // Escape
    t[0x2A] = Some(Scancode::n(0x0E)); // Backspace
    t[0x2B] = Some(Scancode::n(0x0F)); // Tab
    t[0x2C] = Some(Scancode::n(0x39)); // Space
    t[0x2D] = Some(Scancode::n(0x0C)); // - _
    t[0x2E] = Some(Scancode::n(0x0D)); // = +
    t[0x2F] = Some(Scancode::n(0x1A)); // [ {
    t[0x30] = Some(Scancode::n(0x1B)); // ] }
    t[0x31] = Some(Scancode::n(0x2B)); // \ |
    t[0x32] = Some(Scancode::n(0x2B)); // ISO # ~ (same physical position as \ on ANSI)
    t[0x33] = Some(Scancode::n(0x27)); // ; :
    t[0x34] = Some(Scancode::n(0x28)); // ' "
    t[0x35] = Some(Scancode::n(0x29)); // ` ~
    t[0x36] = Some(Scancode::n(0x33)); // , <
    t[0x37] = Some(Scancode::n(0x34)); // . >
    t[0x38] = Some(Scancode::n(0x35)); // / ?
    t[0x39] = Some(Scancode::n(0x3A)); // Caps Lock
    // F1..F10 -> 0x3B..=0x44, F11 0x57, F12 0x58.
    let mut f: u16 = 0;
    while f < 10 {
        t[0x3A + f as usize] = Some(Scancode::n(0x3B + f));
        f += 1;
    }
    t[0x44] = Some(Scancode::n(0x57)); // F11
    t[0x45] = Some(Scancode::n(0x58)); // F12
    t[0x46] = Some(Scancode::e(0x37)); // Print Screen
    t[0x47] = Some(Scancode::n(0x46)); // Scroll Lock
    t[0x48] = Some(Scancode::PAUSE); // Pause
    t[0x49] = Some(Scancode::e(0x52)); // Insert
    t[0x4A] = Some(Scancode::e(0x47)); // Home
    t[0x4B] = Some(Scancode::e(0x49)); // Page Up
    t[0x4C] = Some(Scancode::e(0x53)); // Delete
    t[0x4D] = Some(Scancode::e(0x4F)); // End
    t[0x4E] = Some(Scancode::e(0x51)); // Page Down
    t[0x4F] = Some(Scancode::e(0x4D)); // Right
    t[0x50] = Some(Scancode::e(0x4B)); // Left
    t[0x51] = Some(Scancode::e(0x50)); // Down
    t[0x52] = Some(Scancode::e(0x48)); // Up
    t[0x53] = Some(Scancode::NUM_LOCK); // Num Lock
    t[0x54] = Some(Scancode::e(0x35)); // KP /
    t[0x55] = Some(Scancode::n(0x37)); // KP *
    t[0x56] = Some(Scancode::n(0x4A)); // KP -
    t[0x57] = Some(Scancode::n(0x4E)); // KP +
    t[0x58] = Some(Scancode::e(0x1C)); // KP Enter
    t[0x59] = Some(Scancode::n(0x4F)); // KP 1
    t[0x5A] = Some(Scancode::n(0x50)); // KP 2
    t[0x5B] = Some(Scancode::n(0x51)); // KP 3
    t[0x5C] = Some(Scancode::n(0x4B)); // KP 4
    t[0x5D] = Some(Scancode::n(0x4C)); // KP 5
    t[0x5E] = Some(Scancode::n(0x4D)); // KP 6
    t[0x5F] = Some(Scancode::n(0x47)); // KP 7
    t[0x60] = Some(Scancode::n(0x48)); // KP 8
    t[0x61] = Some(Scancode::n(0x49)); // KP 9
    t[0x62] = Some(Scancode::n(0x52)); // KP 0
    t[0x63] = Some(Scancode::n(0x53)); // KP .
    t[0x64] = Some(Scancode::n(0x56)); // ISO \ | (105th key)
    t[0x65] = Some(Scancode::e(0x5D)); // Application / Menu
    t[0x66] = Some(Scancode::e(0x5E)); // Power
    t[0x67] = Some(Scancode::n(0x59)); // KP =
    // F13..F24.
    let f13: [u16; 12] = [
        0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x76,
    ];
    let mut k = 0;
    while k < 12 {
        t[0x68 + k] = Some(Scancode::n(f13[k]));
        k += 1;
    }
    t[0x7F] = Some(Scancode::e(0x20)); // Mute (keyboard page alias)
    t[0x80] = Some(Scancode::e(0x30)); // Volume Up
    t[0x81] = Some(Scancode::e(0x2E)); // Volume Down
    t[0x85] = Some(Scancode::n(0x7E)); // KP , (Brazilian)
    t[0x87] = Some(Scancode::n(0x73)); // International1: JIS Ro / ABNT /?
    t[0x88] = Some(Scancode::n(0x70)); // International2: Katakana/Hiragana
    t[0x89] = Some(Scancode::n(0x7D)); // International3: Yen
    t[0x8A] = Some(Scancode::n(0x79)); // International4: Henkan
    t[0x8B] = Some(Scancode::n(0x7B)); // International5: Muhenkan
    t[0x90] = Some(Scancode::n(0x72)); // LANG1: Hangul/English
    t[0x91] = Some(Scancode::n(0x71)); // LANG2: Hanja
    t[0xE0] = Some(Scancode::n(0x1D)); // Left Ctrl
    t[0xE1] = Some(Scancode::n(0x2A)); // Left Shift
    t[0xE2] = Some(Scancode::n(0x38)); // Left Alt
    t[0xE3] = Some(Scancode::e(0x5B)); // Left GUI (Windows)
    t[0xE4] = Some(Scancode::e(0x1D)); // Right Ctrl
    t[0xE5] = Some(Scancode::n(0x36)); // Right Shift
    t[0xE6] = Some(Scancode::e(0x38)); // Right Alt (AltGr)
    t[0xE7] = Some(Scancode::e(0x5C)); // Right GUI
    t
};

/// Consumer page (0x0C) media keys that have a Windows scancode.
const CONSUMER: [(u16, Scancode); 18] = [
    (0x00B5, Scancode::e(0x19)), // Scan Next Track
    (0x00B6, Scancode::e(0x10)), // Scan Previous Track
    (0x00B7, Scancode::e(0x24)), // Stop
    (0x00CD, Scancode::e(0x22)), // Play/Pause
    (0x00E2, Scancode::e(0x20)), // Mute
    (0x00E9, Scancode::e(0x30)), // Volume Increment
    (0x00EA, Scancode::e(0x2E)), // Volume Decrement
    (0x0183, Scancode::e(0x6D)), // AL Consumer Control Configuration (Media Select)
    (0x018A, Scancode::e(0x6C)), // AL Email Reader
    (0x0192, Scancode::e(0x21)), // AL Calculator
    (0x0194, Scancode::e(0x6B)), // AL Local Machine Browser (My Computer)
    (0x0221, Scancode::e(0x65)), // AC Search
    (0x0223, Scancode::e(0x32)), // AC Home
    (0x0224, Scancode::e(0x6A)), // AC Back
    (0x0225, Scancode::e(0x69)), // AC Forward
    (0x0226, Scancode::e(0x68)), // AC Stop
    (0x0227, Scancode::e(0x67)), // AC Refresh
    (0x022A, Scancode::e(0x66)), // AC Bookmarks (Favorites)
];

/// Maps a HID usage (`page << 16 | id`, bare id = keyboard page) to a scancode.
#[must_use]
pub fn hid_to_scancode(usage: u32) -> Option<Scancode> {
    let (page, id) = match usage >> 16 {
        0 => (PAGE_KEYBOARD, usage & 0xFFFF),
        p => (p, usage & 0xFFFF),
    };
    match page {
        PAGE_KEYBOARD => KEYBOARD.get(usize::try_from(id).ok()?).copied().flatten(),
        PAGE_CONSUMER => CONSUMER
            .iter()
            .find(|(u, _)| u32::from(*u) == id)
            .map(|(_, s)| *s),
        _ => None,
    }
}

/// Full usage value for a keyboard-page id.
#[must_use]
pub const fn keyboard(id: u16) -> u32 {
    (PAGE_KEYBOARD << 16) | id as u32
}

/// Full usage value for a consumer-page id.
#[must_use]
pub const fn consumer(id: u16) -> u32 {
    (PAGE_CONSUMER << 16) | id as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn letters_digits_and_common_keys() {
        assert_eq!(hid_to_scancode(0x04), Some(Scancode::n(0x1E))); // A
        assert_eq!(hid_to_scancode(keyboard(0x1D)), Some(Scancode::n(0x2C))); // Z
        assert_eq!(hid_to_scancode(0x1E), Some(Scancode::n(0x02))); // 1
        assert_eq!(hid_to_scancode(0x27), Some(Scancode::n(0x0B))); // 0
        assert_eq!(hid_to_scancode(0x28), Some(Scancode::n(0x1C))); // Enter
        assert_eq!(hid_to_scancode(0x3A), Some(Scancode::n(0x3B))); // F1
        assert_eq!(hid_to_scancode(0x43), Some(Scancode::n(0x44))); // F10
        assert_eq!(hid_to_scancode(0x45), Some(Scancode::n(0x58))); // F12
        assert_eq!(hid_to_scancode(0x73), Some(Scancode::n(0x76))); // F24
    }

    #[test]
    fn extended_keys_have_e0() {
        for id in [
            0x46, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52, 0x54, 0x58, 0x65,
            0xE3, 0xE4, 0xE6, 0xE7,
        ] {
            let s = hid_to_scancode(id).expect("mapped");
            assert!(s.extended, "usage {id:#x} must be extended");
        }
        // Numpad digits share codes with navigation keys but are NOT extended.
        assert_eq!(hid_to_scancode(0x5F), Some(Scancode::n(0x47))); // KP7 vs Home(E0 47)
        assert_eq!(hid_to_scancode(0x4A), Some(Scancode::e(0x47)));
        assert_eq!(hid_to_scancode(0x28).map(|s| s.extended), Some(false)); // Enter
        assert_eq!(hid_to_scancode(0x58).map(|s| s.extended), Some(true)); // KP Enter
    }

    #[test]
    fn pause_and_numlock_use_virtual_keys() {
        assert_eq!(hid_to_scancode(0x48), Some(Scancode::PAUSE));
        assert_eq!(hid_to_scancode(0x48).map(|s| s.vk), Some(0x13));
        assert_eq!(hid_to_scancode(0x53).map(|s| s.vk), Some(0x90));
        assert_eq!(hid_to_scancode(0x04).map(|s| s.vk), Some(0));
    }

    #[test]
    fn iso_and_jis_keys() {
        assert_eq!(hid_to_scancode(0x64), Some(Scancode::n(0x56))); // ISO 105th key
        assert_eq!(hid_to_scancode(0x87), Some(Scancode::n(0x73))); // Ro
        assert_eq!(hid_to_scancode(0x89), Some(Scancode::n(0x7D))); // Yen
        assert_eq!(hid_to_scancode(0x8A), Some(Scancode::n(0x79))); // Henkan
    }

    #[test]
    fn media_keys() {
        assert_eq!(hid_to_scancode(consumer(0xCD)), Some(Scancode::e(0x22)));
        assert_eq!(hid_to_scancode(consumer(0xE9)), Some(Scancode::e(0x30)));
        assert_eq!(hid_to_scancode(consumer(0xB5)), Some(Scancode::e(0x19)));
        assert_eq!(hid_to_scancode(consumer(0x0001)), None);
        assert_eq!(hid_to_scancode((0x01 << 16) | 0x30), None); // generic desktop page
    }

    #[test]
    fn full_104_key_layout_is_covered_and_unique() {
        // Every key on a 104-key ANSI board (minus HID duplicates) maps, and no two physical
        // keys share a scancode.
        let mut ids: Vec<u32> = (0x04..=0x31)
            .chain(0x33..=0x63)
            .chain([0x65])
            .chain(0xE0..=0xE7)
            .collect();
        ids.sort_unstable();
        assert_eq!(ids.len(), 104);
        let mut seen: HashMap<(u16, bool, u16), u32> = HashMap::new();
        for id in ids {
            let s = hid_to_scancode(id).unwrap_or_else(|| panic!("usage {id:#x} unmapped"));
            if let Some(prev) = seen.insert((s.code, s.extended, s.vk), id) {
                panic!("usages {prev:#x} and {id:#x} share scancode {:#x}", s.code);
            }
        }
        // The 105th ISO key adds one more unique code.
        assert!(!seen.contains_key(&(0x56, false, 0)));
    }

    #[test]
    fn out_of_range_is_none() {
        assert_eq!(hid_to_scancode(0x00), None);
        assert_eq!(hid_to_scancode(0xE8), None);
        assert_eq!(hid_to_scancode(0xFFFF), None);
    }
}
