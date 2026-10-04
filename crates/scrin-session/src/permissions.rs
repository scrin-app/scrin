//! Session permissions as a small bit set.

use std::fmt;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not, Sub, SubAssign};

/// One capability a host can grant to a controller during a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Permission {
    /// See the host screen.
    View = 0,
    /// Send keyboard and mouse input.
    Input = 1,
    /// Synchronise the clipboard.
    Clipboard = 2,
    /// Copy files from the controller to the host.
    FilesIn = 3,
    /// Copy files from the host to the controller.
    FilesOut = 4,
    /// Hear the host audio.
    Audio = 5,
    /// Play the controller microphone on the host.
    Microphone = 6,
    /// Restart the host and reconnect.
    Restart = 7,
    /// Open a remote terminal.
    Terminal = 8,
    /// Record the session.
    Record = 9,
    /// Blank the host screen while controlled.
    PrivacyMode = 10,
    /// Block local input on the host.
    BlockInput = 11,
    /// Forward TCP ports through the session.
    Tunnel = 12,
    /// Text chat.
    Chat = 13,
    /// Draw on a shared whiteboard overlay.
    Whiteboard = 14,
}

impl Permission {
    /// Every permission, in bit order.
    pub const ALL: [Self; 15] = [
        Self::View,
        Self::Input,
        Self::Clipboard,
        Self::FilesIn,
        Self::FilesOut,
        Self::Audio,
        Self::Microphone,
        Self::Restart,
        Self::Terminal,
        Self::Record,
        Self::PrivacyMode,
        Self::BlockInput,
        Self::Tunnel,
        Self::Chat,
        Self::Whiteboard,
    ];

    /// The single bit representing this permission.
    pub const fn bit(self) -> u32 {
        1 << (self as u32)
    }

    /// Stable lowercase name, for logs and UI keys.
    pub const fn name(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Input => "input",
            Self::Clipboard => "clipboard",
            Self::FilesIn => "files_in",
            Self::FilesOut => "files_out",
            Self::Audio => "audio",
            Self::Microphone => "microphone",
            Self::Restart => "restart",
            Self::Terminal => "terminal",
            Self::Record => "record",
            Self::PrivacyMode => "privacy_mode",
            Self::BlockInput => "block_input",
            Self::Tunnel => "tunnel",
            Self::Chat => "chat",
            Self::Whiteboard => "whiteboard",
        }
    }
}

const ALL_MASK: u32 = (1 << Permission::ALL.len()) - 1;

/// A set of [`Permission`]s backed by a `u32`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Permissions(u32);

impl Permissions {
    /// No permissions.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Every permission.
    pub const fn full() -> Self {
        Self(ALL_MASK)
    }

    /// Only [`Permission::View`].
    pub const fn view_only() -> Self {
        Self(Permission::View.bit())
    }

    /// Typical IT-support set: view, input, clipboard, audio, chat, whiteboard.
    pub const fn support() -> Self {
        Self(
            Permission::View.bit()
                | Permission::Input.bit()
                | Permission::Clipboard.bit()
                | Permission::Audio.bit()
                | Permission::Chat.bit()
                | Permission::Whiteboard.bit(),
        )
    }

    /// Build a set from raw bits, dropping unknown bits.
    pub const fn from_bits_truncate(bits: u32) -> Self {
        Self(bits & ALL_MASK)
    }

    /// Raw bits.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Set holding exactly `perm`.
    pub const fn only(perm: Permission) -> Self {
        Self(perm.bit())
    }

    /// Whether `perm` is in the set.
    pub const fn contains(self, perm: Permission) -> bool {
        self.0 & perm.bit() != 0
    }

    /// Whether every permission of `other` is in the set.
    pub const fn contains_all(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Number of permissions in the set.
    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    /// The set with `perm` added.
    #[must_use]
    pub const fn with(self, perm: Permission) -> Self {
        Self(self.0 | perm.bit())
    }

    /// The set with `perm` removed.
    #[must_use]
    pub const fn without(self, perm: Permission) -> Self {
        Self(self.0 & !perm.bit())
    }

    /// Union of both sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Intersection of both sets.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Permissions in `self` but not in `other`.
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Iterate the permissions in bit order.
    pub fn iter(self) -> impl Iterator<Item = Permission> {
        Permission::ALL
            .into_iter()
            .filter(move |p| self.contains(*p))
    }
}

impl fmt::Debug for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set()
            .entries(self.iter().map(Permission::name))
            .finish()
    }
}

impl From<Permission> for Permissions {
    fn from(perm: Permission) -> Self {
        Self::only(perm)
    }
}

impl FromIterator<Permission> for Permissions {
    fn from_iter<I: IntoIterator<Item = Permission>>(iter: I) -> Self {
        iter.into_iter().fold(Self::empty(), Self::with)
    }
}

impl BitOr for Permissions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitOr<Permission> for Permissions {
    type Output = Self;
    fn bitor(self, rhs: Permission) -> Self {
        self.with(rhs)
    }
}

impl BitOr for Permission {
    type Output = Permissions;
    fn bitor(self, rhs: Self) -> Permissions {
        Permissions::only(self).with(rhs)
    }
}

impl BitOrAssign for Permissions {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

impl BitAnd for Permissions {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersection(rhs)
    }
}

impl BitAndAssign for Permissions {
    fn bitand_assign(&mut self, rhs: Self) {
        *self = self.intersection(rhs);
    }
}

impl Sub for Permissions {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.difference(rhs)
    }
}

impl SubAssign for Permissions {
    fn sub_assign(&mut self, rhs: Self) {
        *self = self.difference(rhs);
    }
}

impl Not for Permissions {
    type Output = Self;
    fn not(self) -> Self {
        Self::full().difference(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_contains_every_permission_and_nothing_else() {
        let full = Permissions::full();
        assert_eq!(full.len(), 15);
        assert!(Permission::ALL.iter().all(|p| full.contains(*p)));
        assert_eq!(Permissions::from_bits_truncate(u32::MAX), full);
    }

    #[test]
    fn presets_are_nested() {
        assert!(Permissions::support().contains_all(Permissions::view_only()));
        assert!(Permissions::full().contains_all(Permissions::support()));
        assert!(!Permissions::support().contains(Permission::FilesIn));
    }

    #[test]
    fn set_operations_behave_like_sets() {
        let a = Permission::View | Permission::Input;
        let b = Permission::Input | Permission::Chat;
        assert_eq!(a & b, Permissions::only(Permission::Input));
        assert_eq!(
            a | b,
            Permission::View | Permission::Input | Permission::Chat
        );
        assert_eq!(a - b, Permissions::only(Permission::View));
        assert_eq!(!Permissions::full(), Permissions::empty());
        assert!(
            a.without(Permission::View)
                .without(Permission::Input)
                .is_empty()
        );
    }

    #[test]
    fn iter_and_collect_round_trip() {
        let s = Permissions::support();
        let back: Permissions = s.iter().collect();
        assert_eq!(back, s);
        assert_eq!(format!("{:?}", Permissions::view_only()), "{\"view\"}");
    }
}
