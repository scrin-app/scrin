//! Session kinds and the anti-scam policy.

use std::fmt;

use crate::permissions::{Permission, Permissions};

/// Controller device identity (Ed25519 public key bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId(pub [u8; 32]);

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PeerId(")?;
        for b in &self.0[..4] {
            write!(f, "{b:02x}")?;
        }
        write!(f, "…)")
    }
}

/// How the controller got here; decides which [`Policy`] caps apply.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SessionKind {
    /// One-time code, controller identity not verified.
    Anonymous,
    /// Controller signed in with a verified account.
    Verified {
        /// Verified organisation, if any.
        org: Option<String>,
        /// Verified display name.
        name: String,
    },
    /// Controller key is on the host trust list (unattended access).
    Trusted,
    /// Managed device; access granted by a signed organisation policy.
    OrgPolicy,
}

impl SessionKind {
    /// Whether the session can run without anyone at the host.
    pub const fn is_unattended(&self) -> bool {
        matches!(self, Self::Trusted | Self::OrgPolicy)
    }
}

/// Anti-scam and session-limit policy for the host. All durations are milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// Hard cap on an anonymous session.
    pub anonymous_max_duration_ms: u64,
    /// Cap on any other session; `None` = unlimited.
    pub max_duration_ms: Option<u64>,
    /// Permissions an anonymous controller can never hold.
    pub anonymous_forbidden: Permissions,
    /// Ceiling for [`SessionKind::OrgPolicy`] sessions, set by the organisation.
    pub org_allowed: Permissions,
    /// Delay before Accept becomes clickable on an anonymous request.
    pub anonymous_accept_delay_ms: u64,
    /// Unanswered requests are rejected after this long.
    pub request_timeout_ms: u64,
    /// Whether an anonymous controller may be added to the trust list mid-session.
    pub anonymous_can_add_trust: bool,
    /// Trusted / org-policy requests are accepted without a dialog.
    pub unattended_auto_accept: bool,
}

/// One hour.
pub const ANONYMOUS_MAX_DURATION_MS: u64 = 60 * 60 * 1000;

impl Default for Policy {
    fn default() -> Self {
        Self {
            anonymous_max_duration_ms: ANONYMOUS_MAX_DURATION_MS,
            max_duration_ms: None,
            anonymous_forbidden: Permission::FilesIn
                | Permission::FilesOut
                | Permission::PrivacyMode
                | Permission::Tunnel
                | Permission::Terminal
                | Permission::BlockInput,
            org_allowed: Permissions::full(),
            anonymous_accept_delay_ms: 5_000,
            request_timeout_ms: 60_000,
            anonymous_can_add_trust: false,
            unattended_auto_accept: true,
        }
    }
}

impl Policy {
    /// The ceiling of permissions any session of `kind` may hold.
    pub fn allowed(&self, kind: &SessionKind) -> Permissions {
        let mut allowed = match kind {
            SessionKind::Anonymous => Permissions::full() - self.anonymous_forbidden,
            SessionKind::Verified { .. } | SessionKind::Trusted => Permissions::full(),
            SessionKind::OrgPolicy => self.org_allowed,
        };
        if !kind.is_unattended() {
            allowed = allowed.without(Permission::PrivacyMode);
        }
        allowed
    }

    /// Whether `perm` may be granted to a session of `kind`.
    pub fn permits(&self, kind: &SessionKind, perm: Permission) -> bool {
        self.allowed(kind).contains(perm)
    }

    /// Minimum time between showing the request dialog and Accept being enabled.
    pub const fn accept_delay_ms(&self, kind: &SessionKind) -> u64 {
        match kind {
            SessionKind::Anonymous => self.anonymous_accept_delay_ms,
            _ => 0,
        }
    }

    /// Maximum session duration for `kind`, if any.
    pub fn max_duration_ms(&self, kind: &SessionKind) -> Option<u64> {
        let general = self.max_duration_ms;
        match kind {
            SessionKind::Anonymous => Some(general.map_or(self.anonymous_max_duration_ms, |g| {
                g.min(self.anonymous_max_duration_ms)
            })),
            _ => general,
        }
    }

    /// Whether the controller may be added to the trust list during this session.
    pub const fn can_add_trust(&self, kind: &SessionKind) -> bool {
        match kind {
            SessionKind::Anonymous => self.anonymous_can_add_trust,
            _ => true,
        }
    }

    /// Whether a request of `kind` is accepted without a dialog.
    pub const fn auto_accepts(&self, kind: &SessionKind) -> bool {
        self.unattended_auto_accept && kind.is_unattended()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified() -> SessionKind {
        SessionKind::Verified {
            org: Some("Acme".into()),
            name: "Ana".into(),
        }
    }

    #[test]
    fn anonymous_ceiling_excludes_files_terminal_tunnel_privacy_block_input() {
        let p = Policy::default();
        let a = p.allowed(&SessionKind::Anonymous);
        for perm in [
            Permission::FilesIn,
            Permission::FilesOut,
            Permission::PrivacyMode,
            Permission::Tunnel,
            Permission::Terminal,
            Permission::BlockInput,
        ] {
            assert!(!a.contains(perm), "{perm:?}");
        }
        assert!(a.contains_all(Permissions::support()));
    }

    #[test]
    fn privacy_mode_only_for_trusted_and_org_policy() {
        let p = Policy::default();
        assert!(!p.permits(&verified(), Permission::PrivacyMode));
        assert!(!p.permits(&SessionKind::Anonymous, Permission::PrivacyMode));
        assert!(p.permits(&SessionKind::Trusted, Permission::PrivacyMode));
        assert!(p.permits(&SessionKind::OrgPolicy, Permission::PrivacyMode));
    }

    #[test]
    fn privacy_mode_stays_forbidden_for_anonymous_even_if_forbidden_list_is_emptied() {
        let p = Policy {
            anonymous_forbidden: Permissions::empty(),
            ..Policy::default()
        };
        assert!(!p.permits(&SessionKind::Anonymous, Permission::PrivacyMode));
        assert!(p.permits(&SessionKind::Anonymous, Permission::FilesIn));
    }

    #[test]
    fn org_policy_ceiling_is_configurable() {
        let p = Policy {
            org_allowed: Permissions::view_only(),
            ..Policy::default()
        };
        assert_eq!(p.allowed(&SessionKind::OrgPolicy), Permissions::view_only());
    }

    #[test]
    fn anonymous_duration_capped_at_one_hour_and_by_general_cap() {
        let p = Policy::default();
        assert_eq!(p.max_duration_ms(&SessionKind::Anonymous), Some(3_600_000));
        assert_eq!(p.max_duration_ms(&SessionKind::Trusted), None);
        let p = Policy {
            max_duration_ms: Some(1_000),
            ..Policy::default()
        };
        assert_eq!(p.max_duration_ms(&SessionKind::Anonymous), Some(1_000));
        assert_eq!(p.max_duration_ms(&verified()), Some(1_000));
    }

    #[test]
    fn accept_delay_and_trust_rules_depend_on_kind() {
        let p = Policy::default();
        assert_eq!(p.accept_delay_ms(&SessionKind::Anonymous), 5_000);
        assert_eq!(p.accept_delay_ms(&verified()), 0);
        assert!(!p.can_add_trust(&SessionKind::Anonymous));
        assert!(p.can_add_trust(&verified()));
        assert!(p.auto_accepts(&SessionKind::Trusted));
        assert!(!p.auto_accepts(&verified()));
    }

    #[test]
    fn peer_id_debug_is_short() {
        assert_eq!(format!("{:?}", PeerId([0xab; 32])), "PeerId(abababab…)");
    }
}
