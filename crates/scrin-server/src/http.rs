//! The TCP listener: TLS (manual, ACME or none), then HTTP/1.1 with upgrades.
//!
//! `GET /relay` goes to the embedded iroh relay service; everything else
//! (API, `/v1/ws`, probes, ops) goes to the axum router. The relay requires
//! the connection's IO to be a `TokioIo<MaybeTlsStream>` (it downcasts the
//! upgraded stream), so this loop builds exactly that type.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use hyper::body::Incoming;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use iroh_relay::server::RelayService;
use iroh_relay::server::http_server::RelayServiceWithNotify;
use iroh_relay::server::streams::MaybeTlsStream;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, watch};
use tokio_rustls::TlsAcceptor;
use tokio_rustls_acme::AcmeAcceptor;
use tower::ServiceExt as _;

use crate::api::Peer;
use crate::relay::RELAY_PATH;

const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How the TCP listener terminates TLS.
#[derive(Clone)]
pub enum TcpTls {
    None,
    Manual(TlsAcceptor),
    Acme {
        acceptor: AcmeAcceptor,
        config: Arc<rustls::ServerConfig>,
    },
}

impl std::fmt::Debug for TcpTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "TcpTls::None",
            Self::Manual(_) => "TcpTls::Manual",
            Self::Acme { .. } => "TcpTls::Acme",
        })
    }
}

/// Serves `listener` until `shutdown` flips to `true`.
pub async fn serve(
    listener: TcpListener,
    tls: TcpTls,
    router: Router,
    relay: Option<RelayService>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            Some(_) = tasks.join_next() => {}
            res = listener.accept() => match res {
                Ok((stream, peer)) => {
                    let tls = tls.clone();
                    let router = router.clone();
                    let relay = relay.clone();
                    tasks.spawn(async move {
                        if let Err(e) = handle(stream, peer, tls, router, relay).await {
                            tracing::debug!(%peer, error = %e, "connection ended with error");
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }
    if let Some(r) = relay {
        r.shutdown().await;
    }
    tasks.shutdown().await;
}

async fn handle(
    stream: TcpStream,
    peer: SocketAddr,
    tls: TcpTls,
    router: Router,
    relay: Option<RelayService>,
) -> anyhow::Result<()> {
    let _ = stream.set_nodelay(true);
    let io = match tls {
        TcpTls::None => MaybeTlsStream::Plain(stream),
        TcpTls::Manual(acceptor) => {
            let tls =
                tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await??;
            MaybeTlsStream::Tls(tls)
        }
        TcpTls::Acme { acceptor, config } => {
            let start =
                tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await??;
            let Some(start) = start else {
                tracing::info!("acme: answered TLS-ALPN-01 challenge");
                return Ok(());
            };
            let tls =
                tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, start.into_stream(config)).await??;
            MaybeTlsStream::Tls(tls)
        }
    };

    let relay = relay.map(|r| RelayServiceWithNotify::new(r, Arc::new(Notify::new())));
    let svc = hyper::service::service_fn(move |mut req: Request<Incoming>| {
        let router = router.clone();
        let relay = relay.clone();
        async move {
            if let Some(relay) = relay
                && req.method() == hyper::Method::GET
                && req.uri().path() == RELAY_PATH
            {
                use hyper::service::Service as _;
                let res = match relay.call(req).await {
                    Ok(r) => r.map(Body::new),
                    Err(e) => {
                        tracing::debug!(error = %e, "relay upgrade failed");
                        Response::builder()
                            .status(500)
                            .body(Body::empty())
                            .unwrap_or_default()
                    }
                };
                return Ok::<_, Infallible>(res);
            }
            req.extensions_mut().insert(Peer(peer));
            let res = router.oneshot(req.map(Body::new)).await;
            Ok(res.unwrap_or_else(|e: Infallible| match e {}))
        }
    });

    hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT)
        .serve_connection(TokioIo::new(io), svc)
        .with_upgrades()
        .await?;
    Ok(())
}
