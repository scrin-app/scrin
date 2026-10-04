//! Conversions between FFI types and `scrin.v1` envelopes on the Control stream.

use scrin_net::framing::{StreamKind, accept_stream, open_stream, read_frame, write_frame};
use scrin_net::{Connection, RecvStream, SendStream};
use scrin_proto::v1::{self, envelope::Payload};
use scrin_session::{EndReason, Permission, Permissions, RejectReason};
use tokio::sync::mpsc;

use crate::types::{MouseButtonKind, RemoteInput, TouchPhase, VideoCodec, VideoConfigInfo};

/// Largest frame accepted on an Input stream (the engine's cap).
const MAX_INPUT_FRAME: usize = 64 * 1024;

/// Writes one envelope as a length-prefixed frame.
pub(crate) async fn send(w: &mut SendStream, payload: Payload) -> scrin_net::Result<()> {
    let bytes = scrin_proto::encode_envelope(&scrin_proto::envelope(payload));
    write_frame(w, &bytes).await
}

/// Reads envelopes until the stream ends; `None` is sent once at the end.
pub(crate) fn spawn_reader(
    mut recv: RecvStream,
    tx: mpsc::UnboundedSender<Option<Payload>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Ok(Some(bytes)) = read_frame(&mut recv).await else {
                let _ = tx.send(None);
                return;
            };
            // Unknown or empty payloads come from newer peers: skip them.
            if let Ok(env) = scrin_proto::decode_envelope(&bytes)
                && let Some(p) = env.payload
                && tx.send(Some(p)).is_err()
            {
                return;
            }
        }
    })
}

/// Host: accepts the controller's `Input` streams and forwards their input
/// envelopes into the session's payload channel. Never sends `None` (that
/// would read as the Control stream ending).
pub(crate) async fn accept_input_streams(
    conn: Connection,
    tx: mpsc::UnboundedSender<Option<Payload>>,
) {
    while let Ok((kind, _send, mut recv)) = accept_stream(&conn).await {
        if kind != StreamKind::Input {
            continue;
        }
        let tx = tx.clone();
        tokio::spawn(async move {
            while let Ok(Some(bytes)) =
                scrin_net::framing::read_frame_capped(&mut recv, MAX_INPUT_FRAME).await
            {
                if let Ok(env) = scrin_proto::decode_envelope(&bytes)
                    && let Some(p) = env.payload
                    && is_input(&p)
                    && tx.send(Some(p)).is_err()
                {
                    return;
                }
            }
        });
    }
}

/// Controller: opens one `Input` stream and writes every queued input envelope.
pub(crate) async fn input_writer(conn: Connection, mut rx: mpsc::UnboundedReceiver<Payload>) {
    let Ok((mut w, _recv)) = open_stream(&conn, StreamKind::Input).await else {
        return;
    };
    while let Some(p) = rx.recv().await {
        if send(&mut w, p).await.is_err() {
            break;
        }
    }
    let _ = w.finish();
}

/// Proto enum value of a permission: index in `Permission::ALL` + 1.
pub(crate) fn perms_to_wire(p: Permissions) -> Vec<i32> {
    p.iter()
        .filter_map(|perm| {
            Permission::ALL
                .iter()
                .position(|q| *q == perm)
                .and_then(|i| i32::try_from(i + 1).ok())
        })
        .collect()
}

pub(crate) fn perms_from_wire(list: &[i32]) -> Permissions {
    list.iter()
        .filter_map(|v| usize::try_from(*v).ok())
        .filter_map(|v| v.checked_sub(1))
        .filter_map(|i| Permission::ALL.get(i).copied())
        .fold(Permissions::empty(), Permissions::with)
}

pub(crate) fn reject_to_wire(r: RejectReason) -> v1::SessionRejectReason {
    match r {
        RejectReason::UserRejected | RejectReason::Reported => {
            v1::SessionRejectReason::DeclinedByUser
        }
        RejectReason::Timeout => v1::SessionRejectReason::Timeout,
        RejectReason::Busy => v1::SessionRejectReason::Busy,
    }
}

pub(crate) fn reject_from_wire(v: i32) -> RejectReason {
    match v1::SessionRejectReason::try_from(v) {
        Ok(v1::SessionRejectReason::Timeout) => RejectReason::Timeout,
        Ok(v1::SessionRejectReason::Busy) => RejectReason::Busy,
        _ => RejectReason::UserRejected,
    }
}

