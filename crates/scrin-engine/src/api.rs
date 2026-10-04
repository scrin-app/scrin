//! Commands the UI sends and events the engine emits.
//!
//! Everything here is plain data with `serde`, so the desktop shell can pass
//! it through Tauri unchanged. Permissions travel as their stable lowercase
//! names (`Permission::name`, e.g. `"view"`, `"files_in"`).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::backend::InputEvent;
use scrin_session::{Permission, Permissions};

/// Engine-assigned session handle: `h<n>` (we are the host) or `c<n>` (we control).
pub type SessionId = String;

/// What the UI can ask the engine to do.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Command {
    GetStatus,
    RegenerateCode,
    /// Dial `target` (scrin ID, `scrin:` ticket or 64-hex endpoint id).
    /// An empty `code` asks for unattended (trusted) access.
    Connect {
        target: String,
        code: String,
        /// Permission names to request; `None` = the support set.
        requested: Option<Vec<String>>,
    },
    /// Controller: the user compared the emoji. `false` ends the session.
    ConfirmSas {
        session: SessionId,
        matches: bool,
    },
    /// Host: accept an incoming request with these permission names.
    Accept {
        session: SessionId,
        permissions: Vec<String>,
    },
    Reject {
        session: SessionId,
    },
    /// Host: take a permission away live.
    Revoke {
        session: SessionId,
        permission: String,
    },
    /// Host: add a permission live (within policy).
    Grant {
        session: SessionId,
        permission: String,
    },
    /// Host: remember this controller for unattended access.
    TrustPeer {
        session: SessionId,
        label: String,
    },
    EndSession {
        session: SessionId,
    },
    /// Controller: send one input event (needs the `input` permission).
    SendInput {
        session: SessionId,
        event: InputEvent,
    },
    SetQuality {
        session: SessionId,
        quality: Quality,
    },
    ListTrusted,
    /// `device` is the 64-hex device id.
    RemoveTrusted {
        device: String,
    },
}

/// Answer to a [`Command`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Reply {
    Ok,
    Status(Status),
    Session(SessionId),
    Trusted(Vec<TrustedInfo>),
}

/// Stream quality preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Quality {
    /// Let BWE decide; keep resolution, drop fps first.
    Auto,
    /// Cap at ~8 Mbps.
    Balanced,
    /// Keep resolution sharp (IT support).
    Sharp,
    /// Keep frame rate (gaming); drop resolution first.
    Speed,
}

impl Quality {
    /// Upper bitrate the receiver asks for, if any.
    #[must_use]
    pub const fn cap_bps(self) -> Option<u32> {
        match self {
            Self::Balanced => Some(8_000_000),
            Self::Auto | Self::Sharp | Self::Speed => None,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// 64-hex Ed25519 device key (= iroh endpoint id).
    pub device_id: String,
    pub fingerprint: String,
    /// 9-digit scrin ID. Provisional (derived from the key) until the
    /// rendezvous server registers the device.
    pub scrin_id: String,
    /// Dialable without a server: `scrin:<id>?a=…&r=…`.
    pub ticket: String,
    /// One-time code in display form `ABCD-EFGH`. Never logged.
    pub code: String,
    /// Epoch ms.
    pub code_issued_at: u64,
    pub code_expires_at: u64,
    pub online: bool,
    pub backend: String,
}

impl std::fmt::Debug for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Status")
            .field("device_id", &self.device_id)
            .field("scrin_id", &self.scrin_id)
            .field("code", &"[redacted]")
            .field("code_expires_at", &self.code_expires_at)
            .field("online", &self.online)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedInfo {
    pub device: String,
    pub fingerprint: String,
    pub label: String,
    pub profile: String,
    pub added_at: u64,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Host,
    Controller,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    /// Controller: resolving and dialling.
    Connecting,
    /// Controller: SPAKE2 running.
    Pairing,
    /// Controller: waiting for the person at the host. Host: dialog showing.
    AwaitingAccept,
    Active,
    Ended,
}

/// Session statistics, once per second on the controller.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsInfo {
    pub rtt_ms: f64,
    pub fps: f64,
    pub bitrate_bps: u64,
    /// Fraction of frames lost after FEC, 0..=1.
    pub loss: f64,
    pub decode_ms: f64,
    pub width: u32,
    pub height: u32,
    pub frames_total: u64,
}

/// A decoded frame for the native renderer (BGRA8, top-down).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
    pub frame_id: u32,
}

/// Everything the engine tells the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum Event {
    Status(Status),
    /// Host: a paired controller asks for a session; show the interstitial.
    IncomingRequest {
        session: SessionId,
        peer: String,
        fingerprint: String,
        /// `anonymous` | `trusted`.
        kind: String,
        /// Indexes into the SAS emoji table; `None` for trusted (no code) sessions.
        sas: Option<[u8; 5]>,
        requested: Vec<String>,
        allowed: Vec<String>,
        /// Epoch ms when Accept becomes clickable (anti-scam delay).
        accept_enabled_at: u64,
        expires_at: u64,
    },
    /// Controller: compare these with the host's screen.
    Sas {
        session: SessionId,
        emoji: [u8; 5],
    },
    StateChanged {
        session: SessionId,
        role: Role,
        state: SessionState,
        peer: Option<String>,
        reason: Option<String>,
    },
    PermissionsChanged {
        session: SessionId,
        granted: Vec<String>,
    },
    /// Host: the controller asked for one more permission (policy allows it).
    PermissionRequested {
        session: SessionId,
        permission: String,
    },
    Stats {
        session: SessionId,
        stats: StatsInfo,
    },
    /// Native renderer only; never serialised to the webview.
    #[serde(skip)]
    VideoFrame {
        session: SessionId,
        frame: Arc<VideoFrame>,
    },
    Error {
        session: Option<SessionId>,
        /// Stable code: `offline`, `wrong-code`, `code-unavailable`,
        /// `rejected`, `busy`, `timeout`, `sas-mismatch`, `untrusted`,
        /// `accept-too-early`, `policy`, `media`, `trust-tampered`, `internal`.
        code: String,
        message: String,
    },
}

/// Permission names → set; unknown names are an error.
pub fn parse_permissions(names: &[String]) -> crate::Result<Permissions> {
    names.iter().try_fold(Permissions::empty(), |acc, n| {
        Ok(acc.with(parse_permission(n)?))
    })
}

pub fn parse_permission(name: &str) -> crate::Result<Permission> {
    Permission::ALL
        .into_iter()
        .find(|p| p.name() == name)
        .ok_or(crate::EngineError::Invalid("unknown permission"))
}

#[must_use]
pub fn permission_names(p: Permissions) -> Vec<String> {
    p.iter().map(|x| x.name().to_owned()).collect()
}
