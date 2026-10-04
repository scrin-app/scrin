//! Short authentication string: 5 emoji (30 bits) derived from the pairing key.
//!
//! The list is 64 visually distinct emoji with plain names so users can read
//! them aloud in any language; names are translated in the UI by index.

/// (emoji, English name). Order is part of the protocol: never reorder.
pub const EMOJI: [(&str, &str); 64] = [
    ("🐶", "dog"),
    ("🐱", "cat"),
    ("🦁", "lion"),
    ("🐴", "horse"),
    ("🦄", "unicorn"),
    ("🐷", "pig"),
    ("🐘", "elephant"),
    ("🐰", "rabbit"),
    ("🐼", "panda"),
    ("🐓", "rooster"),
    ("🐧", "penguin"),
    ("🐢", "turtle"),
    ("🐟", "fish"),
    ("🐙", "octopus"),
    ("🦋", "butterfly"),
    ("🌷", "tulip"),
    ("🌳", "tree"),
    ("🌵", "cactus"),
    ("🍄", "mushroom"),
    ("🌏", "globe"),
    ("🌙", "moon"),
    ("☁️", "cloud"),
    ("🔥", "fire"),
    ("🍌", "banana"),
    ("🍎", "apple"),
    ("🍓", "strawberry"),
    ("🌽", "corn"),
    ("🍕", "pizza"),
    ("🎂", "cake"),
    ("❤️", "heart"),
    ("😀", "smiley"),
    ("🤖", "robot"),
    ("🎩", "hat"),
    ("👓", "glasses"),
    ("🔧", "wrench"),
    ("🎅", "santa"),
    ("👍", "thumbs up"),
    ("☂️", "umbrella"),
    ("⌛", "hourglass"),
    ("⏰", "clock"),
    ("🎁", "gift"),
    ("💡", "light bulb"),
    ("📕", "book"),
    ("✏️", "pencil"),
    ("📎", "paperclip"),
    ("✂️", "scissors"),
    ("🔒", "lock"),
    ("🔑", "key"),
    ("🔨", "hammer"),
    ("☎️", "telephone"),
    ("🏁", "flag"),
    ("🚂", "train"),
    ("🚲", "bicycle"),
    ("✈️", "aeroplane"),
    ("🚀", "rocket"),
    ("🏆", "trophy"),
    ("⚽", "ball"),
    ("🎸", "guitar"),
    ("🎺", "trumpet"),
    ("🔔", "bell"),
    ("⚓", "anchor"),
    ("🎧", "headphones"),
    ("📁", "folder"),
    ("📌", "pin"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sas(pub [u8; 5]);

impl Sas {
    #[must_use]
    pub fn derive(key: &[u8; 32]) -> Self {
        let d = blake3::derive_key("scrin sas v1", key);
        let bits = u64::from_le_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]);
        let mut out = [0u8; 5];
        for (i, slot) in out.iter_mut().enumerate() {
            // Truncation is intended: each index is 6 bits (< 64).
            #[allow(clippy::cast_possible_truncation)]
            {
                *slot = ((bits >> (i * 6)) & 0x3f) as u8;
            }
        }
        Self(out)
    }

    #[must_use]
    pub fn emoji(&self) -> String {
        self.0
            .iter()
            .map(|&i| EMOJI[usize::from(i)].0)
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|&i| EMOJI[usize::from(i)].1).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let a = Sas::derive(&[3; 32]);
        assert_eq!(a, Sas::derive(&[3; 32]));
        assert!(a.0.iter().all(|&i| i < 64));
        assert_ne!(a, Sas::derive(&[4; 32]));
        assert_eq!(a.names().len(), 5);
    }

    #[test]
    fn emoji_names_are_unique() {
        let mut names: Vec<_> = EMOJI.iter().map(|e| e.1).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 64);
    }
}