pub(crate) fn end_to_wire(e: EndReason) -> v1::SessionEndReason {
    match e {
        EndReason::HostStopped | EndReason::Rejected(_) => v1::SessionEndReason::ClosedByHost,
        EndReason::Reported => v1::SessionEndReason::Reported,
        EndReason::PeerEnded => v1::SessionEndReason::ClosedByController,
        EndReason::PeerDisconnected => v1::SessionEndReason::NetworkLost,
        EndReason::TimeLimit => v1::SessionEndReason::TimeLimit,
    }
}

pub(crate) fn end_from_wire(v: i32) -> EndReason {
    match v1::SessionEndReason::try_from(v) {
        Ok(v1::SessionEndReason::Reported) => EndReason::Reported,
        Ok(v1::SessionEndReason::TimeLimit) => EndReason::TimeLimit,
        Ok(v1::SessionEndReason::NetworkLost) => EndReason::PeerDisconnected,
        Ok(v1::SessionEndReason::ClosedByController) => EndReason::PeerEnded,
        _ => EndReason::HostStopped,
    }
}

pub(crate) fn input_to_wire(i: RemoteInput) -> Payload {
    match i {
        RemoteInput::Touch {
            pointer_id,
            phase,
            x,
            y,
        } => Payload::TouchEvent(v1::TouchEvent {
            points: vec![v1::TouchPoint {
                id: pointer_id,
                phase: touch_phase_to_wire(phase).into(),
                display_id: 0,
                x: x.clamp(0.0, 1.0),
                y: y.clamp(0.0, 1.0),
                pressure: 0.0,
            }],
        }),
        RemoteInput::Key {
            hid_usage,
            down,
            modifiers,
        } => Payload::KeyEvent(v1::KeyEvent {
            hid_usage,
            down,
            modifiers,
            text: None,
            repeat: false,
        }),
        RemoteInput::Text { text } => Payload::KeyEvent(v1::KeyEvent {
            hid_usage: 0,
            down: true,
            modifiers: 0,
            text: Some(text),
            repeat: false,
        }),
        RemoteInput::MouseMove { x, y } => Payload::MouseMove(v1::MouseMove {
            motion: Some(v1::mouse_move::Motion::Absolute(v1::AbsolutePosition {
                display_id: 0,
                x: x.clamp(0.0, 1.0),
                y: y.clamp(0.0, 1.0),
            })),
        }),
        RemoteInput::MouseButton { button, down } => Payload::MouseButton(v1::MouseButton {
            button: button_to_wire(button).into(),
            down,
        }),
        RemoteInput::Wheel { dx, dy } => Payload::MouseWheel(v1::MouseWheel {
            delta_x: dx,
            delta_y: dy,
        }),
    }
}

/// Input envelopes from the controller, as FFI events. Touch frames yield one event per point.
pub(crate) fn input_from_wire(p: &Payload) -> Vec<RemoteInput> {
    match p {
        Payload::TouchEvent(t) => t
            .points
            .iter()
            .filter_map(|pt| {
                Some(RemoteInput::Touch {
                    pointer_id: pt.id,
                    phase: touch_phase_from_wire(pt.phase)?,
                    x: pt.x.clamp(0.0, 1.0),
                    y: pt.y.clamp(0.0, 1.0),
                })
            })
            .collect(),
        Payload::KeyEvent(k) => vec![match &k.text {
            Some(text) => RemoteInput::Text { text: text.clone() },
            None => RemoteInput::Key {
                hid_usage: k.hid_usage,
                down: k.down,
                modifiers: k.modifiers,
            },
        }],
        Payload::MouseMove(m) => match &m.motion {
            Some(v1::mouse_move::Motion::Absolute(a)) => vec![RemoteInput::MouseMove {
                x: a.x.clamp(0.0, 1.0),
                y: a.y.clamp(0.0, 1.0),
            }],
            _ => Vec::new(),
        },
        Payload::MouseButton(b) => button_from_wire(b.button)
            .map(|button| RemoteInput::MouseButton {
                button,
                down: b.down,
            })
            .into_iter()
            .collect(),
        Payload::MouseWheel(w) => vec![RemoteInput::Wheel {
            dx: w.delta_x,
            dy: w.delta_y,
        }],
        _ => Vec::new(),
    }
}

pub(crate) fn is_input(p: &Payload) -> bool {
    matches!(
        p,
        Payload::TouchEvent(_)
            | Payload::KeyEvent(_)
            | Payload::MouseMove(_)
            | Payload::MouseButton(_)
            | Payload::MouseWheel(_)
    )
}

