//! WebSocket fallback of the gateway (`wss://host/v1/ws?id=<9 digits>`), for
//! networks that block UDP. Framing: [`super::frame`]; contract: `GATEWAY.md`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use futures_util::{SinkExt as _, StreamExt as _};
use iroh::endpoint::{Connection as HostConn, RecvStream as HostRecv, SendStream as HostSend};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;

use super::frame::Frame;
use super::pipe::{PIPE_BUF, Rx as _, Tx as _};
use super::quota::Quota;
use super::{Gateway, close_host, code, host_close_code};
use crate::api::{Peer, client_ip};

/// Largest accepted WebSocket message (one frame).
pub const MAX_WS_MESSAGE: usize = 256 * 1024;
/// Concurrent streams per session.
const MAX_STREAMS: usize = 256;
/// Queued frames per browser->host stream before the WebSocket reader waits.
const STREAM_QUEUE: usize = 32;

#[derive(Debug, Deserialize)]
struct Target {
    id: String,
}

#[derive(Debug, Clone)]
struct WsState {
    gw: Arc<Gateway>,
    trust_forwarded: bool,
}

pub fn router(gw: Arc<Gateway>, trust_forwarded: bool) -> Router {
    Router::new()
        .route("/v1/ws", get(upgrade))
        .with_state(WsState {
            gw,
            trust_forwarded,
        })
}

/// The TCP peer inserted by the connection loop, if any.
struct MaybePeer(Option<Peer>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for MaybePeer {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(Ok(Self(parts.extensions.get::<Peer>().copied())))
    }
}

async fn upgrade(
    State(st): State<WsState>,
    Query(t): Query<Target>,
    headers: HeaderMap,
    MaybePeer(peer): MaybePeer,
    ws: WebSocketUpgrade,
) -> Response {
    let ip = client_ip(&headers, peer.as_ref(), st.trust_forwarded);
    let host = match st.gw.open(ip, &t.id).await {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!(%ip, error = %e, "ws: rejected");
            return e.into_response();
        }
    };
    let gw = st.gw.clone();
    ws.max_message_size(MAX_WS_MESSAGE)
        .on_upgrade(move |socket| async move {
            let _guard = gw.session();
            let quota = Arc::new(Quota::new(gw.limits(), Instant::now()));
            let started = Instant::now();
            let reason = Session::new(gw.clone(), host, quota.clone())
                .run(socket)
                .await;
            tracing::info!(
                %ip,
                secs = started.elapsed().as_secs(),
                bytes = quota.bytes(),
                reason,
                "ws: session ended"
            );
        })
}

enum Cmd {
    Data(Bytes),
    Fin,
    Reset(u64),
}

#[derive(Default)]
struct StreamSlot {
    /// Browser -> host half (absent for host-initiated uni streams).
    to_host: Option<mpsc::Sender<Cmd>>,
    /// Stops the host -> browser half (absent for browser-initiated uni streams).
    stop_reader: Option<oneshot::Sender<u64>>,
}

struct Session {
    gw: Arc<Gateway>,
    host: HostConn,
    quota: Arc<Quota>,
    out: mpsc::Sender<Frame>,
    out_rx: Option<mpsc::Receiver<Frame>>,
    streams: HashMap<u64, StreamSlot>,
    /// Highest browser-initiated id seen per type (bidi, uni), QUIC-style.
    max_browser: [Option<u64>; 2],
    next_host: [u64; 2],
    tasks: JoinSet<()>,
}

/// Stream id type bits (RFC 9000 §2.1): bit 0 = initiator (0 browser,
/// 1 host), bit 1 = direction (0 bidi, 1 uni).
fn is_browser(id: u64) -> bool {
    id & 1 == 0
}
fn is_uni(id: u64) -> bool {
    id & 2 != 0
}

impl Session {
    fn new(gw: Arc<Gateway>, host: HostConn, quota: Arc<Quota>) -> Self {
        let (out, out_rx) = mpsc::channel(256);
        Self {
            gw,
            host,
            quota,
            out,
            out_rx: Some(out_rx),
            streams: HashMap::new(),
            max_browser: [None, None],
            next_host: [1, 3],
            tasks: JoinSet::new(),
        }
    }

