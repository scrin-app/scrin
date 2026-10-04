//! wasm-bindgen bindings of the scrin core for the browser client.
//!
//! Exposes the controller side of SPAKE2 pairing, the inner end-to-end
//! channel and the FEC media reassembler. The browser's device key stays a
//! non-extractable `WebCrypto` key; only its 32-byte public key enters here.
//! Contract: `docs/protocol/gateway-session.md`.

pub mod core;

use wasm_bindgen::prelude::*;

use crate::core::{ControllerPaired, ControllerPairing, CoreError, Lanes, MediaReassembler};
use scrin_media::fec::{MediaKind, ReassemblyStats};

fn js(e: &CoreError) -> JsError {
    JsError::new(&e.to_string())
}

/// Bytes a side signs in the gateway `Attest` message.
#[wasm_bindgen(js_name = attestMessage)]
pub fn attest_message(
    signer_is_host: bool,
    host: &[u8],
    controller: &[u8],
    controller_tag: &[u8],
    host_tag: &[u8],
) -> Result<Vec<u8>, JsError> {
    core::attest_message(signer_is_host, host, controller, controller_tag, host_tag)
        .map_err(|e| js(&e))
}

/// Verifies an Ed25519 signature by a 32-byte public key (the host's `Attest`).
#[wasm_bindgen(js_name = verifySignature)]
#[must_use]
pub fn verify_signature(public_key: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    core::verify_signature(public_key, msg, sig)
}

/// Ed25519 public key of a 32-byte seed (fallback identity).
#[wasm_bindgen(js_name = seedPublicKey)]
pub fn seed_public_key(seed: &[u8]) -> Result<Vec<u8>, JsError> {
    core::seed_public_key(seed)
        .map(|k| k.to_vec())
        .map_err(|e| js(&e))
}

/// Ed25519 signature with a 32-byte seed (fallback identity).
#[wasm_bindgen(js_name = seedSign)]
pub fn seed_sign(seed: &[u8], msg: &[u8]) -> Result<Vec<u8>, JsError> {
    core::seed_sign(seed, msg)
        .map(|s| s.to_vec())
        .map_err(|e| js(&e))
}

/// Controller side of quick-connect pairing.
#[wasm_bindgen]
#[derive(Debug)]
pub struct Pairing {
    inner: ControllerPairing,
}

#[wasm_bindgen]
impl Pairing {
    /// `entropy`: 32 bytes from `crypto.getRandomValues`, fresh per attempt.
    #[wasm_bindgen(constructor)]
    pub fn new(code: &str, me: &[u8], host: &[u8], entropy: &[u8]) -> Result<Pairing, JsError> {
        ControllerPairing::start(code, me, host, entropy)
            .map(|inner| Self { inner })
            .map_err(|e| js(&e))
    }

    /// SPAKE2 message for `PairStart`.
    #[must_use]
    pub fn message(&self) -> Vec<u8> {
        self.inner.message().to_vec()
    }

    /// Finishes with the host's `PairStart`; usable once.
    pub fn finish(&mut self, peer_msg: &[u8]) -> Result<Paired, JsError> {
        self.inner
            .finish(peer_msg)
            .map(|inner| Paired { inner })
            .map_err(|e| js(&e))
    }
}

/// Pairing result awaiting confirmation.
#[wasm_bindgen]
#[derive(Debug)]
pub struct Paired {
    inner: ControllerPaired,
}

#[wasm_bindgen]
impl Paired {
    /// Tag to send in `PairConfirm`.
    #[must_use]
    pub fn confirmation(&self) -> Vec<u8> {
        self.inner.confirmation().to_vec()
    }

    /// Constant-time check of the host's `PairConfirm` tag.
    #[wasm_bindgen(js_name = verifyPeer)]
    #[must_use]
    pub fn verify_peer(&self, tag: &[u8]) -> bool {
        self.inner.verify_peer(tag)
    }

