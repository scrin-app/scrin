//! Frame sharding with Reed-Solomon forward error correction.
//!
//! An encoded frame (one video access unit or one audio packet) is split into
//! equally sized *data shards* and protected by a configurable number of
//! *parity shards*. Every shard travels in its own QUIC datagram behind a
//! fixed 16-byte [`ShardHeader`].
//!
//! # Wire layout of the shard header (16 bytes, big endian)
//!
//! | offset | size | field                                              |
//! |-------:|-----:|----------------------------------------------------|
//! | 0      | 1    | `version` = 1                                      |
//! | 1      | 1    | `kind`: 0 video, 1 audio                           |
//! | 2      | 1    | `flags`: bit0 keyframe, bit1 parity, others zero   |
//! | 3      | 1    | reserved, zero                                     |
//! | 4      | 4    | `frame_id` (u32, wraps)                            |
//! | 8      | 2    | `shard_index` (0-based, data shards first)         |
//! | 10     | 2    | `shard_count` (data + parity, ≤ 1024)              |
//! | 12     | 2    | `data_shards` (≤ `shard_count`)                    |
//! | 14     | 2    | `payload_len` (shard size in bytes, same per frame) |
//!
//! # Frame length
//!
//! The header has no room for a 32-bit frame length, so the length travels
//! *inside* the protected data: the encoder shards the byte stream
//! `frame_len (u32 BE) ‖ frame ‖ zero padding`. Because the prefix is part of
//! the Reed-Solomon data it is recovered together with the frame, and the
//! receiver strips the padding with it.
//!
//! All shards of a frame share one size (Reed-Solomon needs equal shards). The
//! encoder picks the smallest even size that fits the frame into the minimum
//! number of shards, so padding is always below `data_shards + 2` bytes rather
//! than up to a whole shard.

use std::collections::{HashMap, HashSet};

/// Size of the serialized [`ShardHeader`].
pub const HEADER_LEN: usize = 16;
/// Largest shard payload in bytes; header + payload stays within 1200-byte datagrams.
pub const MAX_SHARD_PAYLOAD: usize = 1150;
/// Upper bound on data + parity shards of one frame.
pub const MAX_SHARD_COUNT: u16 = 1024;
/// Largest accepted parity ratio.
pub const MAX_PARITY_RATIO: f32 = 0.5;
/// Bytes of the big-endian frame length prefix carried in the data shards.
pub const FRAME_LEN_PREFIX: usize = 4;
/// Current header version.
pub const HEADER_VERSION: u8 = 1;

const FLAG_KEYFRAME: u8 = 0b01;
const FLAG_PARITY: u8 = 0b10;
const FLAG_MASK: u8 = FLAG_KEYFRAME | FLAG_PARITY;
// Even, so equal-size shards can always be rounded up to an even length.
const _: () = assert!(MAX_SHARD_PAYLOAD.is_multiple_of(2));

/// Which media stream a shard belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// Encoded video access unit.
    Video,
    /// Encoded audio packet.
    Audio,
}

impl MediaKind {
    fn to_wire(self) -> u8 {
        match self {
            Self::Video => 0,
            Self::Audio => 1,
        }
    }

    fn from_wire(byte: u8) -> Result<Self, FecError> {
        match byte {
            0 => Ok(Self::Video),
            1 => Ok(Self::Audio),
            other => Err(FecError::Kind(other)),
        }
    }
}

/// Errors from sharding, parsing or reassembly.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FecError {
    /// Datagram shorter than the header.
    #[error("datagram of {0} bytes is shorter than the shard header")]
    Truncated(usize),
    /// Unknown header version.
    #[error("unsupported shard header version {0}")]
    Version(u8),
    /// Unknown media kind byte.
    #[error("unknown media kind {0}")]
    Kind(u8),
    /// Unknown flag bits or non-zero reserved byte.
    #[error("unknown flags or reserved bits {0:#04x}")]
    Flags(u8),
    /// Shard counts, index or size outside the allowed geometry.
    #[error("invalid shard geometry")]
    Geometry,
    /// Header `payload_len` disagrees with the datagram.
    #[error("payload length {declared} does not match the {actual} bytes received")]
    PayloadLen {
        /// Length declared in the header.
        declared: usize,
        /// Bytes actually present.
        actual: usize,
    },
    /// Shard disagrees with earlier shards of the same frame.
    #[error("shard inconsistent with earlier shards of frame {0}")]
    Inconsistent(u32),
    /// Frame needs more than [`MAX_SHARD_COUNT`] shards.
    #[error("frame of {0} bytes is too large to shard")]
    FrameTooLarge(usize),
    /// Parity ratio outside `0.0..=0.5` (or NaN).
    #[error("parity ratio {0} outside 0.0..=0.5")]
    ParityRatio(f32),
    /// Reed-Solomon encode or decode failed.
    #[error("reed-solomon: {0}")]
    ReedSolomon(String),
    /// Recovered length prefix is larger than the shard data.
    #[error("corrupt frame length prefix in frame {0}")]
    CorruptLength(u32),
}

