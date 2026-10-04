//! Wire types (prost) and the hand-packed media header for scrin.
//!
//! - [`v1`]: control-plane messages generated from `proto/scrin/v1/*.proto`;
//!   every message on a reliable stream is one length-delimited [`v1::Envelope`].
//! - [`media`]: the fixed 16-byte header in front of every media datagram.

pub mod media;

/// Generated protobuf types of package `scrin.v1`.
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_debug_implementations,
    missing_docs
)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/scrin.v1.rs"));
}

use prost::Message;

/// Protocol version spoken by this build (`Hello.protocol_version`).
pub const PROTOCOL_VERSION: u32 = 1;
/// Oldest protocol version this build accepts.
pub const MIN_SUPPORTED_VERSION: u32 = 1;

/// Failure to decode a control message.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// Bytes are not a valid protobuf `Envelope`.
    #[error("invalid envelope: {0}")]
    Protobuf(#[from] prost::DecodeError),
    /// Envelope decoded but carried no (or an unknown) payload.
    #[error("envelope has no known payload")]
    EmptyPayload,
}

/// Serialises an envelope to protobuf bytes (no length prefix).
pub fn encode_envelope(envelope: &v1::Envelope) -> Vec<u8> {
    envelope.encode_to_vec()
}

/// Parses protobuf bytes (no length prefix) into an envelope with a known payload.
pub fn decode_envelope(bytes: &[u8]) -> Result<v1::Envelope, DecodeError> {
    let envelope = v1::Envelope::decode(bytes)?;
    if envelope.payload.is_none() {
        return Err(DecodeError::EmptyPayload);
    }
    Ok(envelope)
}

/// Wraps a payload into an envelope.
pub fn envelope(payload: v1::envelope::Payload) -> v1::Envelope {
    v1::Envelope {
        payload: Some(payload),
    }
}

#[cfg(test)]
mod tests {
    use super::v1::envelope::Payload;
    use super::v1::{self, mouse_move::Motion};
    use super::*;

    fn round_trip(payload: Payload) {
        let env = envelope(payload);
        let bytes = encode_envelope(&env);
        assert_eq!(decode_envelope(&bytes).expect("decode"), env);
    }

    #[test]
    fn hello_round_trips() {
        round_trip(Payload::Hello(v1::Hello {
            protocol_version: PROTOCOL_VERSION,
            min_supported_version: MIN_SUPPORTED_VERSION,
            app_version: "0.1.0".into(),
            platform: v1::Platform::Windows.into(),
            device_name: "desk".into(),
            capabilities: Some(v1::Capabilities {
                decoders: vec![v1::CodecCapability {
                    codec: v1::Codec::Av1.into(),
                    max_width: 3840,
                    max_height: 2160,
                    max_fps: 120,
                    supports_444: true,
                    supports_hdr: false,
                    hardware: true,
                }],
                clipboard_formats: vec!["text/plain".into(), "image/png".into()],
                file_transfer: true,
                multi_monitor: true,
                ..Default::default()
            }),
        }));
    }

    #[test]
    fn session_messages_round_trip() {
        round_trip(Payload::SessionRequest(v1::SessionRequest {
            requested: vec![v1::Permission::View.into(), v1::Permission::Input.into()],
            controller_name: "alice".into(),
            unattended: false,
        }));
        round_trip(Payload::SessionAccept(v1::SessionAccept {
            granted: vec![v1::Permission::View.into()],
            displays: vec![v1::DisplayInfo {
                id: 1,
                width: 2560,
                height: 1440,
                refresh_mhz: 144_000,
                scale: 1.25,
                primary: true,
                ..Default::default()
            }],
            max_duration_s: 3600,
        }));
        round_trip(Payload::SessionReject(v1::SessionReject {
            reason: v1::SessionRejectReason::DeclinedByUser.into(),
            message: String::new(),
        }));
        round_trip(Payload::SessionEnd(v1::SessionEnd {
            reason: v1::SessionEndReason::ClosedByHost.into(),
            message: "bye".into(),
        }));
    }

    #[test]
    fn input_and_media_round_trip() {
        round_trip(Payload::KeyEvent(v1::KeyEvent {
            hid_usage: 0x04,
            down: true,
            modifiers: 1,
            text: Some("A".into()),
            repeat: false,
        }));
        round_trip(Payload::MouseMove(v1::MouseMove {
            motion: Some(Motion::Relative(v1::RelativeMotion { dx: -3, dy: 7 })),
        }));
        round_trip(Payload::Pong(v1::Pong {
            seq: 9,
            t1_us: 1,
            t2_us: 2,
            t3_us: 3,
        }));
        round_trip(Payload::FileOffer(v1::FileOffer {
            id: 42,
            name: "a.txt".into(),
            size: 1024,
            blake3: vec![7; 32],
            modified_unix_ms: 0,
        }));
    }

    #[test]
    fn empty_envelope_is_rejected() {
        assert!(matches!(
            decode_envelope(&[]),
            Err(DecodeError::EmptyPayload)
        ));
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(matches!(
            decode_envelope(&[0xFF, 0xFF, 0xFF]),
            Err(DecodeError::Protobuf(_))
        ));
    }
}
