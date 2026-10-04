//! Deterministic test vectors for `docs/protocol/gateway-session.md`.
//!
//! `cargo test -p scrin-wasm --test vectors` recomputes every value and
//! compares it with `testvectors/gateway-session.json`. Set
//! `SCRIN_UPDATE_VECTORS=1` to rewrite the file after an intentional change.
//! The browser side replays the same file (`packages/protocol/src/wasm.test.ts`).

use std::path::PathBuf;

use data_encoding::HEXLOWER;
use scrin_crypto::channel::Side;
use scrin_crypto::identity::Identity;
use scrin_crypto::pake::{Pairing, Role};
use scrin_media::fec::{FrameEncoder, MediaKind};
use scrin_proto::v1::{self, envelope::Payload};
use scrin_wasm::core::{
    CHANNEL_CONTEXT, CONTROL_LANE, ControllerPairing, DATAGRAM_LANE, Lanes, attest_message,
    stream_lane,
};
use serde_json::{Value, json};

const CODE: &str = "K7QX-M2PA";
const HOST_SEED: [u8; 32] = [0x11; 32];
const CONTROLLER_SEED: [u8; 32] = [0x22; 32];
const HOST_ENTROPY: [u8; 32] = [0x33; 32];
const CONTROLLER_ENTROPY: [u8; 32] = [0x44; 32];

fn hex(b: &[u8]) -> String {
    HEXLOWER.encode(b)
}

fn msg(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend_from_slice(body);
    v
}

fn frame(body: &[u8]) -> Vec<u8> {
    let len = u32::try_from(body.len()).expect("small");
    let mut v = len.to_be_bytes().to_vec();
    v.extend_from_slice(body);
    v
}

fn env(p: Payload) -> Vec<u8> {
    scrin_proto::encode_envelope(&scrin_proto::envelope(p))
}

/// A tiny H.264 Annex B keyframe: SPS (High, level 3.1), PPS, IDR slice.
fn keyframe_au() -> Vec<u8> {
    let mut au = vec![0, 0, 0, 1, 0x67, 0x64, 0x00, 0x1f, 0xac, 0xd9, 0x40, 0x50];
    au.extend_from_slice(&[0, 0, 0, 1, 0x68, 0xeb, 0xe3, 0xcb, 0x22, 0xc0]);
    au.extend_from_slice(&[0, 0, 1, 0x65, 0x88, 0x84]);
    au.extend((0..2600u32).map(|i| u8::try_from(i % 251).expect("< 256")));
    au
}