    async fn run(mut self, socket: WebSocket) -> u32 {
        let (mut sink, mut stream) = socket.split();
        let Some(mut out_rx) = self.out_rx.take() else {
            return code::PROTOCOL;
        };
        let (closed_tx, mut closed_rx) = mpsc::channel::<u64>(64);
        let reason = loop {
            tokio::select! {
                c = self.quota.watchdog() => break c,
                e = self.host.closed() => break host_close_code(&e),
                Some(f) = out_rx.recv() => {
                    if let Frame::Data { payload, .. } | Frame::Dgram(payload) = &f {
                        self.gw.count_bytes(payload.len());
                    }
                    if sink.send(Message::Binary(f.encode())).await.is_err() {
                        break code::BROWSER_GONE;
                    }
                }
                Some(id) = closed_rx.recv() => { self.streams.remove(&id); }
                Some(_) = self.tasks.join_next() => {}
                msg = stream.next() => match msg {
                    Some(Ok(Message::Binary(b))) => {
                        if let Err(c) = self.on_frame(&b, &closed_tx).await {
                            break c;
                        }
                    }
                    Some(Ok(Message::Close(cf))) => {
                        break cf.map_or(code::BROWSER_GONE, |f| browser_code(f.code));
                    }
                    Some(Ok(Message::Text(_))) => break code::PROTOCOL,
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break code::BROWSER_GONE,
                },
                r = self.host.accept_bi() => {
                    let Ok((hs, hr)) = r else { continue };
                    let id = self.alloc_host(0);
                    self.attach(id, Some(hs), Some(hr), &closed_tx);
                }
                r = self.host.accept_uni() => {
                    let Ok(hr) = r else { continue };
                    let id = self.alloc_host(1);
                    self.attach(id, None, Some(hr), &closed_tx);
                }
                r = self.host.read_datagram() => {
                    let Ok(p) = r else { continue };
                    match self.quota.spend_datagram(p.len()) {
                        Ok(true) => { let _ = self.out.try_send(Frame::Dgram(p)); }
                        Ok(false) => {}
                        Err(c) => break c,
                    }
                }
            }
        };
        close_host(&self.host, reason);
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: code::ws(reason),
                reason: "".into(),
            })))
            .await;
        self.tasks.shutdown().await;
        reason
    }

    fn alloc_host(&mut self, uni: usize) -> u64 {
        let id = self.next_host[uni];
        self.next_host[uni] += 4;
        id
    }

    async fn on_frame(&mut self, raw: &Bytes, closed: &mpsc::Sender<u64>) -> Result<(), u32> {
        let frame = Frame::decode(raw).map_err(|_| code::PROTOCOL)?;
        match frame {
            Frame::Dgram(p) => {
                if self.quota.spend_datagram(p.len())? {
                    self.gw.count_bytes(p.len());
                    let _ = self.host.send_datagram(p);
                }
            }
            Frame::Data { id, payload } => {
                let tx = self.sender_for(id, closed).await?;
                if let Some(tx) = tx {
                    let _ = tx.send(Cmd::Data(payload)).await;
                }
            }
            Frame::Fin { id } => {
                if let Some(tx) = self.sender_for(id, closed).await? {
                    let _ = tx.send(Cmd::Fin).await;
                }
            }
            Frame::Reset { id, code } => {
                if let Some(tx) = self.sender_for(id, closed).await? {
                    let _ = tx.send(Cmd::Reset(code)).await;
                }
            }
            Frame::Stop { id, code } => {
                if let Some(stop) = self.streams.get_mut(&id).and_then(|s| s.stop_reader.take()) {
                    let _ = stop.send(code);
                }
            }
        }
        Ok(())
    }

    /// The browser->host sender for `id`, opening the host stream on first
    /// use of a new browser-initiated id. `None` = stream already gone.
    async fn sender_for(
        &mut self,
        id: u64,
        closed: &mpsc::Sender<u64>,
    ) -> Result<Option<mpsc::Sender<Cmd>>, u32> {
        if let Some(slot) = self.streams.get(&id) {
            return Ok(slot.to_host.clone());
        }
        if !is_browser(id) {
            // Host-initiated uni streams can't carry browser data; closed ids are ignored.
            return Ok(None);
        }
        let ty = usize::from(is_uni(id));
        if self.max_browser[ty].is_some_and(|m| id <= m) {
            return Ok(None);
        }
        if self.streams.len() >= MAX_STREAMS {
            return Err(code::QUOTA);
        }
        self.max_browser[ty] = Some(id);
        if is_uni(id) {
            let hs = self.host.open_uni().await.map_err(|_| code::HOST_GONE)?;
            self.attach(id, Some(hs), None, closed);
        } else {
            let (hs, hr) = self.host.open_bi().await.map_err(|_| code::HOST_GONE)?;
            self.attach(id, Some(hs), Some(hr), closed);
        }
        Ok(self.streams.get(&id).and_then(|s| s.to_host.clone()))
    }

    fn attach(
        &mut self,
        id: u64,
        send: Option<HostSend>,
        recv: Option<HostRecv>,
        closed: &mpsc::Sender<u64>,
    ) {
        let mut slot = StreamSlot::default();
        let halves = usize::from(send.is_some()) + usize::from(recv.is_some());
        let done = Arc::new(std::sync::atomic::AtomicUsize::new(halves));
        if let Some(hs) = send {
            let (tx, rx) = mpsc::channel(STREAM_QUEUE);
            slot.to_host = Some(tx);
            self.tasks.spawn(to_host(
                id,
                rx,
                hs,
                self.quota.clone(),
                self.gw.clone(),
                self.out.clone(),
                Finished(id, done.clone(), closed.clone()),
            ));
        }
        if let Some(hr) = recv {
            let (tx, rx) = oneshot::channel();
            slot.stop_reader = Some(tx);
            self.tasks.spawn(from_host(
                id,
                hr,
                rx,
                self.quota.clone(),
                self.out.clone(),
                Finished(id, done, closed.clone()),
            ));
        }
        self.streams.insert(id, slot);
    }
}

