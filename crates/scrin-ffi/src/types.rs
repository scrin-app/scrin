//! Records, enums and the listener trait that cross the FFI boundary.

use scrin_session::Permission;

/// Errors surfaced to Kotlin as `ScrinException`.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ScrinError {
    #[error("invalid input: {msg}")]
    InvalidInput { msg: String },
    #[error("network: {msg}")]
    Network { msg: String },
    #[error("pairing failed: {msg}")]
    Pairing { msg: String },
    #[error("not allowed in the current state: {msg}")]
    State { msg: String },
    #[error("storage: {msg}")]
    Storage { msg: String },
}

impl ScrinError {
    pub(crate) fn input(msg: impl Into<String>) -> Self {
        Self::InvalidInput { msg: msg.into() }
    }
    pub(crate) fn state(msg: impl Into<String>) -> Self {
        Self::State { msg: msg.into() }
    }
    pub(crate) fn storage(msg: &impl ToString) -> Self {
        Self::Storage {
            msg: msg.to_string(),
        }
    }
}

impl From<scrin_net::NetError> for ScrinError {
    fn from(e: scrin_net::NetError) -> Self {
        use scrin_net::NetError as N;
        match e {
            N::PairingFailed
            | N::CodeConsumed
            | N::CodeExpired
            | N::Untrusted
            | N::BadSignature => Self::Pairing { msg: e.to_string() },
            other => Self::Network {
                msg: other.to_string(),
            },
        }
    }
}

impl From<scrin_crypto::Error> for ScrinError {
    fn from(e: scrin_crypto::Error) -> Self {
        Self::InvalidInput { msg: e.to_string() }
    }
}

/// Network configuration of the core.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct CoreConfig {
    /// Shown to the host in the pre-accept dialog (e.g. "Pixel 9 of Ana").
    pub device_name: String,
    /// Self-hosted relay URLs. Empty = n0 public relays (development builds only).
    pub relay_urls: Vec<String>,
    /// Bind 127.0.0.1 only, no relays, no address lookup (host tests).
    pub loopback_only: bool,
    /// Rendezvous server (`https://…`, or `http://` on a private network) for
    /// 9-digit scrin IDs: register + presence as host, signed resolve as controller.
    /// `None` = tickets only.
    pub server_url: Option<String>,
}

/// A fresh one-time code for the host screen.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CodeInfo {
    /// `ABCD-EFGH`.
    pub display: String,
    pub expires_in_s: u32,
}

/// Five dictated words (D24) for the host screen. Never log `words`.
#[derive(Clone, uniffi::Record)]
pub struct PassphraseInfo {
    /// Space-separated, in the requested language.
    pub words: String,
    /// Seconds until the server forgets the locator (words 1–2).
    pub expires_in_s: u32,
}

impl std::fmt::Debug for PassphraseInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PassphraseInfo")
            .field("words", &"<redacted>")
            .field("expires_in_s", &self.expires_in_s)
            .finish()
    }
}

/// What the host shares with a controller (besides the code).
#[derive(Debug, Clone, uniffi::Record)]
pub struct HostInfo {
    /// Connect ticket `scrin:<hex id>?a=<ip:port>…&r=<relay>` (the desktop
    /// engine's format): device id + current addresses.
    pub ticket: String,
    pub device_id: String,
    pub fingerprint: String,
    /// 9-digit scrin ID once the rendezvous server registered this device.
    pub scrin_id: Option<String>,
}

/// Parsed connect target.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TicketInfo {
    pub device_id: String,
    pub fingerprint: String,
    pub direct_addresses: u32,
    pub relay_url: Option<String>,
}

/// Short authentication string both people compare.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SasInfo {
    /// Five emoji.
    pub emoji: Vec<String>,
    /// Their English names, for screen readers and verbal comparison.
    pub names: Vec<String>,
}

/// Coarse session state for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SessionState {
    Listening,
    Connecting,
    Pairing,
    AwaitingAccept,
    IncomingRequest,
    Active,
    Ended,
}

/// Mirrors `scrin_session::Permission` (same order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SessionPermission {
    View,
    Input,
    Clipboard,
    FilesIn,
    FilesOut,
    Audio,
    Microphone,
    Restart,
    Terminal,
    Record,
    PrivacyMode,
    BlockInput,
    Tunnel,
    Chat,
    Whiteboard,
}

const FFI_ALL: [SessionPermission; 15] = [
    SessionPermission::View,
    SessionPermission::Input,
    SessionPermission::Clipboard,
    SessionPermission::FilesIn,
    SessionPermission::FilesOut,
    SessionPermission::Audio,
    SessionPermission::Microphone,
    SessionPermission::Restart,
    SessionPermission::Terminal,
    SessionPermission::Record,
    SessionPermission::PrivacyMode,
    SessionPermission::BlockInput,
    SessionPermission::Tunnel,
    SessionPermission::Chat,
    SessionPermission::Whiteboard,
];

impl SessionPermission {
    pub(crate) fn to_core(self) -> Permission {
        let i = FFI_ALL.iter().position(|p| *p == self).unwrap_or(0);
        Permission::ALL[i]
    }

