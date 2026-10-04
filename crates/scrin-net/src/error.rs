//! One error type for the transport layer.

use crate::handshake::RejectReason;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NetError {
    #[error("binding the endpoint failed: {0}")]
    Bind(String),
    #[error("invalid network config: {0}")]
    Config(&'static str),
    #[error("dialling the peer failed: {0}")]
    Connect(String),
    #[error("connection error: {0}")]
    Connection(String),
    #[error("stream i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("stream closed by the peer")]
    StreamClosed,
    #[error("frame of {len} bytes exceeds the {max}-byte limit")]
    FrameTooLarge { len: usize, max: usize },
    #[error("datagrams are not supported on this connection")]
    DatagramsUnsupported,
    #[error("datagram of {len} bytes exceeds the current maximum of {max}")]
    DatagramTooLarge { len: usize, max: usize },
    #[error("protocol violation: {0}")]
    Protocol(&'static str),
    #[error("no protocol version in common with the peer")]
    VersionMismatch,
    #[error("the peer rejected the handshake: {0}")]
    Rejected(RejectReason),
    #[error("pairing failed: wrong code or tampered handshake")]
    PairingFailed,
    #[error("the one-time code was already used")]
    CodeConsumed,
    #[error("the one-time code expired")]
    CodeExpired,
    #[error("the peer is not in the trust store")]
    Untrusted,
    #[error("the peer's signature did not verify")]
    BadSignature,
    #[error("the peer's timestamp is outside the allowed clock skew")]
    StaleTimestamp,
    #[error("the host refused trusted access")]
    AuthRejected,
    #[error("the handshake timed out")]
    Timeout,
    #[error("random source unavailable: {0}")]
    Random(String),
    #[error(transparent)]
    Crypto(#[from] scrin_crypto::Error),
}

pub type Result<T> = std::result::Result<T, NetError>;
