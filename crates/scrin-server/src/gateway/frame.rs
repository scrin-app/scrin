//! WebSocket fallback framing (see `GATEWAY.md`) and QUIC varints.
//!
//! Every binary WebSocket message is one frame:
//!
//! | tag  | name     | body                                   |
//! |------|----------|----------------------------------------|
//! | 0x00 | `Data`   | varint stream id, then payload bytes   |
//! | 0x01 | `Dgram`  | payload bytes                          |
//! | 0x02 | `Fin`    | varint stream id                       |
//! | 0x03 | `Reset`  | varint stream id, varint error code    |
//! | 0x04 | `Stop`   | varint stream id, varint error code    |
//!
//! Varints are QUIC variable-length integers (RFC 9000 §16), max 2^62-1.

use bytes::{BufMut, Bytes, BytesMut};

pub const TAG_DATA: u8 = 0x00;
pub const TAG_DGRAM: u8 = 0x01;
pub const TAG_FIN: u8 = 0x02;
pub const TAG_RESET: u8 = 0x03;
pub const TAG_STOP: u8 = 0x04;

/// Largest value a QUIC varint can hold.
pub const VARINT_MAX: u64 = (1 << 62) - 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Data {
        id: u64,
        payload: Bytes,
    },
    Dgram(Bytes),
    Fin {
        id: u64,
    },
    Reset {
        id: u64,
        code: u64,
    },
    /// The sender of this frame no longer reads stream `id` (QUIC `STOP_SENDING`).
    Stop {
        id: u64,
        code: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("empty frame")]
    Empty,
    #[error("unknown frame tag {0:#04x}")]
    UnknownTag(u8),
    #[error("truncated varint")]
    Truncated,
    #[error("trailing bytes after frame")]
    Trailing,
}

/// Appends `v` as a QUIC varint. Values above [`VARINT_MAX`] are clamped
/// (callers only pass stream ids and codes, which stay far below it).
pub fn put_varint(out: &mut BytesMut, v: u64) {
    let v = v.min(VARINT_MAX);
    if v < 1 << 6 {
        out.put_u8(u8::try_from(v).unwrap_or(0));
    } else if v < 1 << 14 {
        out.put_u16(u16::try_from(v).unwrap_or(0) | 0x4000);
    } else if v < 1 << 30 {
        out.put_u32(u32::try_from(v).unwrap_or(0) | 0x8000_0000);
    } else {
        out.put_u64(v | 0xc000_0000_0000_0000);
    }
}

/// Reads a varint from the front of `buf`; returns `(value, bytes used)`.
pub fn get_varint(buf: &[u8]) -> Result<(u64, usize), FrameError> {
    let first = *buf.first().ok_or(FrameError::Truncated)?;
    let len = 1usize << (first >> 6);
    let bytes = buf.get(..len).ok_or(FrameError::Truncated)?;
    let mut v = u64::from(first & 0x3f);
    for b in &bytes[1..] {
        v = (v << 8) | u64::from(*b);
    }
    Ok((v, len))
}

impl Frame {
    #[must_use]
    pub fn encode(&self) -> Bytes {
        let mut out = BytesMut::new();
        match self {
            Self::Data { id, payload } => {
                out.reserve(9 + payload.len());
                out.put_u8(TAG_DATA);
                put_varint(&mut out, *id);
                out.extend_from_slice(payload);
            }
            Self::Dgram(p) => {
                out.reserve(1 + p.len());
                out.put_u8(TAG_DGRAM);
                out.extend_from_slice(p);
            }
            Self::Fin { id } => {
                out.put_u8(TAG_FIN);
                put_varint(&mut out, *id);
            }
            Self::Reset { id, code } => {
                out.put_u8(TAG_RESET);
                put_varint(&mut out, *id);
                put_varint(&mut out, *code);
            }
            Self::Stop { id, code } => {
                out.put_u8(TAG_STOP);
                put_varint(&mut out, *id);
                put_varint(&mut out, *code);
            }
        }
        out.freeze()
    }

    pub fn decode(msg: &Bytes) -> Result<Self, FrameError> {
        let (&tag, rest) = msg.split_first().ok_or(FrameError::Empty)?;
        match tag {
            TAG_DATA => {
                let (id, n) = get_varint(rest)?;
                Ok(Self::Data {
                    id,
                    payload: msg.slice(1 + n..),
                })
            }
            TAG_DGRAM => Ok(Self::Dgram(msg.slice(1..))),
            TAG_FIN => {
                let (id, n) = get_varint(rest)?;
                if n != rest.len() {
                    return Err(FrameError::Trailing);
                }
                Ok(Self::Fin { id })
            }
            TAG_RESET | TAG_STOP => {
                let (id, n) = get_varint(rest)?;
                let (code, m) = get_varint(&rest[n..])?;
                if n + m != rest.len() {
                    return Err(FrameError::Trailing);
                }
                Ok(if tag == TAG_RESET {
                    Self::Reset { id, code }
                } else {
                    Self::Stop { id, code }
                })
            }
            t => Err(FrameError::UnknownTag(t)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_round_trips_at_every_width() {
        for v in [
            0,
            63,
            64,
            16_383,
            16_384,
            (1 << 30) - 1,
            1 << 30,
            VARINT_MAX,
        ] {
            let mut b = BytesMut::new();
            put_varint(&mut b, v);
            let (got, n) = get_varint(&b).expect("decode");
            assert_eq!((got, n), (v, b.len()), "{v}");
        }
        // RFC 9000 §A.1 examples.
        assert_eq!(get_varint(&[0x25]).expect("1"), (37, 1));
        assert_eq!(get_varint(&[0x7b, 0xbd]).expect("2"), (15_293, 2));
        assert_eq!(
            get_varint(&[0x9d, 0x7f, 0x3e, 0x7d]).expect("4"),
            (494_878_333, 4)
        );
    }

    #[test]
    fn frames_round_trip() {
        let frames = [
            Frame::Data {
                id: 0,
                payload: Bytes::from_static(b"hello"),
            },
            Frame::Data {
                id: 1_000,
                payload: Bytes::new(),
            },
            Frame::Dgram(Bytes::from_static(&[1, 2, 3])),
            Frame::Fin { id: 4 },
            Frame::Reset { id: 7, code: 300 },
            Frame::Stop { id: 9, code: 1 },
        ];
        for f in frames {
            assert_eq!(Frame::decode(&f.encode()).expect("decode"), f);
        }
    }

    #[test]
    fn bad_frames_are_rejected() {
        assert_eq!(Frame::decode(&Bytes::new()), Err(FrameError::Empty));
        assert_eq!(
            Frame::decode(&Bytes::from_static(&[9])),
            Err(FrameError::UnknownTag(9))
        );
        assert_eq!(
            Frame::decode(&Bytes::from_static(&[0, 0x40])),
            Err(FrameError::Truncated)
        );
        assert_eq!(
            Frame::decode(&Bytes::from_static(&[2, 1, 0])),
            Err(FrameError::Trailing)
        );
    }
}