impl From<reed_solomon_simd::Error> for FecError {
    fn from(err: reed_solomon_simd::Error) -> Self {
        Self::ReedSolomon(err.to_string())
    }
}

/// Parsed 16-byte shard header. See the module docs for the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardHeader {
    /// Media stream.
    pub kind: MediaKind,
    /// Frame is a keyframe (decodable on its own).
    pub keyframe: bool,
    /// Wrapping frame counter.
    pub frame_id: u32,
    /// Shard position; `>= data_shards` means parity.
    pub shard_index: u16,
    /// Total data + parity shards of the frame.
    pub shard_count: u16,
    /// Data shards of the frame.
    pub data_shards: u16,
    /// Shard payload size in bytes (identical for every shard of the frame).
    pub payload_len: u16,
}

impl ShardHeader {
    /// Whether this shard carries parity rather than frame data.
    pub fn is_parity(&self) -> bool {
        self.shard_index >= self.data_shards
    }

    /// Serialize to the 16-byte wire form.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut flags = 0;
        if self.keyframe {
            flags |= FLAG_KEYFRAME;
        }
        if self.is_parity() {
            flags |= FLAG_PARITY;
        }
        let mut out = [0u8; HEADER_LEN];
        out[0] = HEADER_VERSION;
        out[1] = self.kind.to_wire();
        out[2] = flags;
        out[4..8].copy_from_slice(&self.frame_id.to_be_bytes());
        out[8..10].copy_from_slice(&self.shard_index.to_be_bytes());
        out[10..12].copy_from_slice(&self.shard_count.to_be_bytes());
        out[12..14].copy_from_slice(&self.data_shards.to_be_bytes());
        out[14..16].copy_from_slice(&self.payload_len.to_be_bytes());
        out
    }

    /// Parse and validate a header from the start of `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<Self, FecError> {
        let Some(raw) = bytes.get(..HEADER_LEN) else {
            return Err(FecError::Truncated(bytes.len()));
        };
        if raw[0] != HEADER_VERSION {
            return Err(FecError::Version(raw[0]));
        }
        let kind = MediaKind::from_wire(raw[1])?;
        let flags = raw[2];
        if flags & !FLAG_MASK != 0 || raw[3] != 0 {
            return Err(FecError::Flags(flags | raw[3]));
        }
        let be16 = |at: usize| u16::from_be_bytes([raw[at], raw[at + 1]]);
        let header = Self {
            kind,
            keyframe: flags & FLAG_KEYFRAME != 0,
            frame_id: u32::from_be_bytes([raw[4], raw[5], raw[6], raw[7]]),
            shard_index: be16(8),
            shard_count: be16(10),
            data_shards: be16(12),
            payload_len: be16(14),
        };
        header.validate()?;
        if (flags & FLAG_PARITY != 0) != header.is_parity() {
            return Err(FecError::Flags(flags));
        }
        Ok(header)
    }

    /// Check counts, index and size against the allowed geometry.
    pub fn validate(&self) -> Result<(), FecError> {
        let size = usize::from(self.payload_len);
        let ok = self.data_shards >= 1
            && self.data_shards <= self.shard_count
            && self.shard_count <= MAX_SHARD_COUNT
            && self.shard_index < self.shard_count
            && size >= 2
            && size % 2 == 0
            && size <= MAX_SHARD_PAYLOAD
            // The first data shard must at least hold the length prefix.
            && usize::from(self.data_shards) * size >= FRAME_LEN_PREFIX;
        if ok { Ok(()) } else { Err(FecError::Geometry) }
    }
}

/// One shard: header plus `payload_len` bytes of data or parity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shard {
    /// Shard header.
    pub header: ShardHeader,
    /// Shard bytes; length equals `header.payload_len`.
    pub payload: Vec<u8>,
}

