//! Unreliable QUIC datagrams (media path).
//!
//! The size limit moves with the path MTU, so it is checked on every send and
//! reported as a typed error instead of a silent drop.

use bytes::Bytes;
use iroh::endpoint::{Connection, SendDatagramError};

use crate::{NetError, Result};

/// Current maximum datagram payload, or [`NetError::DatagramsUnsupported`].
pub fn max_datagram_size(conn: &Connection) -> Result<usize> {
    conn.max_datagram_size()
        .ok_or(NetError::DatagramsUnsupported)
}

/// Sends one datagram, dropping older queued ones under congestion.
pub fn send_datagram(conn: &Connection, payload: Bytes) -> Result<()> {
    let max = max_datagram_size(conn)?;
    if payload.len() > max {
        return Err(NetError::DatagramTooLarge {
            len: payload.len(),
            max,
        });
    }
    conn.send_datagram(payload).map_err(|e| map_send(e, max))
}

/// Receives the next datagram. Errors only when the connection ends.
pub async fn recv_datagram(conn: &Connection) -> Result<Bytes> {
    conn.read_datagram()
        .await
        .map_err(|e| NetError::Connection(e.to_string()))
}

fn map_send(e: SendDatagramError, max: usize) -> NetError {
    match e {
        SendDatagramError::UnsupportedByPeer | SendDatagramError::Disabled => {
            NetError::DatagramsUnsupported
        }
        // The MTU estimate shrank between our check and the send.
        SendDatagramError::TooLarge => NetError::DatagramTooLarge { len: max + 1, max },
        SendDatagramError::ConnectionLost(e) => NetError::Connection(e.to_string()),
    }
}
