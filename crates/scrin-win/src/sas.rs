//! Ctrl+Alt+Del from a remote controller.
//!
//! Windows ignores an *injected* Ctrl+Alt+Del: only the secure attention
//! sequence (`SendSAS`, from the scrin service) opens the security screen.
//! [`ChordDetector`] spots the chord in the HID key stream so the host can
//! ask the service instead (`win::sas_client`), and swallow the Delete press.

/// HID usages (keyboard page 0x07).
const LEFT_CTRL: u32 = 0xE0;
const LEFT_ALT: u32 = 0xE2;
const RIGHT_CTRL: u32 = 0xE4;
const RIGHT_ALT: u32 = 0xE6;
const DELETE: u32 = 0x4C;
const KEYPAD_DOT: u32 = 0x63;

/// Tracks held modifiers across key events.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChordDetector {
    ctrl: u8,
    alt: u8,
}

impl ChordDetector {
    /// Feeds one key event; `true` means this is the Delete press of a
    /// Ctrl+Alt+Del chord (send the SAS, do not inject this key).
    pub fn on_key(&mut self, hid_usage: u32, down: bool) -> bool {
        let bit = |left: u32| -> u8 { if hid_usage == left { 1 } else { 2 } };
        match hid_usage {
            LEFT_CTRL | RIGHT_CTRL => {
                set(&mut self.ctrl, bit(LEFT_CTRL), down);
                false
            }
            LEFT_ALT | RIGHT_ALT => {
                set(&mut self.alt, bit(LEFT_ALT), down);
                false
            }
            DELETE | KEYPAD_DOT => down && self.ctrl != 0 && self.alt != 0,
            _ => false,
        }
    }
}

fn set(mask: &mut u8, bit: u8, down: bool) {
    if down {
        *mask |= bit;
    } else {
        *mask &= !bit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_chord_in_any_modifier_combination() {
        let mut d = ChordDetector::default();
        assert!(!d.on_key(LEFT_CTRL, true));
        assert!(!d.on_key(RIGHT_ALT, true));
        assert!(d.on_key(DELETE, true));
        assert!(!d.on_key(DELETE, false), "release is not a second chord");
        assert!(d.on_key(KEYPAD_DOT, true));
    }

    #[test]
    fn needs_both_modifiers_held() {
        let mut d = ChordDetector::default();
        assert!(!d.on_key(DELETE, true));
        d.on_key(LEFT_CTRL, true);
        assert!(!d.on_key(DELETE, true));
        d.on_key(LEFT_ALT, true);
        d.on_key(LEFT_ALT, false);
        assert!(!d.on_key(DELETE, true));
    }

    #[test]
    fn left_and_right_modifiers_are_tracked_separately() {
        let mut d = ChordDetector::default();
        d.on_key(LEFT_CTRL, true);
        d.on_key(RIGHT_CTRL, true);
        d.on_key(LEFT_CTRL, false);
        d.on_key(LEFT_ALT, true);
        assert!(d.on_key(DELETE, true), "right ctrl still held");
    }
}
