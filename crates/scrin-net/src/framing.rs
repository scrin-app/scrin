//! Length-prefixed frames on QUIC bi-streams, and the stream-kind byte.
//!
//! Every bi-stream starts with one [`StreamKind`] byte written by the opener.
//! After that, each frame is a `u32` big-endian length then that many bytes.
//! A reader never allocates more than its cap: the length is checked first.

use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{NetError, Result};

/// Hard ceiling on one frame.
pub const MAX_FRAME_LEN: usize = 4 * 1024 * 1024;

/// Application error code used to stop a stream whose kind we do not know.
pub const UNKNOWN_KIND_CODE: VarInt = VarInt::from_u32(0x5c01);

/// How long the opener has to send the kind byte before we drop the stream.
const KIND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum StreamKind {
    Control = 0,
    Input = 1,
    Clipboard = 2,
    File = 3,
    Chat = 4,
    Tunnel = 5,
}

impl StreamKind {
    #[must_use]
    pub const fn as_byte(self) -> u8 {
        self as u8
    }

    /// `None` for kinds added by newer peers; the stream is then stopped.
    #[must_use]
    pub const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Control,
            1 => Self::Input,
            2 => Self::Clipboard,
            3 => Self::File,
            4 => Self::Chat,
            5 => Self::Tunnel,
            _ => return None,
        })
    }
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_FRAME_LEN {
        return Err(NetError::FrameTooLarge {
            len: payload.len(),
            max: MAX_FRAME_LEN,
        });
    }
    let len = u32::try_from(payload.len()).map_err(|_| NetError::Protocol("frame length"))?;
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    w.write_all(&buf).await?;
    Ok(())
}

/// Reads one frame up to [`MAX_FRAME_LEN`]. `Ok(None)` on a clean end of stream.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Vec<u8>>> {
    read_frame_capped(r, MAX_FRAME_LEN).await
}

/// Reads one frame no larger than `max` (itself capped at [`MAX_FRAME_LEN`]).
pub async fn read_frame_capped<R: AsyncRead + Unpin>(
    r: &mut R,
    max: usize,
) -> Result<Option<Vec<u8>>> {
    let max = max.min(MAX_FRAME_LEN);
    let mut len_buf = [0u8; 4];
    // A clean end of stream is only one that falls between frames.
    if r.read(&mut len_buf[..1]).await? == 0 {
        return Ok(None);
    }
    read_exact(r, &mut len_buf[1..]).await?;
    let len = usize::try_from(u32::from_be_bytes(len_buf))
        .map_err(|_| NetError::Protocol("frame length"))?;
    if len > max {
        return Err(NetError::FrameTooLarge { len, max });
    }
    let mut body = vec![0u8; len];
    read_exact(r, &mut body).await?;
    Ok(Some(body))
}

async fn read_exact<R: AsyncRead + Unpin>(r: &mut R, buf: &mut [u8]) -> Result<()> {
    match r.read_exact(buf).await {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Err(NetError::StreamClosed),
        Err(e) => Err(e.into()),
    }
}

/// Opens a bi-stream and announces its kind.
pub async fn open_stream(conn: &Connection, kind: StreamKind) -> Result<(SendStream, RecvStream)> {
    let (mut send, recv) = conn
        .open_bi()
        .await
        .map_err(|e| NetError::Connection(e.to_string()))?;
    send.write_all(&[kind.as_byte()])
        .await
        .map_err(|e| NetError::Connection(e.to_string()))?;
    Ok((send, recv))
}

/// Accepts the next bi-stream of a known kind.
///
/// Streams of an unknown kind (from a newer peer) are stopped with
/// [`UNKNOWN_KIND_CODE`] and skipped, as are streams that close or stall
/// before sending their kind byte. Errors only when the connection ends.
pub async fn accept_stream(conn: &Connection) -> Result<(StreamKind, SendStream, RecvStream)> {
    loop {
        let (mut send, mut recv) = conn
            .accept_bi()
            .await
            .map_err(|e| NetError::Connection(e.to_string()))?;
        let mut kind = [0u8; 1];
        let Ok(Ok(())) = tokio::time::timeout(KIND_TIMEOUT, recv.read_exact(&mut kind)).await
        else {
            let _ = recv.stop(UNKNOWN_KIND_CODE);
            continue;
        };
        if let Some(kind) = StreamKind::from_byte(kind[0]) {
            return Ok((kind, send, recv));
        }
        let _ = recv.stop(UNKNOWN_KIND_CODE);
        let _ = send.reset(UNKNOWN_KIND_CODE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_round_trip_and_eof_is_clean() {
        let (mut a, mut b) = tokio::io::duplex(1 << 16);
        write_frame(&mut a, b"hello").await.expect("write");
        write_frame(&mut a, b"").await.expect("write empty");
        drop(a);
        assert_eq!(
            read_frame(&mut b).await.expect("read"),
            Some(b"hello".to_vec())
        );
        assert_eq!(read_frame(&mut b).await.expect("read"), Some(Vec::new()));
        assert_eq!(read_frame(&mut b).await.expect("eof"), None);
    }

    #[tokio::test]
    async fn oversized_prefix_is_rejected_before_allocating() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&u32::MAX.to_be_bytes()).await.expect("write");
        assert!(matches!(
            read_frame(&mut b).await,
            Err(NetError::FrameTooLarge { .. })
        ));
    }

    #[tokio::test]
    async fn cap_below_max_is_enforced() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        write_frame(&mut a, &[7u8; 100]).await.expect("write");
        assert!(matches!(
            read_frame_capped(&mut b, 99).await,
            Err(NetError::FrameTooLarge { len: 100, max: 99 })
        ));
    }

    #[tokio::test]
    async fn truncated_frame_is_not_a_clean_eof() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&10u32.to_be_bytes()).await.expect("write");
        a.write_all(b"abc").await.expect("write");
        drop(a);
        assert!(matches!(
            read_frame(&mut b).await,
            Err(NetError::StreamClosed)
        ));
    }

    #[tokio::test]
    async fn writer_refuses_oversized_payload() {
        let (mut a, _b) = tokio::io::duplex(64);
        let big = vec![0u8; MAX_FRAME_LEN + 1];
        assert!(matches!(
            write_frame(&mut a, &big).await,
            Err(NetError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn kind_bytes_are_stable() {
        for b in 0u8..=5 {
            assert_eq!(StreamKind::from_byte(b).expect("known").as_byte(), b);
        }
        assert_eq!(StreamKind::Tunnel.as_byte(), 5);
        assert!(StreamKind::from_byte(6).is_none());
        assert!(StreamKind::from_byte(255).is_none());
    }
}
