//! Hand-packed 16-byte media datagram header (hot path, not protobuf).
//!
//! Every video/audio/cursor/FEC datagram starts with this header. All
//! multi-byte integers are big-endian.
//!
//! ```text
//! offset size  field
//!  0     1     version (high nibble, = 1) | kind (low nibble)
//!  1     1     flags (high nibble)        | stream_id (low nibble, 0..=15)
//!  2     4     frame_id         u32  wrapping per-stream frame counter
//!  6     2     shard_index      u16  index of this shard in the frame (data first, then parity)
//!  8     2     shard_count      u16  total shards of the frame (data + parity), >= 1
//! 10     2     data_shards      u16  data shards of the frame, 1..=shard_count
//! 12     4     capture_ts       u32  capture time in 100 us units (wraps after ~119 h)
//! ```
//!
//! Kinds: 0 video, 1 audio, 2 cursor, 3 fec (parity shard). Flags (bits of the
//! high nibble of byte 1): bit 7 keyframe, bit 6 end-of-frame, bits 5..4
//! reserved and must be zero.

use thiserror::Error;

/// Size of the encoded header in bytes.
pub const HEADER_LEN: usize = 16;
/// Header version produced and accepted by this build.
pub const VERSION: u8 = 1;

const FLAG_KEYFRAME: u8 = 0b1000;
const FLAG_END_OF_FRAME: u8 = 0b0100;
const FLAGS_RESERVED: u8 = 0b0011;

/// Payload type carried by a datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MediaKind {
    /// Encoded video bitstream shard.
    Video = 0,
    /// Opus audio packet.
    Audio = 1,
    /// Cursor update.
    Cursor = 2,
    /// Reed-Solomon parity shard.
    Fec = 3,
}

impl MediaKind {
    fn from_nibble(n: u8) -> Result<Self, MediaHeaderError> {
        match n {
            0 => Ok(Self::Video),
            1 => Ok(Self::Audio),
            2 => Ok(Self::Cursor),
            3 => Ok(Self::Fec),
            other => Err(MediaHeaderError::UnknownKind(other)),
        }
    }
}

/// Decoded media header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaHeader {
    /// Payload type.
    pub kind: MediaKind,
    /// The shard belongs to a keyframe.
    pub keyframe: bool,
    /// Last data shard of the frame.
    pub end_of_frame: bool,
    /// Stream id from `VideoConfig.stream_id` (0..=15).
    pub stream_id: u8,
    /// Wrapping per-stream frame counter.
    pub frame_id: u32,
    /// Index of this shard (data shards first, then parity).
    pub shard_index: u16,
    /// Total shards of the frame (data + parity).
    pub shard_count: u16,
    /// Number of data shards of the frame.
    pub data_shards: u16,
    /// Capture time in 100 µs units, wrapping.
    pub capture_ts_100us: u32,
}

/// Why a header could not be encoded or decoded.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum MediaHeaderError {
    /// Fewer than [`HEADER_LEN`] bytes.
    #[error("media header too short: {0} bytes, need {HEADER_LEN}")]
    TooShort(usize),
    /// Version nibble is not [`VERSION`].
    #[error("unsupported media header version {0}")]
    UnsupportedVersion(u8),
    /// Kind nibble is not a known [`MediaKind`].
    #[error("unknown media kind {0}")]
    UnknownKind(u8),
    /// A reserved flag bit was set.
    #[error("reserved media header flags set: {0:#06b}")]
    ReservedFlags(u8),
    /// `stream_id` does not fit in 4 bits.
    #[error("stream id {0} exceeds 15")]
    StreamIdOutOfRange(u8),
    /// Shard index/count/data counts are inconsistent.
    #[error("invalid shard layout: index {index}, count {count}, data {data}")]
    InvalidShards {
        /// `shard_index`
        index: u16,
        /// `shard_count`
        count: u16,
        /// `data_shards`
        data: u16,
    },
}

impl MediaHeader {
    fn validate(&self) -> Result<(), MediaHeaderError> {
        if self.stream_id > 0x0F {
            return Err(MediaHeaderError::StreamIdOutOfRange(self.stream_id));
        }
        if self.shard_count == 0
            || self.data_shards == 0
            || self.data_shards > self.shard_count
            || self.shard_index >= self.shard_count
        {
            return Err(MediaHeaderError::InvalidShards {
                index: self.shard_index,
                count: self.shard_count,
                data: self.data_shards,
            });
        }
        Ok(())
    }

    /// Writes the header into `out`.
    pub fn encode(&self, out: &mut [u8; HEADER_LEN]) -> Result<(), MediaHeaderError> {
        self.validate()?;
        let mut flags = 0u8;
        if self.keyframe {
            flags |= FLAG_KEYFRAME;
        }
        if self.end_of_frame {
            flags |= FLAG_END_OF_FRAME;
        }
        out[0] = (VERSION << 4) | (self.kind as u8);
        out[1] = (flags << 4) | self.stream_id;
        out[2..6].copy_from_slice(&self.frame_id.to_be_bytes());
        out[6..8].copy_from_slice(&self.shard_index.to_be_bytes());
        out[8..10].copy_from_slice(&self.shard_count.to_be_bytes());
        out[10..12].copy_from_slice(&self.data_shards.to_be_bytes());
        out[12..16].copy_from_slice(&self.capture_ts_100us.to_be_bytes());
        Ok(())
    }