    /// Five emoji indices (0..64) of the short authentication string.
    #[must_use]
    pub fn sas(&self) -> Vec<u8> {
        self.inner.sas().to_vec()
    }

    /// The inner channel (`export("scrin gateway channel v1")`, controller side).
    #[must_use]
    pub fn channel(&self) -> Channel {
        Channel {
            inner: self.inner.channel(),
        }
    }
}

/// Inner end-to-end channel: per-lane ChaCha20-Poly1305.
#[wasm_bindgen]
#[derive(Debug)]
pub struct Channel {
    inner: Lanes,
}

#[wasm_bindgen]
impl Channel {
    /// Seals one stream frame body or datagram on `lane`.
    pub fn seal(&mut self, lane: u32, plaintext: &[u8]) -> Result<Vec<u8>, JsError> {
        self.inner.seal(lane, plaintext).map_err(|e| js(&e))
    }

    /// Opens one frame body or datagram; throws on tamper, replay or reorder.
    pub fn open(&mut self, lane: u32, sealed: &[u8]) -> Result<Vec<u8>, JsError> {
        self.inner.open(lane, sealed).map_err(|e| js(&e))
    }

    /// Like `open` but returns `undefined` instead of throwing (datagram hot path).
    #[wasm_bindgen(js_name = tryOpen)]
    #[must_use]
    pub fn try_open(&mut self, lane: u32, sealed: &[u8]) -> Option<Vec<u8>> {
        self.inner.open(lane, sealed).ok()
    }
}

/// One reassembled media frame: an H.264 Annex B access unit, or one Opus
/// packet when `audio` is true.
#[wasm_bindgen]
#[derive(Debug)]
pub struct VideoFrame {
    frame_id: u32,
    keyframe: bool,
    recovered: bool,
    audio: bool,
    data: Vec<u8>,
}

#[wasm_bindgen]
impl VideoFrame {
    #[wasm_bindgen(getter, js_name = frameId)]
    #[must_use]
    pub fn frame_id(&self) -> u32 {
        self.frame_id
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn keyframe(&self) -> bool {
        self.keyframe
    }

    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn recovered(&self) -> bool {
        self.recovered
    }

    /// The frame is an Opus packet (media kind 1), not video.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn audio(&self) -> bool {
        self.audio
    }

    /// Moves the bytes out (the frame is empty afterwards).
    #[wasm_bindgen(js_name = takeData)]
    pub fn take_data(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.data)
    }
}

/// FEC reassembler for video and audio shards (`scrin_media::fec`).
#[wasm_bindgen]
#[derive(Debug, Default)]
pub struct Reassembler {
    inner: MediaReassembler,
}

fn stats_vec(s: ReassemblyStats) -> Vec<f64> {
    #[expect(clippy::cast_precision_loss)] // counters stay far below 2^53
    let v = [
        s.completed,
        s.recovered,
        s.lost,
        s.late_shards,
        s.duplicate_shards,
        s.invalid_shards,
    ]
    .map(|n| n as f64);
    v.to_vec()
}

#[wasm_bindgen]
impl Reassembler {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Reassembler {
        Self::default()
    }

    /// Feeds one opened shard datagram; returns a frame when complete.
    pub fn push(&mut self, datagram: &[u8]) -> Option<VideoFrame> {
        self.inner.push(datagram).map(|f| VideoFrame {
            frame_id: f.frame_id,
            keyframe: f.keyframe,
            recovered: f.recovered,
            audio: f.kind == MediaKind::Audio,
            data: f.data,
        })
    }

    /// Video `[completed, recovered, lost, late, duplicate, invalid]` counters.
    #[must_use]
    pub fn stats(&self) -> Vec<f64> {
        stats_vec(self.inner.stats())
    }

    /// Audio counters, same layout as `stats`.
    #[wasm_bindgen(js_name = audioStats)]
    #[must_use]
    pub fn audio_stats(&self) -> Vec<f64> {
        stats_vec(self.inner.audio_stats())
    }
}
