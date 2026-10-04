//! iroh transport, framing, stream kinds and datagrams for scrin.
//!
//! - [`endpoint`]: an iroh endpoint keyed by the scrin device identity.
//! - [`framing`]: length-prefixed frames and the stream-kind byte on bi-streams.
//! - [`datagram`]: size-checked unreliable datagrams for media.
//! - [`handshake`]: version negotiation, quick-connect pairing (SPAKE2) and
//!   trusted (unattended) authentication on the Control stream.
//! - [`reconnect`]: a pure exponential backoff policy.

pub mod datagram;
pub mod endpoint;
mod error;
pub mod framing;
pub mod handshake;
pub mod reconnect;

pub use endpoint::{NetConfig, NetEndpoint, RelayConfig, remote_device_id};
pub use error::{NetError, Result};
pub use framing::StreamKind;
pub use iroh::endpoint::{Connection, Incoming, RecvStream, SendStream};
pub use iroh::{EndpointAddr, RelayUrl, TransportAddr};

/// ALPN of the scrin protocol. Equal to [`scrin_crypto::PROTOCOL`].
pub const ALPN: &[u8] = b"scrin/1";