#[expect(clippy::too_many_lines)] // a flat table of sample messages
fn protobuf_samples() -> Vec<(&'static str, Vec<u8>)> {
    use v1::mouse_move::Motion;
    vec![
        (
            "sessionRequest",
            env(Payload::SessionRequest(v1::SessionRequest {
                requested: vec![1, 2, 3, 14],
                controller_name: "Browser".into(),
                unattended: false,
            })),
        ),
        (
            "keyEventCtrlA",
            env(Payload::KeyEvent(v1::KeyEvent {
                hid_usage: 0x04,
                down: true,
                modifiers: 2,
                text: None,
                repeat: false,
            })),
        ),
        (
            "keyEventRepeatUp",
            env(Payload::KeyEvent(v1::KeyEvent {
                hid_usage: 0x000C_00E9,
                down: false,
                modifiers: 0,
                text: None,
                repeat: true,
            })),
        ),
        (
            "mouseMoveAbsolute",
            env(Payload::MouseMove(v1::MouseMove {
                motion: Some(Motion::Absolute(v1::AbsolutePosition {
                    display_id: 0,
                    x: 0.5,
                    y: 0.25,
                })),
            })),
        ),
        (
            "mouseMoveRelative",
            env(Payload::MouseMove(v1::MouseMove {
                motion: Some(Motion::Relative(v1::RelativeMotion { dx: -3, dy: 7 })),
            })),
        ),
        (
            "mouseButtonRightDown",
            env(Payload::MouseButton(v1::MouseButton {
                button: v1::MouseButtonKind::Right.into(),
                down: true,
            })),
        ),
        (
            "mouseWheel",
            env(Payload::MouseWheel(v1::MouseWheel {
                delta_x: 30,
                delta_y: -120,
            })),
        ),
        (
            "keyframeRequest",
            env(Payload::KeyframeRequest(v1::KeyframeRequest {
                stream_id: 0,
                last_good_frame_id: 41,
            })),
        ),
        (
            "bitrateFeedback",
            env(Payload::BitrateFeedback(v1::BitrateFeedback {
                stream_id: 0,
                base_receive_us: 5_000_000_123,
                arrivals: vec![
                    v1::DatagramArrival {
                        frame_id: 7,
                        shard_index: 0,
                        receive_delta_us: 0,
                        size_bytes: 1166,
                    },
                    v1::DatagramArrival {
                        frame_id: 7,
                        shard_index: 1,
                        receive_delta_us: 250,
                        size_bytes: 1166,
                    },
                ],
                datagrams_received: 2,
                datagrams_lost: 1,
                shards_recovered_by_fec: 0,
                frames_dropped: 0,
                estimated_bps: 0,
            })),
        ),
        (
            "ping",
            env(Payload::Ping(v1::Ping {
                seq: 3,
                t1_us: 1_234_567,
            })),
        ),
        (
            "sessionEndByController",
            env(Payload::SessionEnd(v1::SessionEnd {
                reason: v1::SessionEndReason::ClosedByController.into(),
                message: String::new(),
            })),
        ),
        (
            "chat",
            env(Payload::ChatMessage(v1::ChatMessage {
                id: 1,
                text: "salut, ăîșț".into(),
                sent_unix_ms: 1_791_000_000_000,
            })),
        ),
        (
            "sessionAccept",
            env(Payload::SessionAccept(v1::SessionAccept {
                granted: vec![1, 2, 14],
                displays: vec![v1::DisplayInfo {
                    id: 0,
                    name: "DELL U3423WE".into(),
                    x: -1920,
                    y: 0,
                    width: 3440,
                    height: 1440,
                    refresh_mhz: 59_940,
                    scale: 1.25,
                    primary: true,
                    hdr: false,
                }],
                max_duration_s: 3600,
            })),
        ),
        (
            "sessionReject",
            env(Payload::SessionReject(v1::SessionReject {
                reason: v1::SessionRejectReason::Busy.into(),
                message: "busy".into(),
            })),
        ),
        (
            "permissionsUpdate",
            env(Payload::PermissionsUpdate(v1::PermissionsUpdate {
                granted: vec![1],
            })),
        ),
        (
            "videoConfig",
            env(Payload::VideoConfig(v1::VideoConfig {
                stream_id: 0,
                codec: v1::Codec::H264.into(),
                width: 1920,
                height: 1080,
                fps: 60,
                bitrate_bps: 8_000_000,
                chroma: v1::ChromaSubsampling::ChromaSubsampling420.into(),
                hdr: false,
                display_id: 0,
                codec_config: Vec::new(),
            })),
        ),
        (
            "pong",
            env(Payload::Pong(v1::Pong {
                seq: 3,
                t1_us: 1_234_567,
                t2_us: 9_000_000_000,
                t3_us: 9_000_000_040,
            })),
        ),
        (
            "sessionEndTimeLimit",
            env(Payload::SessionEnd(v1::SessionEnd {
                reason: v1::SessionEndReason::TimeLimit.into(),
                message: "60 min".into(),
            })),
        ),
    ]
}