/// Removes the stream from the session map when both halves are done.
struct Finished(u64, Arc<std::sync::atomic::AtomicUsize>, mpsc::Sender<u64>);

impl Drop for Finished {
    fn drop(&mut self) {
        if self.1.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) == 1 {
            let _ = self.2.try_send(self.0);
        }
    }
}

fn browser_code(ws_code: u16) -> u32 {
    if (4000..5000).contains(&ws_code) {
        u32::from(ws_code - 4000)
    } else {
        code::BROWSER_GONE
    }
}

async fn to_host(
    id: u64,
    mut rx: mpsc::Receiver<Cmd>,
    mut hs: HostSend,
    quota: Arc<Quota>,
    gw: Arc<Gateway>,
    out: mpsc::Sender<Frame>,
    _done: Finished,
) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Data(p) => {
                if quota.spend(p.len()).await.is_err() {
                    hs.tx_reset(u64::from(code::QUOTA));
                    return;
                }
                gw.count_bytes(p.len());
                if let Err(c) = hs.tx_write(&p).await {
                    let _ = out
                        .send(Frame::Stop {
                            id,
                            code: c.unwrap_or(0),
                        })
                        .await;
                    return;
                }
            }
            Cmd::Fin => {
                hs.tx_finish().await;
                return;
            }
            Cmd::Reset(c) => {
                hs.tx_reset(c);
                return;
            }
        }
    }
}

async fn from_host(
    id: u64,
    mut hr: HostRecv,
    mut stop: oneshot::Receiver<u64>,
    quota: Arc<Quota>,
    out: mpsc::Sender<Frame>,
    _done: Finished,
) {
    let mut buf = vec![0u8; PIPE_BUF];
    loop {
        let read = tokio::select! {
            c = &mut stop => {
                if let Ok(c) = c {
                    hr.rx_stop(c);
                }
                return;
            }
            r = hr.rx_read(&mut buf) => r,
        };
        let frame = match read {
            Ok(Some(n)) => {
                if quota.spend(n).await.is_err() {
                    hr.rx_stop(u64::from(code::QUOTA));
                    return;
                }
                Frame::Data {
                    id,
                    payload: Bytes::copy_from_slice(&buf[..n]),
                }
            }
            Ok(None) => Frame::Fin { id },
            Err(c) => Frame::Reset {
                id,
                code: c.unwrap_or(0),
            },
        };
        let last = !matches!(frame, Frame::Data { .. });
        if out.send(frame).await.is_err() || last {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_id_bits() {
        assert!(is_browser(0) && !is_uni(0));
        assert!(!is_browser(1) && !is_uni(1));
        assert!(is_browser(2) && is_uni(2));
        assert!(!is_browser(3) && is_uni(3));
    }

    #[test]
    fn ws_close_codes_map_back() {
        assert_eq!(browser_code(4000), 0);
        assert_eq!(browser_code(code::ws(code::IDLE)), code::IDLE);
        assert_eq!(browser_code(1000), code::BROWSER_GONE);
    }
}