    /// Returns the encoded header as a new array.
    pub fn to_bytes(&self) -> Result<[u8; HEADER_LEN], MediaHeaderError> {
        let mut out = [0u8; HEADER_LEN];
        self.encode(&mut out)?;
        Ok(out)
    }

    /// Parses the first [`HEADER_LEN`] bytes of `buf`; the payload follows.
    pub fn decode(buf: &[u8]) -> Result<Self, MediaHeaderError> {
        let Some(b) = buf.first_chunk::<HEADER_LEN>() else {
            return Err(MediaHeaderError::TooShort(buf.len()));
        };
        let version = b[0] >> 4;
        if version != VERSION {
            return Err(MediaHeaderError::UnsupportedVersion(version));
        }
        let kind = MediaKind::from_nibble(b[0] & 0x0F)?;
        let flags = b[1] >> 4;
        if flags & FLAGS_RESERVED != 0 {
            return Err(MediaHeaderError::ReservedFlags(flags));
        }
        let header = Self {
            kind,
            keyframe: flags & FLAG_KEYFRAME != 0,
            end_of_frame: flags & FLAG_END_OF_FRAME != 0,
            stream_id: b[1] & 0x0F,
            frame_id: u32::from_be_bytes([b[2], b[3], b[4], b[5]]),
            shard_index: u16::from_be_bytes([b[6], b[7]]),
            shard_count: u16::from_be_bytes([b[8], b[9]]),
            data_shards: u16::from_be_bytes([b[10], b[11]]),
            capture_ts_100us: u32::from_be_bytes([b[12], b[13], b[14], b[15]]),
        };
        header.validate()?;
        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MediaHeader {
        MediaHeader {
            kind: MediaKind::Video,
            keyframe: true,
            end_of_frame: false,
            stream_id: 3,
            frame_id: 0xDEAD_BEEF,
            shard_index: 7,
            shard_count: 12,
            data_shards: 10,
            capture_ts_100us: 0x0102_0304,
        }
    }

    #[test]
    fn round_trips_every_kind_and_flag() {
        for kind in [
            MediaKind::Video,
            MediaKind::Audio,
            MediaKind::Cursor,
            MediaKind::Fec,
        ] {
            for (keyframe, end_of_frame) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let h = MediaHeader {
                    kind,
                    keyframe,
                    end_of_frame,
                    ..sample()
                };
                let bytes = h.to_bytes().expect("encode");
                assert_eq!(MediaHeader::decode(&bytes), Ok(h));
            }
        }
    }

    #[test]
    fn exact_byte_layout() {
        let bytes = sample().to_bytes().expect("encode");
        assert_eq!(
            bytes,
            [
                0x10, 0x83, 0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x07, 0x00, 0x0C, 0x00, 0x0A, 0x01, 0x02,
                0x03, 0x04
            ]
        );
    }

    #[test]
    fn decodes_with_trailing_payload() {
        let mut buf = sample().to_bytes().expect("encode").to_vec();
        buf.extend_from_slice(&[0xAA; 100]);
        assert_eq!(MediaHeader::decode(&buf), Ok(sample()));
    }

    #[test]
    fn rejects_short_buffer() {
        let bytes = sample().to_bytes().expect("encode");
        assert_eq!(
            MediaHeader::decode(&bytes[..15]),
            Err(MediaHeaderError::TooShort(15))
        );
        assert_eq!(MediaHeader::decode(&[]), Err(MediaHeaderError::TooShort(0)));
    }

    #[test]
    fn rejects_wrong_version() {
        let mut bytes = sample().to_bytes().expect("encode");
        bytes[0] = (2 << 4) | (bytes[0] & 0x0F);
        assert_eq!(
            MediaHeader::decode(&bytes),
            Err(MediaHeaderError::UnsupportedVersion(2))
        );
        bytes[0] &= 0x0F;
        assert_eq!(
            MediaHeader::decode(&bytes),
            Err(MediaHeaderError::UnsupportedVersion(0))
        );
    }

    #[test]
    fn rejects_unknown_kind_and_reserved_flags() {
        let mut bytes = sample().to_bytes().expect("encode");
        bytes[0] = (VERSION << 4) | 0x09;
        assert_eq!(
            MediaHeader::decode(&bytes),
            Err(MediaHeaderError::UnknownKind(9))
        );
        let mut bytes = sample().to_bytes().expect("encode");
        bytes[1] |= 0x10;
        assert!(matches!(
            MediaHeader::decode(&bytes),
            Err(MediaHeaderError::ReservedFlags(_))
        ));
    }

    #[test]
    fn rejects_inconsistent_shards() {
        for (index, count, data) in [(0, 0, 0), (12, 12, 10), (0, 4, 5), (0, 4, 0)] {
            let h = MediaHeader {
                shard_index: index,
                shard_count: count,
                data_shards: data,
                ..sample()
            };
            assert!(matches!(
                h.to_bytes(),
                Err(MediaHeaderError::InvalidShards { .. })
            ));
        }
        let mut bytes = sample().to_bytes().expect("encode");
        bytes[6..8].copy_from_slice(&99u16.to_be_bytes());
        assert!(matches!(
            MediaHeader::decode(&bytes),
            Err(MediaHeaderError::InvalidShards { .. })
        ));
    }

    #[test]
    fn rejects_stream_id_over_15_on_encode() {
        let h = MediaHeader {
            stream_id: 16,
            ..sample()
        };
        assert_eq!(h.to_bytes(), Err(MediaHeaderError::StreamIdOutOfRange(16)));
    }
}