impl Shard {
    /// Serialize header + payload into a datagram.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.payload.len());
        self.write_to(&mut out);
        out
    }

    /// Append header + payload to `out`.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.header.encode());
        out.extend_from_slice(&self.payload);
    }

    /// Parse and validate a datagram.
    pub fn from_bytes(datagram: &[u8]) -> Result<Self, FecError> {
        let header = ShardHeader::decode(datagram)?;
        let payload = &datagram[HEADER_LEN..];
        let declared = usize::from(header.payload_len);
        if payload.len() != declared {
            return Err(FecError::PayloadLen {
                declared,
                actual: payload.len(),
            });
        }
        Ok(Self {
            header,
            payload: payload.to_vec(),
        })
    }
}

/// Splits frames into data + parity shards.
#[derive(Debug, Clone)]
pub struct FrameEncoder {
    kind: MediaKind,
    parity_ratio: f32,
}

impl FrameEncoder {
    /// Create an encoder; `parity_ratio` is parity shards per data shard (0.0–0.5).
    pub fn new(kind: MediaKind, parity_ratio: f32) -> Result<Self, FecError> {
        let mut encoder = Self {
            kind,
            parity_ratio: 0.0,
        };
        encoder.set_parity_ratio(parity_ratio)?;
        Ok(encoder)
    }

    /// Change the parity ratio (e.g. from the bandwidth estimator's loss rate).
    pub fn set_parity_ratio(&mut self, ratio: f32) -> Result<(), FecError> {
        if !(0.0..=MAX_PARITY_RATIO).contains(&ratio) {
            return Err(FecError::ParityRatio(ratio));
        }
        self.parity_ratio = ratio;
        Ok(())
    }

    /// Current parity ratio.
    pub fn parity_ratio(&self) -> f32 {
        self.parity_ratio
    }

    /// Parity shards produced for `data_shards` data shards: `ceil(data × ratio)`,
    /// at least one when the ratio is positive.
    pub fn parity_count(&self, data_shards: u16) -> u16 {
        if self.parity_ratio <= 0.0 {
            return 0;
        }
        let parity = (f32::from(data_shards) * self.parity_ratio).ceil();
        // ratio ≤ 0.5 and data ≤ u16::MAX, so the value is in 0..=32768.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let parity = parity as u16;
        parity.max(1)
    }

    /// Shard one frame. Returns data shards (index order) followed by parity shards.
    pub fn encode(
        &self,
        frame_id: u32,
        keyframe: bool,
        frame: &[u8],
    ) -> Result<Vec<Shard>, FecError> {
        let frame_len =
            u32::try_from(frame.len()).map_err(|_| FecError::FrameTooLarge(frame.len()))?;
        let total = frame.len() + FRAME_LEN_PREFIX;
        let data_count = total.div_ceil(MAX_SHARD_PAYLOAD);
        let data_shards = u16::try_from(data_count)
            .ok()
            .filter(|&d| d <= MAX_SHARD_COUNT)
            .ok_or(FecError::FrameTooLarge(frame.len()))?;
        let parity_shards = self.parity_count(data_shards);
        let shard_count = data_shards
            .checked_add(parity_shards)
            .filter(|&c| c <= MAX_SHARD_COUNT)
            .ok_or(FecError::FrameTooLarge(frame.len()))?;
        // Smallest even size fitting `total` into `data_count` shards; ≤ MAX_SHARD_PAYLOAD.
        let shard_size = total.div_ceil(data_count).next_multiple_of(2);
        let payload_len = u16::try_from(shard_size).map_err(|_| FecError::Geometry)?;

        let mut stream = Vec::with_capacity(data_count * shard_size);
        stream.extend_from_slice(&frame_len.to_be_bytes());
        stream.extend_from_slice(frame);
        stream.resize(data_count * shard_size, 0);

        let header = |shard_index: u16| ShardHeader {
            kind: self.kind,
            keyframe,
            frame_id,
            shard_index,
            shard_count,
            data_shards,
            payload_len,
        };
        let mut shards = Vec::with_capacity(usize::from(shard_count));
        for (index, chunk) in (0..data_shards).zip(stream.chunks_exact(shard_size)) {
            shards.push(Shard {
                header: header(index),
                payload: chunk.to_vec(),
            });
        }
        if parity_shards > 0 {
            let recovery = reed_solomon_simd::encode(
                data_count,
                usize::from(parity_shards),
                stream.chunks_exact(shard_size),
            )?;
            for (index, payload) in (data_shards..shard_count).zip(recovery) {
                shards.push(Shard {
                    header: header(index),
                    payload,
                });
            }
        }
        Ok(shards)
    }
}

