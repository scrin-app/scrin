//! Mapping between the session state machines and `scrin.v1` envelopes, and
//! the control-stream reader/writer tasks.
//!
//! Every reliable stream carries length-prefixed frames (scrin-net framing)
//! whose body is one protobuf [`v1::Envelope`]. The Control stream is the one
//! the handshake opened; input travels on its own `StreamKind::Input` stream so
//! a burst of mouse moves never delays a `SessionEnd`.

use std::time::Duration;

use scrin_net::framing::{read_frame, write_frame};
use scrin_net::{Connection, RecvStream, SendStream};
use scrin_proto::v1::{self, envelope::Payload};
use scrin_session::{EndReason, Permission, Permissions, RejectReason};
use tokio::sync::mpsc;

use crate::backend::InputEvent;
use crate::gw::{CONTROL_LANE, Seal};

/// Largest control message we accept (cursor shapes are the biggest).
pub(crate) const MAX_CONTROL_MSG: usize = 1024 * 1024;

pub(crate) fn env(p: Payload) -> v1::Envelope {
    scrin_proto::envelope(p)
}

/// `scrin.v1.Permission` = session bit index + 1 (0 is UNSPECIFIED).
pub(crate) fn perms_to_wire(p: Permissions) -> Vec<i32> {
    p.iter()
        .map(|perm| i32::try_from(perm.bit().trailing_zeros()).unwrap_or(0) + 1)
        .collect()
}

pub(crate) fn perms_from_wire(v: &[i32]) -> Permissions {
    v.iter()
        .filter_map(|&w| {
            let idx = usize::try_from(w.checked_sub(1)?).ok()?;
            Permission::ALL.get(idx).copied()
        })
        .fold(Permissions::empty(), Permissions::with)
}

pub(crate) fn end_to_wire(r: EndReason) -> v1::SessionEndReason {
    match r {
        EndReason::HostStopped | EndReason::Rejected(_) => v1::SessionEndReason::ClosedByHost,
        EndReason::Reported => v1::SessionEndReason::Reported,
        EndReason::PeerEnded => v1::SessionEndReason::ClosedByController,
        EndReason::PeerDisconnected => v1::SessionEndReason::NetworkLost,
        EndReason::TimeLimit => v1::SessionEndReason::TimeLimit,
    }
}

pub(crate) fn end_from_wire(r: i32) -> EndReason {
    match v1::SessionEndReason::try_from(r) {
        Ok(v1::SessionEndReason::ClosedByHost | v1::SessionEndReason::HostShutdown) => {
            EndReason::HostStopped
        }
        Ok(v1::SessionEndReason::ClosedByController) => EndReason::PeerEnded,
        Ok(v1::SessionEndReason::TimeLimit) => EndReason::TimeLimit,
        Ok(v1::SessionEndReason::Reported) => EndReason::Reported,
        _ => EndReason::PeerDisconnected,
    }
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

pub(crate) fn reject_from_wire(r: i32) -> RejectReason {
    match v1::SessionRejectReason::try_from(r) {
        Ok(v1::SessionRejectReason::Timeout) => RejectReason::Timeout,
        Ok(v1::SessionRejectReason::Busy) => RejectReason::Busy,
        _ => RejectReason::UserRejected,
    }
}

pub(crate) const fn end_reason_name(r: EndReason) -> &'static str {
    match r {
        EndReason::HostStopped => "host-stopped",
        EndReason::Reported => "reported",
        EndReason::PeerEnded => "peer-ended",
        EndReason::PeerDisconnected => "disconnected",
        EndReason::TimeLimit => "time-limit",
        EndReason::Rejected(r) => reject_reason_name(r),
    }
}

pub(crate) const fn reject_reason_name(r: RejectReason) -> &'static str {
    match r {
        RejectReason::UserRejected => "rejected",
        RejectReason::Timeout => "timeout",
        RejectReason::Busy => "busy",
        RejectReason::Reported => "reported",
    }
}

pub(crate) fn input_to_payload(e: &InputEvent) -> Payload {
    match e {
        InputEvent::Key(k) => Payload::KeyEvent(k.clone()),
        InputEvent::MouseMove(m) => Payload::MouseMove(*m),
        InputEvent::MouseButton(b) => Payload::MouseButton(*b),
        InputEvent::MouseWheel(w) => Payload::MouseWheel(*w),
    }
}

pub(crate) fn input_from_payload(p: Payload) -> Option<InputEvent> {
    Some(match p {
        Payload::KeyEvent(k) => InputEvent::Key(k),
        Payload::MouseMove(m) => InputEvent::MouseMove(m),
        Payload::MouseButton(b) => InputEvent::MouseButton(b),
        Payload::MouseWheel(w) => InputEvent::MouseWheel(w),
        _ => return None,
    })
}