    pub(crate) fn from_core(p: Permission) -> Self {
        let i = Permission::ALL.iter().position(|q| *q == p).unwrap_or(0);
        FFI_ALL[i]
    }
}

/// Trust level of an unattended controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrustProfile {
    ViewOnly,
    Support,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TrustedDevice {
    pub device_id: String,
    pub fingerprint: String,
    pub label: String,
    pub profile: TrustProfile,
    /// Unix seconds.
    pub added_at: u64,
    pub expires_at: Option<u64>,
}

/// The host-side pre-accept interstitial (ADR-0009).
#[derive(Debug, Clone, uniffi::Record)]
pub struct IncomingRequest {
    pub peer_id: String,
    pub peer_fingerprint: String,
    /// Controller has a verified account. Anonymous requests show the scam warning.
    pub verified: bool,
    pub unattended: bool,
    pub controller_name: String,
    pub requested: Vec<SessionPermission>,
    /// Only these may be offered in the dialog.
    pub allowed: Vec<SessionPermission>,
    /// Accept stays disabled this long (anti-scam delay).
    pub accept_in_ms: u64,
    /// Auto-reject after this long.
    pub expires_in_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EndKind {
    Cancelled,
    Rejected,
    HostStopped,
    Reported,
    PeerEnded,
    Disconnected,
    TimeLimit,
    ConnectFailed,
    PairingFailed,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EndInfo {
    pub kind: EndKind,
    pub detail: String,
    /// Session length if one actually ran.
    pub duration_ms: Option<u64>,
}

/// Informational events that need no state change.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Notice {
    PolicyDenied { permission: SessionPermission },
    TrustAdded { device_id: String },
    TrustDenied,
    Reported { device_id: String },
}

#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct SessionStats {
    pub rtt_ms: u32,
    pub bytes_in: u64,
    pub bytes_out: u64,
    /// Selected path is a direct UDP path (not relayed).
    pub direct: bool,
    pub frames: u64,
    pub frames_recovered: u64,
    pub frames_lost: u64,
    /// Video frames per second over the last interval (received on the
    /// controller, sent on the host).
    pub fps: f32,
    /// Media bitrate over the last interval (in on the controller, out on the host).
    pub bitrate_bps: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VideoConfigInfo {
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
    /// Out-of-band codec config (SPS/PPS for H.264); may be empty.
    pub codec_config: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MouseButtonKind {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

/// Remote input. Coordinates are normalised to `[0, 1]` of the host display.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum RemoteInput {
    Touch {
        pointer_id: u32,
        phase: TouchPhase,
        x: f32,
        y: f32,
    },
    /// USB HID usage (keyboard page 0x07; other pages in the high 16 bits).
    Key {
        hid_usage: u32,
        down: bool,
        modifiers: u32,
    },
    Text {
        text: String,
    },
    MouseMove {
        x: f32,
        y: f32,
    },
    MouseButton {
        button: MouseButtonKind,
        down: bool,
    },
    Wheel {
        dx: i32,
        dy: i32,
    },
}

/// Session events, implemented in Kotlin. Called from core worker threads.
#[uniffi::export(foreign)]
pub trait SessionListener: Send + Sync {
    fn on_state(&self, state: SessionState);
    fn on_sas(&self, sas: SasInfo);
    fn on_incoming_request(&self, request: IncomingRequest);
    fn on_permissions(&self, granted: Vec<SessionPermission>);
    /// Controller asks for one more permission; host UI asks the user.
    fn on_permission_asked(&self, permission: SessionPermission);
    fn on_notice(&self, notice: Notice);
    fn on_stats(&self, stats: SessionStats);
    /// Host: the rendezvous server registered this device under `scrin_id`
    /// (also after each presence refresh that changed it).
    fn on_registered(&self, scrin_id: String);
    fn on_video_config(&self, config: VideoConfigInfo);
    /// Controller: one complete access unit (H.264 Annex B), reassembled from
    /// FEC shards. `pts_us` is the local arrival time in µs since the stream
    /// started (monotonic; the wire carries no sender timestamp).
    fn on_video_frame(&self, data: Vec<u8>, keyframe: bool, frame_id: u32, pts_us: u64);
    /// Host: the controller lost a frame; send a sync frame.
    fn on_keyframe_request(&self);
    /// Host: input from the controller (already checked against the Input permission).
    fn on_input(&self, event: RemoteInput);
    fn on_ended(&self, end: EndInfo);
    fn on_error(&self, message: String);
}

pub(crate) fn perms_to_ffi(p: scrin_session::Permissions) -> Vec<SessionPermission> {
    p.iter().map(SessionPermission::from_core).collect()
}

pub(crate) fn perms_from_ffi(list: &[SessionPermission]) -> scrin_session::Permissions {
    list.iter()
        .fold(scrin_session::Permissions::empty(), |acc, p| {
            acc.with(p.to_core())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_mapping_round_trips_every_variant() {
        for (i, p) in FFI_ALL.iter().enumerate() {
            assert_eq!(p.to_core(), Permission::ALL[i]);
            assert_eq!(SessionPermission::from_core(Permission::ALL[i]), *p);
        }
    }
}