/// Limits for [`FrameReassembler`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReassemblerConfig {
    /// Frames more than this many ids behind the newest seen are dropped.
    pub window: u32,
    /// Cap on buffered shard bytes across all pending frames; oldest frames are
    /// abandoned first when exceeded.
    pub max_buffered_bytes: usize,
}

impl Default for ReassemblerConfig {
    fn default() -> Self {
        Self {
            window: 64,
            max_buffered_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Counters exposed by [`FrameReassembler::stats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReassemblyStats {
    /// Frames delivered.
    pub completed: u64,
    /// Delivered frames that needed Reed-Solomon recovery.
    pub recovered: u64,
    /// Frames abandoned incomplete (fell out of the window or the byte cap).
    pub lost: u64,
    /// Shards for frames already delivered or older than the window.
    pub late_shards: u64,
    /// Shards received twice.
    pub duplicate_shards: u64,
    /// Shards rejected as malformed or inconsistent.
    pub invalid_shards: u64,
}

/// A frame rebuilt from its shards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedFrame {
    /// Frame id from the shard headers.
    pub frame_id: u32,
    /// Media stream.
    pub kind: MediaKind,
    /// Keyframe flag.
    pub keyframe: bool,
    /// The original frame bytes.
    pub data: Vec<u8>,
    /// Whether parity was needed to rebuild it.
    pub recovered: bool,
}

#[derive(Debug)]
struct PendingFrame {
    kind: MediaKind,
    keyframe: bool,
    shard_count: u16,
    data_shards: u16,
    payload_len: u16,
    shards: Vec<Option<Vec<u8>>>,
    received: u16,
    bytes: usize,
}

impl PendingFrame {
    fn new(header: &ShardHeader) -> Self {
        Self {
            kind: header.kind,
            keyframe: header.keyframe,
            shard_count: header.shard_count,
            data_shards: header.data_shards,
            payload_len: header.payload_len,
            shards: vec![None; usize::from(header.shard_count)],
            received: 0,
            bytes: 0,
        }
    }

    fn matches(&self, header: &ShardHeader) -> bool {
        self.kind == header.kind
            && self.keyframe == header.keyframe
            && self.shard_count == header.shard_count
            && self.data_shards == header.data_shards
            && self.payload_len == header.payload_len
    }
}

/// `a` is strictly newer than `b` in wrapping (serial number) order.
fn is_newer(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < 0x8000_0000
}

/// Rebuilds frames from shards arriving in any order, with losses and duplicates.
///
/// Memory is bounded: at most `window + 1` frames are pending, each holding at
/// most [`MAX_SHARD_COUNT`] × [`MAX_SHARD_PAYLOAD`] bytes, and the total is
/// further capped by [`ReassemblerConfig::max_buffered_bytes`].
#[derive(Debug)]
pub struct FrameReassembler {
    config: ReassemblerConfig,
    pending: HashMap<u32, PendingFrame>,
    delivered: HashSet<u32>,
    newest: Option<u32>,
    buffered: usize,
    stats: ReassemblyStats,
}

impl Default for FrameReassembler {
    fn default() -> Self {
        Self::new(ReassemblerConfig::default())
    }
}

impl FrameReassembler {
    /// Create a reassembler with the given limits.
    pub fn new(config: ReassemblerConfig) -> Self {
        Self {
            config,
            pending: HashMap::new(),
            delivered: HashSet::new(),
            newest: None,
            buffered: 0,
            stats: ReassemblyStats::default(),
        }
    }

    /// Counters so far.
    pub fn stats(&self) -> ReassemblyStats {
        self.stats
    }

    /// Frames currently waiting for more shards.
    pub fn pending_frames(&self) -> usize {
        self.pending.len()
    }

    /// Bytes of shard payload currently buffered.
    pub fn buffered_bytes(&self) -> usize {
        self.buffered
    }

    /// Feed one datagram. Returns the frame once enough shards have arrived.
    pub fn push(&mut self, datagram: &[u8]) -> Result<Option<CompletedFrame>, FecError> {
        let result = Shard::from_bytes(datagram).and_then(|shard| self.accept(shard));
        if result.is_err() {
            self.stats.invalid_shards += 1;
        }
        result
    }

    /// Feed one already parsed shard.
    pub fn push_shard(&mut self, shard: Shard) -> Result<Option<CompletedFrame>, FecError> {
        let result = shard
            .header
            .validate()
            .and_then(|()| {
                let declared = usize::from(shard.header.payload_len);
                if shard.payload.len() == declared {
                    Ok(())
                } else {
                    Err(FecError::PayloadLen {
                        declared,
                        actual: shard.payload.len(),
                    })
                }
            })
            .and_then(|()| self.accept(shard));
        if result.is_err() {
            self.stats.invalid_shards += 1;
        }
        result
    }

    fn accept(&mut self, shard: Shard) -> Result<Option<CompletedFrame>, FecError> {
        let header = shard.header;
        let id = header.frame_id;
        match self.newest {
            Some(newest)
                if !is_newer(id, newest) && newest.wrapping_sub(id) > self.config.window =>
            {
                self.stats.late_shards += 1;
                return Ok(None);
            }
            Some(newest) if is_newer(id, newest) => self.advance(id),
            None => self.newest = Some(id),
            Some(_) => {}
        }
        if self.delivered.contains(&id) {
            self.stats.late_shards += 1;
            return Ok(None);
        }

        let frame = self
            .pending
            .entry(id)
            .or_insert_with(|| PendingFrame::new(&header));
        if !frame.matches(&header) {
            return Err(FecError::Inconsistent(id));
        }
        let slot = &mut frame.shards[usize::from(header.shard_index)];
        if slot.is_some() {
            self.stats.duplicate_shards += 1;
            return Ok(None);
        }
        let len = shard.payload.len();
        *slot = Some(shard.payload);
        frame.received += 1;
        frame.bytes += len;
        self.buffered += len;
        let ready = frame.received >= frame.data_shards;

        if self.buffered > self.config.max_buffered_bytes {
            self.enforce_byte_cap();
            if !self.pending.contains_key(&id) {
                return Ok(None);
            }
        }
        if !ready {
            return Ok(None);
        }
        let Some(frame) = self.pending.remove(&id) else {
            return Ok(None);
        };
        self.buffered -= frame.bytes;
        self.delivered.insert(id);
        let completed = Self::assemble(id, frame)?;
        self.stats.completed += 1;
        if completed.recovered {
            self.stats.recovered += 1;
        }
        Ok(Some(completed))
    }

    /// Move the window forward to `newest`, abandoning frames that fall out of it.
    fn advance(&mut self, newest: u32) {
        self.newest = Some(newest);
        let window = self.config.window;
        let out_of_window = |id: u32| is_newer(newest, id) && newest.wrapping_sub(id) > window;
        let mut abandoned = 0;
        let mut freed = 0;
        self.pending.retain(|&id, frame| {
            let keep = !out_of_window(id);
            if !keep {
                abandoned += 1;
                freed += frame.bytes;
            }
            keep
        });
        self.stats.lost += abandoned;
        self.buffered -= freed;
        self.delivered.retain(|&id| !out_of_window(id));
    }

    /// Abandon the oldest pending frames until under the byte cap.
    fn enforce_byte_cap(&mut self) {
        let Some(newest) = self.newest else { return };
        while self.buffered > self.config.max_buffered_bytes {
            let Some(oldest) = self
                .pending
                .keys()
                .copied()
                .max_by_key(|&id| newest.wrapping_sub(id))
            else {
                return;
            };
            if let Some(frame) = self.pending.remove(&oldest) {
                self.buffered -= frame.bytes;
                self.stats.lost += 1;
            }
        }
    }

    fn assemble(frame_id: u32, frame: PendingFrame) -> Result<CompletedFrame, FecError> {
        let data_count = usize::from(frame.data_shards);
        let shard_size = usize::from(frame.payload_len);
        let parity_count = usize::from(frame.shard_count - frame.data_shards);
        let mut shards = frame.shards;
        let missing = shards[..data_count].iter().any(Option::is_none);
        if missing {
            let originals = shards[..data_count]
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.as_deref().map(|s| (i, s)));
            let recovery = shards[data_count..]
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.as_deref().map(|s| (i, s)));
            let restored =
                reed_solomon_simd::decode(data_count, parity_count, originals, recovery)?;
            for (index, payload) in restored {
                if let Some(slot) = shards.get_mut(index).filter(|_| index < data_count) {
                    *slot = Some(payload);
                }
            }
        }
        let mut stream = Vec::with_capacity(data_count * shard_size);
        for shard in shards.into_iter().take(data_count) {
            let Some(shard) = shard else {
                return Err(FecError::ReedSolomon("shard not restored".into()));
            };
            stream.extend_from_slice(&shard);
        }
        let prefix: [u8; FRAME_LEN_PREFIX] = stream[..FRAME_LEN_PREFIX]
            .try_into()
            .map_err(|_| FecError::CorruptLength(frame_id))?;
        let len = usize::try_from(u32::from_be_bytes(prefix))
            .map_err(|_| FecError::CorruptLength(frame_id))?;
        if len > stream.len() - FRAME_LEN_PREFIX {
            return Err(FecError::CorruptLength(frame_id));
        }
        stream.drain(..FRAME_LEN_PREFIX);
        stream.truncate(len);
        Ok(CompletedFrame {
            frame_id,
            kind: frame.kind,
            keyframe: frame.keyframe,
            data: stream,
            recovered: missing,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn frame(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| u8::try_from(i % 251).expect("< 251") ^ seed)
            .collect()
    }

    fn encoder(ratio: f32) -> FrameEncoder {
        FrameEncoder::new(MediaKind::Video, ratio).expect("valid ratio")
    }

    #[test]
    fn header_is_16_bytes_and_roundtrips() {
        let header = ShardHeader {
            kind: MediaKind::Audio,
            keyframe: true,
            frame_id: 0xDEAD_BEEF,
            shard_index: 5,
            shard_count: 7,
            data_shards: 4,
            payload_len: 1150,
        };
        let bytes = header.encode();
        assert_eq!(bytes.len(), 16);
        assert_eq!(bytes[0], 1);
        assert_eq!(bytes[1], 1);
        assert_eq!(bytes[2], FLAG_KEYFRAME | FLAG_PARITY);
        assert_eq!(&bytes[4..8], &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(ShardHeader::decode(&bytes), Ok(header));
    }

    #[test]
    fn header_rejects_malformed_input() {
        let good = ShardHeader {
            kind: MediaKind::Video,
            keyframe: false,
            frame_id: 1,
            shard_index: 0,
            shard_count: 2,
            data_shards: 1,
            payload_len: 10,
        }
        .encode();
        assert_eq!(
            ShardHeader::decode(&good[..15]),
            Err(FecError::Truncated(15))
        );
        let mut bad = good;
        bad[0] = 2;
        assert_eq!(ShardHeader::decode(&bad), Err(FecError::Version(2)));
        let mut bad = good;
        bad[1] = 9;
        assert_eq!(ShardHeader::decode(&bad), Err(FecError::Kind(9)));
        let mut bad = good;
        bad[3] = 1;
        assert!(matches!(ShardHeader::decode(&bad), Err(FecError::Flags(_))));
        let mut bad = good;
        bad[2] = FLAG_PARITY; // index 0 is a data shard
        assert!(matches!(ShardHeader::decode(&bad), Err(FecError::Flags(_))));
        let mut bad = good;
        bad[10..12].copy_from_slice(&2000u16.to_be_bytes());
        assert_eq!(ShardHeader::decode(&bad), Err(FecError::Geometry));
        let mut bad = good;
        bad[12..14].copy_from_slice(&3u16.to_be_bytes()); // data > count
        assert_eq!(ShardHeader::decode(&bad), Err(FecError::Geometry));
        let mut bad = good;
        bad[14..16].copy_from_slice(&11u16.to_be_bytes()); // odd size
        assert_eq!(ShardHeader::decode(&bad), Err(FecError::Geometry));
        let mut bad = good;
        bad[14..16].copy_from_slice(&1152u16.to_be_bytes()); // over max
        assert_eq!(ShardHeader::decode(&bad), Err(FecError::Geometry));
    }

    #[test]
    fn shard_from_bytes_checks_payload_length() {
        let shards = encoder(0.0).encode(1, false, b"hello").expect("encode");
        let mut bytes = shards[0].to_bytes();
        assert_eq!(Shard::from_bytes(&bytes).as_ref(), Ok(&shards[0]));
        bytes.push(0);
        assert!(matches!(
            Shard::from_bytes(&bytes),
            Err(FecError::PayloadLen { .. })
        ));
    }

    #[test]
    fn parity_ratio_validation_and_counts() {
        assert!(FrameEncoder::new(MediaKind::Video, 0.6).is_err());
        assert!(FrameEncoder::new(MediaKind::Video, -0.1).is_err());
        assert!(FrameEncoder::new(MediaKind::Video, f32::NAN).is_err());
        assert_eq!(encoder(0.0).parity_count(10), 0);
        assert_eq!(encoder(0.01).parity_count(1), 1);
        assert_eq!(encoder(0.2).parity_count(10), 2);
        assert_eq!(encoder(0.25).parity_count(10), 3);
        assert_eq!(encoder(0.5).parity_count(1024), 512);
    }

    #[test]
    fn shards_respect_size_limit_and_padding_is_small() {
        let enc = encoder(0.3);
        for len in [0, 1, 1145, 1146, 1147, 5000, 100_000] {
            let shards = enc.encode(7, true, &frame(len, 3)).expect("encode");
            let first = shards[0].header;
            let data = usize::from(first.data_shards);
            assert_eq!(data, (len + 4).div_ceil(MAX_SHARD_PAYLOAD), "len {len}");
            assert_eq!(usize::from(first.shard_count), shards.len());
            assert!(usize::from(first.payload_len) <= MAX_SHARD_PAYLOAD);
            assert!(data * usize::from(first.payload_len) - (len + 4) < data + 2);
            for (i, shard) in shards.iter().enumerate() {
                assert_eq!(usize::from(shard.header.shard_index), i);
                assert_eq!(shard.payload.len(), usize::from(first.payload_len));
                assert_eq!(shard.header.is_parity(), i >= data);
                assert!(
                    shard.to_bytes().len() <= 1200 - 34,
                    "fits a 1200-byte datagram"
                );
            }
        }
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let max_data = usize::from(MAX_SHARD_COUNT) * MAX_SHARD_PAYLOAD - FRAME_LEN_PREFIX;
        assert!(encoder(0.0).encode(0, true, &vec![0; max_data]).is_ok());
        assert!(matches!(
            encoder(0.0).encode(0, true, &vec![0; max_data + 1]),
            Err(FecError::FrameTooLarge(_))
        ));
        // With parity the data budget shrinks.
        assert!(matches!(
            encoder(0.1).encode(0, true, &vec![0; max_data]),
            Err(FecError::FrameTooLarge(_))
        ));
    }

    #[test]
    fn in_order_delivery_without_loss() {
        let data = frame(4000, 1);
        let shards = encoder(0.2).encode(42, true, &data).expect("encode");
        let mut rx = FrameReassembler::default();
        let mut out = None;
        for shard in &shards {
            if let Some(done) = rx.push(&shard.to_bytes()).expect("valid") {
                out = Some(done);
            }
        }
        let done = out.expect("frame completes");
        assert_eq!(done.data, data);
        assert_eq!(done.frame_id, 42);
        assert!(done.keyframe);
        assert!(!done.recovered);
        let stats = rx.stats();
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.late_shards, 1, "the parity after completion is late");
        assert_eq!(rx.pending_frames(), 0);
        assert_eq!(rx.buffered_bytes(), 0);
    }

    #[test]
    fn recovers_lost_data_shards_from_parity() {
        let data = frame(10_000, 9);
        let shards = encoder(0.5).encode(3, false, &data).expect("encode");
        let data_shards = usize::from(shards[0].header.data_shards);
        let parity = shards.len() - data_shards;
        let mut rx = FrameReassembler::default();
        // Drop the first `parity` data shards, deliver the rest in reverse order.
        let mut done = None;
        for shard in shards.iter().skip(parity).rev() {
            if let Some(frame) = rx.push(&shard.to_bytes()).expect("valid") {
                done = Some(frame);
            }
        }
        let done = done.expect("recovered");
        assert!(done.recovered);
        assert_eq!(done.data, data);
        assert_eq!(rx.stats().recovered, 1);
    }

    #[test]
    fn duplicates_are_counted_and_ignored() {
        let shards = encoder(0.2)
            .encode(1, false, &frame(3000, 0))
            .expect("encode");
        let mut rx = FrameReassembler::default();
        let bytes = shards[0].to_bytes();
        assert_eq!(rx.push(&bytes), Ok(None));
        assert_eq!(rx.push(&bytes), Ok(None));
        assert_eq!(rx.stats().duplicate_shards, 1);
    }

    #[test]
    fn too_many_losses_count_as_lost_when_window_passes() {
        let enc = encoder(0.1);
        let mut rx = FrameReassembler::new(ReassemblerConfig {
            window: 4,
            ..ReassemblerConfig::default()
        });
        let first = enc.encode(0, false, &frame(5000, 0)).expect("encode");
        rx.push(&first[0].to_bytes()).expect("valid");
        for id in 1..=5 {
            for shard in enc.encode(id, false, &frame(100, 1)).expect("encode") {
                rx.push(&shard.to_bytes()).expect("valid");
            }
        }
        let stats = rx.stats();
        assert_eq!(stats.lost, 1);
        assert_eq!(stats.completed, 5);
        assert_eq!(rx.pending_frames(), 0);
        // A straggler for frame 0 is now late, not a new frame.
        assert_eq!(rx.push(&first[1].to_bytes()), Ok(None));
        assert_eq!(rx.stats().late_shards, 1 + 5);
        assert_eq!(rx.pending_frames(), 0);
    }

    #[test]
    fn inconsistent_shards_are_rejected() {
        let mut rx = FrameReassembler::default();
        let a = encoder(0.2)
            .encode(9, false, &frame(3000, 0))
            .expect("encode");
        let b = encoder(0.2)
            .encode(9, false, &frame(6000, 0))
            .expect("encode");
        rx.push(&a[0].to_bytes()).expect("valid");
        assert_eq!(rx.push(&b[1].to_bytes()), Err(FecError::Inconsistent(9)));
        assert_eq!(rx.stats().invalid_shards, 1);
    }

    #[test]
    fn byte_cap_bounds_memory() {
        let cap = 20_000;
        let mut rx = FrameReassembler::new(ReassemblerConfig {
            window: 64,
            max_buffered_bytes: cap,
        });
        let enc = encoder(0.0);
        // Malicious sender: one shard of many large frames, never completing any.
        for id in 0..60 {
            let shards = enc.encode(id, false, &frame(20_000, 0)).expect("encode");
            rx.push(&shards[0].to_bytes()).expect("valid");
            assert!(rx.buffered_bytes() <= cap);
        }
        assert!(rx.stats().lost > 0);
    }

    #[test]
    fn frame_ids_wrap_around() {
        let enc = encoder(0.0);
        let mut rx = FrameReassembler::new(ReassemblerConfig {
            window: 4,
            ..ReassemblerConfig::default()
        });
        for id in [u32::MAX - 1, u32::MAX, 0, 1] {
            let shards = enc.encode(id, false, b"x").expect("encode");
            let done = rx
                .push(&shards[0].to_bytes())
                .expect("valid")
                .expect("single shard frame");
            assert_eq!(done.frame_id, id);
        }
        assert_eq!(rx.stats().lost, 0);
        assert_eq!(rx.stats().late_shards, 0);
    }

    #[test]
    fn corrupt_length_prefix_is_rejected() {
        let mut shards = encoder(0.0).encode(5, false, b"abcd").expect("encode");
        shards[0].payload[..4].copy_from_slice(&u32::MAX.to_be_bytes());
        let mut rx = FrameReassembler::default();
        assert_eq!(
            rx.push_shard(shards.remove(0)),
            Err(FecError::CorruptLength(5))
        );
    }

    #[test]
    fn random_garbage_never_panics() {
        let mut rx = FrameReassembler::default();
        let mut x: u32 = 0x1234_5678;
        for len in 0..2000 {
            let bytes: Vec<u8> = (0..len % 200)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    x.to_le_bytes()[0]
                })
                .collect();
            let _ = rx.push(&bytes);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Any delivery order with any loss pattern that leaves ≥ data_shards
        /// shards yields the original frame exactly once.
        #[test]
        fn any_order_any_loss_with_enough_shards(
            len in 0usize..20_000,
            ratio in 0.0f32..=0.5,
            seed in any::<u64>(),
            dup in any::<bool>(),
        ) {
            let data = frame(len, u8::try_from(seed % 256).expect("< 256"));
            let shards = encoder(ratio).encode(77, true, &data).expect("encode");
            let data_shards = usize::from(shards[0].header.data_shards);
            // Deterministic shuffle + drop of (count - data) shards.
            let mut order: Vec<usize> = (0..shards.len()).collect();
            let mut s = seed | 1;
            for i in (1..order.len()).rev() {
                s ^= s << 13; s ^= s >> 7; s ^= s << 17;
                let j = usize::try_from(s % (u64::try_from(i).expect("fits") + 1)).expect("fits");
                order.swap(i, j);
            }
            order.truncate(data_shards);
            let mut rx = FrameReassembler::default();
            let mut done = Vec::new();
            for &i in &order {
                let bytes = shards[i].to_bytes();
                if let Some(frame) = rx.push(&bytes).expect("valid") { done.push(frame); }
                if dup { prop_assert_eq!(rx.push(&bytes).expect("valid"), None); }
            }
            prop_assert_eq!(done.len(), 1);
            prop_assert_eq!(&done[0].data, &data);
            prop_assert_eq!(rx.buffered_bytes(), 0);
        }
    }
}