/// What the actor asks a stream writer to do.
#[derive(Debug)]
pub(crate) enum Outgoing {
    Env(v1::Envelope),
    /// Flush, finish the stream, give the peer a moment to read, then close
    /// the connection.
    Close,
}

/// Writes queued envelopes; on [`Outgoing::Close`] or channel drop it
/// finishes the stream and closes the connection. With `seal` (gateway
/// path) every frame is sealed on the Control lane.
pub(crate) async fn writer_task(
    conn: Connection,
    mut send: SendStream,
    mut rx: mpsc::UnboundedReceiver<Outgoing>,
    seal: Seal,
) {
    let lane = CONTROL_LANE;
    while let Some(msg) = rx.recv().await {
        match msg {
            Outgoing::Env(e) => {
                let Some(bytes) =
                    seal_frame(seal.as_deref(), lane, &scrin_proto::encode_envelope(&e))
                else {
                    break;
                };
                if write_frame(&mut send, &bytes).await.is_err() {
                    break;
                }
            }
            Outgoing::Close => {
                let _ = send.finish();
                // The peer closes once it has read SessionEnd; do not cut it off.
                let _ = tokio::time::timeout(Duration::from_secs(1), conn.closed()).await;
                break;
            }
        }
    }
    conn.close(0u32.into(), b"bye");
}

/// Plain bytes, or sealed on `lane` when a channel is set. `None` = sealing failed.
pub(crate) fn seal_frame(
    seal: Option<&crate::gw::Channel>,
    lane: u32,
    plain: &[u8],
) -> Option<Vec<u8>> {
    match seal {
        None => Some(plain.to_vec()),
        Some(c) => c.seal(lane, plain).ok(),
    }
}

fn open_frame(seal: Option<&crate::gw::Channel>, lane: u32, bytes: Vec<u8>) -> Option<Vec<u8>> {
    match seal {
        None => Some(bytes),
        Some(c) => c.open_stream_frame(lane, &bytes),
    }
}

/// Reads envelopes until the stream ends; `on_msg(None)` signals the end.
/// A frame that fails to open (gateway path) ends the stream.
pub(crate) async fn reader_task(
    mut recv: RecvStream,
    seal: Seal,
    mut on_msg: impl FnMut(Option<v1::Envelope>) + Send,
) {
    let lane = CONTROL_LANE;
    while let Ok(Some(bytes)) =
        scrin_net::framing::read_frame_capped(&mut recv, MAX_CONTROL_MSG).await
    {
        let Some(bytes) = open_frame(seal.as_deref(), lane, bytes) else {
            break;
        };
        match scrin_proto::decode_envelope(&bytes) {
            Ok(e) => on_msg(Some(e)),
            // Unknown payloads from newer peers are skipped, not fatal.
            Err(scrin_proto::DecodeError::EmptyPayload) => {}
            Err(_) => break,
        }
    }
    on_msg(None);
}

/// Input stream reader on the host: every envelope that is an input event.
/// `lane` is the stream's inner-channel lane (gateway path only).
pub(crate) async fn read_input_stream(
    mut recv: RecvStream,
    seal: Seal,
    lane: u32,
    mut on_input: impl FnMut(InputEvent) + Send,
) {
    while let Ok(Some(bytes)) = read_frame(&mut recv).await {
        if bytes.len() > 64 * 1024 {
            break;
        }
        let Some(bytes) = open_frame(seal.as_deref(), lane, bytes) else {
            break;
        };
        if let Ok(e) = scrin_proto::decode_envelope(&bytes)
            && let Some(p) = e.payload
            && let Some(ev) = input_from_payload(p)
        {
            on_input(ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_map_to_the_proto_enum() {
        let p = Permissions::support();
        let w = perms_to_wire(p);
        assert!(w.contains(&i32::from(v1::Permission::View)));
        assert!(w.contains(&i32::from(v1::Permission::Whiteboard)));
        assert_eq!(perms_from_wire(&w), p);
        assert_eq!(perms_from_wire(&[0, 99, -4]), Permissions::empty());
        assert_eq!(
            perms_to_wire(Permissions::only(Permission::FilesIn)),
            vec![i32::from(v1::Permission::FilesIn)]
        );
    }

    #[test]
    fn end_reasons_round_trip_where_they_can() {
        for r in [
            EndReason::HostStopped,
            EndReason::PeerEnded,
            EndReason::TimeLimit,
            EndReason::Reported,
            EndReason::PeerDisconnected,
        ] {
            assert_eq!(end_from_wire(end_to_wire(r).into()), r);
        }
    }
}