#[expect(clippy::too_many_lines)] // one linear script of the whole session
fn compute() -> Value {
    let host = Identity::from_seed(HOST_SEED);
    let ctl = Identity::from_seed(CONTROLLER_SEED);
    let (h, c) = (host.device_id(), ctl.device_id());

    // SPAKE2: the browser path (scrin-wasm core) and a deterministic host.
    let mut cp = ControllerPairing::start(CODE, &c.0, &h.0, &CONTROLLER_ENTROPY).expect("start");
    let (hp, mh) = Pairing::start_with_entropy("K7QXM2PA", Role::Host, h, c, &HOST_ENTROPY);
    let mc = cp.message().to_vec();
    let kh = hp.finish(&mc).expect("host finish");
    let kc = cp.finish(&mh).expect("controller finish");
    let (tag_c, tag_h) = (kc.confirmation(), kh.confirmation());
    assert!(kc.verify_peer(&tag_h));
    kh.verify_peer(&tag_c).expect("controller tag");
    assert_eq!(kc.sas(), kh.sas().0);

    let att_c = attest_message(false, &h.0, &c.0, &tag_c, &tag_h).expect("attest");
    let att_h = attest_message(true, &h.0, &c.0, &tag_c, &tag_h).expect("attest");
    let (sig_c, sig_h) = (ctl.sign(&att_c), host.sign(&att_h));
    c.verify(&att_c, &sig_c).expect("sig c");
    h.verify(&att_h, &sig_h).expect("sig h");

    let hello = [1u8, 0, 1, 0, 1, 0];
    let c_msgs = [
        hello.to_vec(),
        msg(7, &c.0),
        msg(2, &mc),
        msg(3, &tag_c),
        msg(8, &sig_c),
        msg(4, &[0]),
    ];
    let h_msgs = [
        hello.to_vec(),
        msg(7, &h.0),
        msg(2, &mh),
        msg(3, &tag_h),
        msg(8, &sig_h),
    ];
    let mut c_stream = vec![0u8, 0, 0];
    for m in &c_msgs {
        c_stream.extend(frame(m));
    }
    let h_stream: Vec<u8> = h_msgs.iter().flat_map(|m| frame(m)).collect();

    // Inner channel.
    let secret = kh.export(CHANNEL_CONTEXT);
    let mut hl = Lanes::new(&secret, Side::Host);
    let mut cl = kc.channel();
    let samples = protobuf_samples();
    let get = |n: &str| {
        samples
            .iter()
            .find(|(k, _)| *k == n)
            .map(|(_, v)| v.clone())
            .expect("sample")
    };
    let input_lane = stream_lane(false, 1, 0);
    let c_control: Vec<Vec<u8>> = ["sessionRequest", "ping"]
        .iter()
        .map(|n| cl.seal(CONTROL_LANE, &get(n)).expect("seal"))
        .collect();
    let h_control: Vec<Vec<u8>> = ["sessionAccept", "videoConfig", "pong"]
        .iter()
        .map(|n| hl.seal(CONTROL_LANE, &get(n)).expect("seal"))
        .collect();
    let c_input: Vec<Vec<u8>> = ["keyEventCtrlA", "mouseMoveAbsolute", "mouseWheel"]
        .iter()
        .map(|n| cl.seal(input_lane, &get(n)).expect("seal"))
        .collect();
    for (s, n) in c_control.iter().zip(["sessionRequest", "ping"]) {
        assert_eq!(hl.open(CONTROL_LANE, s).expect("open"), get(n));
    }

    let au = keyframe_au();
    let shards = FrameEncoder::new(MediaKind::Video, 0.5)
        .expect("fec")
        .encode(42, true, &au)
        .expect("shard");
    let plain: Vec<Vec<u8>> = shards
        .iter()
        .map(scrin_media::fec::Shard::to_bytes)
        .collect();
    let sealed: Vec<Vec<u8>> = plain
        .iter()
        .map(|p| hl.seal(DATAGRAM_LANE, p).expect("seal"))
        .collect();
    // Deliver in reverse and drop shard 0 so the browser must use parity.
    let delivery: Vec<String> = sealed.iter().skip(1).rev().map(|d| hex(d)).collect();

    let hexes = |v: &[Vec<u8>]| v.iter().map(|b| hex(b)).collect::<Vec<_>>();
    json!({
        "description": "Deterministic vectors for docs/protocol/gateway-session.md. Regenerate: SCRIN_UPDATE_VECTORS=1 cargo test -p scrin-wasm --test vectors",
        "version": 1,
        "inputs": {
            "code": CODE,
            "hostSeed": hex(&HOST_SEED),
            "controllerSeed": hex(&CONTROLLER_SEED),
            "hostEntropy": hex(&HOST_ENTROPY),
            "controllerEntropy": hex(&CONTROLLER_ENTROPY),
        },
        "ids": { "host": hex(&h.0), "controller": hex(&c.0) },
        "pairing": {
            "controllerMsg": hex(&mc),
            "hostMsg": hex(&mh),
            "controllerTag": hex(&tag_c),
            "hostTag": hex(&tag_h),
            "sas": kc.sas().to_vec(),
            "channelSecret": hex(secret.as_slice()),
            "controllerAttestMessage": hex(&att_c),
            "hostAttestMessage": hex(&att_h),
            "controllerSignature": hex(&sig_c),
            "hostSignature": hex(&sig_h),
        },
        "handshake": {
            "controllerMessages": hexes(&c_msgs),
            "hostMessages": hexes(&h_msgs),
            "controllerControlBytes": hex(&c_stream),
            "hostControlBytes": hex(&h_stream),
        },
        "channel": {
            "inputLane": input_lane,
            "datagramLane": DATAGRAM_LANE,
            "controllerControl": { "plain": ["sessionRequest", "ping"], "sealed": hexes(&c_control) },
            "hostControl": { "plain": ["sessionAccept", "videoConfig", "pong"], "sealed": hexes(&h_control) },
            "controllerInput": { "plain": ["keyEventCtrlA", "mouseMoveAbsolute", "mouseWheel"], "sealed": hexes(&c_input) },
        },
        "video": {
            "frameId": 42,
            "keyframe": true,
            "accessUnit": hex(&au),
            "codecString": "avc1.64001f",
            "plainShards": hexes(&plain),
            "sealedDatagramsDelivered": delivery,
        },
        "protobuf": samples.iter().map(|(k, v)| ((*k).to_owned(), Value::String(hex(v)))).collect::<serde_json::Map<_, _>>(),
    })
}

fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testvectors/gateway-session.json")
}

#[test]
fn gateway_session_vectors_match() {
    let value = compute();
    let text = serde_json::to_string_pretty(&value).expect("json") + "\n";
    if std::env::var_os("SCRIN_UPDATE_VECTORS").is_some() {
        std::fs::create_dir_all(path().parent().expect("dir")).expect("mkdir");
        std::fs::write(path(), text).expect("write vectors");
        return;
    }
    let on_disk = std::fs::read_to_string(path())
        .expect("testvectors/gateway-session.json missing: run with SCRIN_UPDATE_VECTORS=1");
    let stored: Value = serde_json::from_str(&on_disk).expect("vector json");
    assert_eq!(stored, value, "vectors drifted from the implementation");
}

#[test]
fn vectors_are_deterministic() {
    assert_eq!(compute(), compute());
}
