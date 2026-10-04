//! Why requests are rejected and sessions end, plus the end-of-session record.

use crate::permissions::Permissions;
use crate::policy::{PeerId, SessionKind};

/// Why the host refused a connection request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RejectReason {
    /// The person at the host clicked Reject.
    UserRejected,
    /// Nobody answered the request in time.
    Timeout,
    /// The host is already handling another request or session.
    Busy,
    /// The host rejected and reported the controller.
    Reported,
}

/// Why a session (or a pending request) ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndReason {
    /// The host user stopped the session.
    HostStopped,
    /// The host user stopped the session and reported the controller.
    Reported,
    /// The controller ended the session.
    PeerEnded,
    /// The connection dropped.
    PeerDisconnected,
    /// The policy time limit was reached.
    TimeLimit,
    /// The request was rejected before the session started.
    Rejected(RejectReason),
}

/// Activity record produced when an accepted session ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLog {
    /// Controller device.
    pub peer: PeerId,
    /// Kind of session.
    pub kind: SessionKind,
    /// Millis when the session became active.
    pub started_at: u64,
    /// Millis when it ended.
    pub ended_at: u64,
    /// Why it ended.
    pub end_reason: EndReason,
    /// Every permission held at any point in the session.
    pub permissions_used: Permissions,
    /// Bytes received from the controller (placeholder counter fed by the shell).
    pub bytes_in: u64,
    /// Bytes sent to the controller (placeholder counter fed by the shell).
    pub bytes_out: u64,
}

impl SessionLog {
    /// Session length in milliseconds.
    pub const fn duration_ms(&self) -> u64 {
        self.ended_at.saturating_sub(self.started_at)
    }
}
