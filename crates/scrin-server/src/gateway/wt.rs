//! WebTransport side of the gateway (`https://host:port/v1/gw?id=<9 digits>`).

use std::sync::Arc;
use std::time::Instant;

use iroh::endpoint::Connection as HostConn;
use tokio::sync::watch;
use tokio::task::JoinSet;
use wtransport::endpoint::IncomingSession;
use wtransport::endpoint::endpoint_side::Server;
use wtransport::{Connection as WtConn, Endpoint, VarInt};

use super::pipe::pipe;
use super::quota::Quota;
use super::{Gateway, close_host, code, host_close_code};
use crate::api::ApiError;

/// Request path of the WebTransport gateway.
pub const WT_PATH: &str = "/v1/gw";

/// Accepts WebTransport sessions until `shutdown` flips.
pub async fn serve(
    endpoint: Endpoint<Server>,
    gw: Arc<Gateway>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            Some(_) = tasks.join_next() => {}
            incoming = endpoint.accept() => {
                let gw = gw.clone();
                tasks.spawn(async move { handle(incoming, gw).await });
            }
        }
    }
    endpoint.close(VarInt::from_u32(code::SHUTDOWN), b"shutdown");
    tasks.shutdown().await;
}

fn target_id(path: &str) -> Option<&str> {
    let (p, q) = path.split_once('?')?;
    if p != WT_PATH {
        return None;
    }
    q.split('&').find_map(|kv| kv.strip_prefix("id="))
}

async fn handle(incoming: IncomingSession, gw: Arc<Gateway>) {
    let request = match incoming.await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(error = %e, "wt: handshake failed");
            return;
        }
    };
    let ip = request.remote_address().ip();
    let Some(id) = target_id(request.path()).map(str::to_owned) else {
        request.not_found().await;
        return;
    };
    let host = match gw.open(ip, &id).await {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!(%ip, error = %e, "wt: rejected");
            match e {
                ApiError::RateLimited | ApiError::LockedOut => request.too_many_requests().await,
                ApiError::Offline | ApiError::Unreachable | ApiError::NotRegistered => {
                    request.not_found().await;
                }
                _ => request.forbidden().await,
            }
            return;
        }
    };
    let browser = match request.accept().await {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "wt: accept failed");
            close_host(&host, code::BROWSER_GONE);
            return;
        }
    };
    let _guard = gw.session();
    let quota = Arc::new(Quota::new(gw.limits(), Instant::now()));
    let started = Instant::now();
    let reason = bridge(&gw, &browser, &host, &quota).await;
    tracing::info!(
        %ip,
        secs = started.elapsed().as_secs(),
        bytes = quota.bytes(),
        reason,
        "wt: session ended"
    );
}

/// Runs the session until one side closes or a limit fires; returns the
/// close code applied to both sides.
async fn bridge(gw: &Arc<Gateway>, browser: &WtConn, host: &HostConn, quota: &Arc<Quota>) -> u32 {
    let mut streams = JoinSet::new();
    let count = {
        let gw = gw.clone();
        move |n: usize| gw.count_bytes(n)
    };
    let reason = loop {
        tokio::select! {
            c = quota.watchdog() => {
                close_host(host, c);
                browser.close(VarInt::from_u32(c), b"");
                break c;
            }
            e = browser.closed() => {
                let c = match &e {
                    wtransport::error::ConnectionError::ApplicationClosed(a) => {
                        u32::try_from(a.code().into_inner()).unwrap_or(code::BROWSER_GONE)
                    }
                    _ => code::BROWSER_GONE,
                };
                close_host(host, c);
                break c;
            }
            e = host.closed() => {
                let c = host_close_code(&e);
                browser.close(VarInt::from_u32(c), b"");
                break c;
            }
            Some(_) = streams.join_next() => {}
            r = browser.accept_bi() => {
                let Ok((bs, br)) = r else { continue };
                let Ok((hs, hr)) = host.open_bi().await else { continue };
                streams.spawn(pipe(br, hs, quota.clone(), count.clone()));
                streams.spawn(pipe(hr, bs, quota.clone(), count.clone()));
            }
            r = host.accept_bi() => {
                let Ok((hs, hr)) = r else { continue };
                let Ok(opening) = browser.open_bi().await else { continue };
                let Ok((bs, br)) = opening.await else { continue };
                streams.spawn(pipe(br, hs, quota.clone(), count.clone()));
                streams.spawn(pipe(hr, bs, quota.clone(), count.clone()));
            }
            r = browser.accept_uni() => {
                let Ok(br) = r else { continue };
                let Ok(hs) = host.open_uni().await else { continue };
                streams.spawn(pipe(br, hs, quota.clone(), count.clone()));
            }
            r = host.accept_uni() => {
                let Ok(hr) = r else { continue };
                let Ok(opening) = browser.open_uni().await else { continue };
                let Ok(bs) = opening.await else { continue };
                streams.spawn(pipe(hr, bs, quota.clone(), count.clone()));
            }
            r = browser.receive_datagram() => {
                let Ok(d) = r else { continue };
                let p = d.payload();
                match quota.spend_datagram(p.len()) {
                    Ok(true) => {
                        count(p.len());
                        let _ = host.send_datagram(p);
                    }
                    Ok(false) => {}
                    Err(c) => {
                        close_host(host, c);
                        browser.close(VarInt::from_u32(c), b"");
                        break c;
                    }
                }
            }
            r = host.read_datagram() => {
                let Ok(p) = r else { continue };
                match quota.spend_datagram(p.len()) {
                    Ok(true) => {
                        count(p.len());
                        let _ = browser.send_datagram(&p);
                    }
                    Ok(false) => {}
                    Err(c) => {
                        close_host(host, c);
                        browser.close(VarInt::from_u32(c), b"");
                        break c;
                    }
                }
            }
        }
    };
    streams.shutdown().await;
    reason
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_id_parses_query() {
        assert_eq!(target_id("/v1/gw?id=123456789"), Some("123456789"));
        assert_eq!(target_id("/v1/gw?x=1&id=123456789"), Some("123456789"));
        assert_eq!(target_id("/v1/gw"), None);
        assert_eq!(target_id("/v2/gw?id=1"), None);
    }
}