pub(crate) fn video_config_to_wire(c: VideoConfigInfo) -> Payload {
    Payload::VideoConfig(v1::VideoConfig {
        stream_id: 0,
        codec: match c.codec {
            VideoCodec::H264 => v1::Codec::H264,
            VideoCodec::Hevc => v1::Codec::Hevc,
            VideoCodec::Av1 => v1::Codec::Av1,
        }
        .into(),
        width: c.width,
        height: c.height,
        fps: c.fps,
        bitrate_bps: c.bitrate_bps,
        chroma: v1::ChromaSubsampling::ChromaSubsampling420.into(),
        hdr: false,
        display_id: 0,
        codec_config: c.codec_config,
    })
}

pub(crate) fn video_config_from_wire(c: &v1::VideoConfig) -> Option<VideoConfigInfo> {
    let codec = match v1::Codec::try_from(c.codec).ok()? {
        v1::Codec::H264 => VideoCodec::H264,
        v1::Codec::Hevc => VideoCodec::Hevc,
        v1::Codec::Av1 => VideoCodec::Av1,
        v1::Codec::Unspecified => return None,
    };
    Some(VideoConfigInfo {
        codec,
        width: c.width,
        height: c.height,
        fps: c.fps,
        bitrate_bps: c.bitrate_bps,
        codec_config: c.codec_config.clone(),
    })
}

fn touch_phase_to_wire(p: TouchPhase) -> v1::TouchPhase {
    match p {
        TouchPhase::Down => v1::TouchPhase::Down,
        TouchPhase::Move => v1::TouchPhase::Move,
        TouchPhase::Up => v1::TouchPhase::Up,
        TouchPhase::Cancel => v1::TouchPhase::Cancel,
    }
}

fn touch_phase_from_wire(v: i32) -> Option<TouchPhase> {
    Some(match v1::TouchPhase::try_from(v).ok()? {
        v1::TouchPhase::Down => TouchPhase::Down,
        v1::TouchPhase::Move => TouchPhase::Move,
        v1::TouchPhase::Up => TouchPhase::Up,
        v1::TouchPhase::Cancel => TouchPhase::Cancel,
        v1::TouchPhase::Unspecified => return None,
    })
}

fn button_to_wire(b: MouseButtonKind) -> v1::MouseButtonKind {
    match b {
        MouseButtonKind::Left => v1::MouseButtonKind::Left,
        MouseButtonKind::Right => v1::MouseButtonKind::Right,
        MouseButtonKind::Middle => v1::MouseButtonKind::Middle,
        MouseButtonKind::Back => v1::MouseButtonKind::Back,
        MouseButtonKind::Forward => v1::MouseButtonKind::Forward,
    }
}

fn button_from_wire(v: i32) -> Option<MouseButtonKind> {
    Some(match v1::MouseButtonKind::try_from(v).ok()? {
        v1::MouseButtonKind::Left => MouseButtonKind::Left,
        v1::MouseButtonKind::Right => MouseButtonKind::Right,
        v1::MouseButtonKind::Middle => MouseButtonKind::Middle,
        v1::MouseButtonKind::Back => MouseButtonKind::Back,
        v1::MouseButtonKind::Forward => MouseButtonKind::Forward,
        v1::MouseButtonKind::Unspecified => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_survive_the_wire() {
        let p = Permissions::support().with(Permission::Whiteboard);
        assert_eq!(perms_from_wire(&perms_to_wire(p)), p);
        assert_eq!(perms_from_wire(&[0, -3, 99]), Permissions::empty());
        assert_eq!(perms_to_wire(Permissions::view_only()), vec![1]);
    }

    #[test]
    fn input_round_trips() {
        let cases = [
            RemoteInput::Touch {
                pointer_id: 2,
                phase: TouchPhase::Move,
                x: 0.25,
                y: 0.75,
            },
            RemoteInput::Key {
                hid_usage: 0x04,
                down: true,
                modifiers: 2,
            },
            RemoteInput::Text { text: "ă".into() },
            RemoteInput::MouseMove { x: 1.0, y: 0.0 },
            RemoteInput::MouseButton {
                button: MouseButtonKind::Right,
                down: false,
            },
            RemoteInput::Wheel { dx: 0, dy: -120 },
        ];
        for c in cases {
            let w = input_to_wire(c.clone());
            assert!(is_input(&w));
            assert_eq!(input_from_wire(&w), vec![c]);
        }
    }

    #[test]
    fn out_of_range_touch_is_clamped() {
        let w = input_to_wire(RemoteInput::Touch {
            pointer_id: 0,
            phase: TouchPhase::Down,
            x: 7.0,
            y: -1.0,
        });
        assert_eq!(
            input_from_wire(&w),
            vec![RemoteInput::Touch {
                pointer_id: 0,
                phase: TouchPhase::Down,
                x: 1.0,
                y: 0.0
            }]
        );
    }
}
