//! scrin-server: rendezvous (ID registry), embedded iroh relay and the
//! browser gateway (WebTransport + WebSocket fallback), in one process.
//!
//! [`Server::start`] binds everything described by a [`Config`] and returns
//! a handle with the bound addresses; `main.rs` is a thin wrapper around it.

pub mod api;
pub mod auth;
pub mod config;
pub mod gateway;
pub mod http;
pub mod ids;
pub mod limits;
pub mod metrics;
pub mod relay;
pub mod store;
pub mod tls;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::api::{AppState, Info};
pub use crate::config::{Config, Role, TlsMode};
use crate::gateway::{Gateway, GatewayConfig, quota};
use crate::http::TcpTls;
use crate::metrics::Metrics;
use crate::store::{MemoryStore, SqliteStore, Store};

/// A running server. Dropping it does not stop it; call [`Server::shutdown`].
#[derive(Debug)]
pub struct Server {
    pub tcp_addr: SocketAddr,
    pub wt_addr: Option<SocketAddr>,
    /// SHA-256 of the self-signed certificate (dev / `--tls self-signed`).
    pub cert_sha256: Option<[u8; 32]>,
    /// The leaf certificate (DER) when self-signed, for test clients.
    pub cert_der: Option<Vec<u8>>,
    pub state: Arc<AppState>,
    gateway: Option<Arc<Gateway>>,
    shutdown: watch::Sender<bool>,
    tasks: JoinSet<()>,
}

/// TLS material resolved from the config.
enum Tls {
    Cert(tls::CertKey),
    Acme(Arc<tokio_rustls_acme::ResolvesServerCertAcme>),
}

impl Server {
    pub async fn start(cfg: Config) -> anyhow::Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let store = open_store(&cfg)?;
        let metrics = Arc::new(Metrics::default());

        let (tls, tcp_tls, mut tasks) = resolve_tls(&cfg)?;
        let self_signed = matches!(cfg.tls_mode(), TlsMode::SelfSigned | TlsMode::None);
        let cert_sha256 = match &tls {
            Tls::Cert(ck) if self_signed => ck.leaf_sha256(),
            _ => None,
        };
        let cert_der = match &tls {
            Tls::Cert(ck) if cert_sha256.is_some() => ck.chain.first().map(|c| c.to_vec()),
            _ => None,
        };

        let state = app_state(&cfg, store.clone(), metrics.clone(), cert_sha256);
        let gateway = if cfg.has(Role::Gateway) {
            Some(bind_gateway(&cfg, state.clone()).await?)
        } else {
            None
        };

        let relay = cfg.has(Role::Relay).then(|| {
            let extra: HashSet<[u8; 32]> = gateway.iter().map(|g| g.endpoint_id()).collect();
            let open = cfg.relay_open || cfg.dev;
            relay::service(
                relay::RelayAccess::new(store.clone(), metrics.clone(), open, extra),
                cfg.relay_bps,
            )
        });
        let router = build_router(&cfg, &state, gateway.as_ref());

        let (shutdown, rx) = watch::channel(false);
        let listener = TcpListener::bind(cfg.tcp_addr())
            .await
            .with_context(|| format!("binding tcp {}", cfg.tcp_addr()))?;
        let tcp_addr = listener.local_addr()?;
        tasks.spawn(http::serve(listener, tcp_tls, router, relay, rx.clone()));

        if let Some(addr) = cfg.http_listen {
            let l = TcpListener::bind(addr)
                .await
                .with_context(|| format!("binding http {addr}"))?;
            tasks.spawn(http::serve(
                l,
                TcpTls::None,
                relay::probe_router(),
                None,
                rx.clone(),
            ));
        }

        if let Some(addr) = cfg.qad_listen {
            spawn_qad(&tls, addr, &mut tasks)?;
        }

        let wt_addr = match &gateway {
            Some(gw) => Some(spawn_webtransport(&cfg, &tls, gw, &rx, &mut tasks)?),
            None => None,
        };
        tasks.spawn(housekeeping(state.clone(), rx));

        Ok(Self {
            tcp_addr,
            wt_addr,
            cert_sha256,
            cert_der,
            state,
            gateway,
            shutdown,
            tasks,
        })
    }

    /// Stops all listeners and waits for the tasks to end.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(gw) = &self.gateway {
            gw.close().await;
        }
        let deadline = tokio::time::sleep(Duration::from_secs(5));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                () = &mut deadline => break,
                next = self.tasks.join_next() => if next.is_none() { break },
            }
        }
        self.tasks.shutdown().await;
    }
}

fn open_store(cfg: &Config) -> anyhow::Result<Arc<dyn Store>> {
    Ok(match &cfg.db {
        Some(p) => {
            if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
            }
            Arc::new(SqliteStore::open(p).with_context(|| format!("opening {}", p.display()))?)
        }
        None => Arc::new(MemoryStore::new()),
    })
}

fn app_state(
    cfg: &Config,
    store: Arc<dyn Store>,
    metrics: Arc<Metrics>,
    cert_sha256: Option<[u8; 32]>,
) -> Arc<AppState> {
    let mut state = AppState::new(store, metrics);
    state.presence_ttl = cfg.presence_ttl;
    state.abuse_block_threshold = cfg.abuse_block_threshold.max(1);
    state.trust_forwarded = cfg.trust_forwarded;
    state.info = Info {
        relay_urls: cfg.relay_urls.clone(),
        presence_ttl: cfg.presence_ttl,
        wt_cert_sha256: cert_sha256.map(|h| data_encoding::HEXLOWER.encode(&h)),
        gateway: cfg.has(Role::Gateway),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    Arc::new(state)
}

async fn bind_gateway(cfg: &Config, state: Arc<AppState>) -> anyhow::Result<Arc<Gateway>> {
    let relays = cfg
        .relay_urls
        .iter()
        .map(|u| u.parse().with_context(|| format!("relay url {u}")))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let gw_cfg = GatewayConfig {
        limits: quota::Limits {
            max_session: Duration::from_secs(cfg.gw_max_secs.max(1)),
            idle: Duration::from_secs(cfg.gw_idle_secs.max(1)),
            max_bps: cfg.gw_max_bps,
            max_bytes: cfg.gw_max_bytes,
        },
        max_sessions: 10_000,
    };
    Gateway::bind(state, gw_cfg, relays, cfg.gw_iroh_bind).await
}

fn build_router(cfg: &Config, state: &Arc<AppState>, gateway: Option<&Arc<Gateway>>) -> Router {
    let mut router = Router::new().merge(api::ops_router(state.clone()));
    if cfg.has(Role::Rendezvous) {
        router = router.merge(api::router(state.clone()));
    }
    if cfg.has(Role::Relay) {
        router = router.merge(relay::probe_router());
    }
    if let Some(gw) = gateway {
        router = router.merge(gateway::ws_router(gw.clone(), cfg.trust_forwarded));
    }
    router.layer(axum::extract::DefaultBodyLimit::max(api::MAX_BODY))
}

fn spawn_webtransport(
    cfg: &Config,
    tls: &Tls,
    gw: &Arc<Gateway>,
    rx: &watch::Receiver<bool>,
    tasks: &mut JoinSet<()>,
) -> anyhow::Result<SocketAddr> {
    let server_cfg = wtransport::ServerConfig::builder()
        .with_bind_address(cfg.udp_addr())
        .with_custom_tls(quic_tls(tls)?)
        .keep_alive_interval(Some(Duration::from_secs(10)))
        .max_idle_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| anyhow::anyhow!("idle timeout: {e}"))?
        .build();
    let ep = wtransport::Endpoint::server(server_cfg)
        .with_context(|| format!("binding udp {}", cfg.udp_addr()))?;
    let addr = ep.local_addr()?;
    tasks.spawn(gateway::serve_webtransport(ep, gw.clone(), rx.clone()));
    Ok(addr)
}

/// Prunes lockout state every minute.
async fn housekeeping(state: Arc<AppState>, mut rx: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            _ = rx.changed() => break,
            _ = tick.tick() => state.lockout.prune(std::time::Instant::now()),
        }
    }
}

fn resolve_tls(cfg: &Config) -> anyhow::Result<(Tls, TcpTls, JoinSet<()>)> {
    let tasks = JoinSet::new();
    match cfg.tls_mode() {
        TlsMode::None => {
            // TCP stays plain; WebTransport still needs a certificate.
            let ck = tls::self_signed(&dev_names(cfg))?;
            Ok((Tls::Cert(ck), TcpTls::None, tasks))
        }
        TlsMode::SelfSigned => {
            let ck = tls::self_signed(&dev_names(cfg))?;
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls::tcp_config(&ck)?));
            Ok((Tls::Cert(ck), TcpTls::Manual(acceptor), tasks))
        }
        TlsMode::Manual => {
            let (Some(c), Some(k)) = (&cfg.cert, &cfg.key) else {
                anyhow::bail!("--tls manual needs --cert and --key");
            };
            let ck = tls::load_pem(c, k)?;
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls::tcp_config(&ck)?));
            Ok((Tls::Cert(ck), TcpTls::Manual(acceptor), tasks))
        }
        TlsMode::Acme => acme(cfg, tasks),
    }
}

fn quic_tls(tls: &Tls) -> anyhow::Result<rustls::ServerConfig> {
    Ok(match tls {
        Tls::Cert(ck) => tls::quic_config(ck)?,
        Tls::Acme(resolver) => tls::quic_config_with_resolver(resolver.clone())?,
    })
}

fn dev_names(cfg: &Config) -> Vec<String> {
    let mut names = vec![
        "localhost".to_owned(),
        "127.0.0.1".to_owned(),
        "::1".to_owned(),
    ];
    names.extend(cfg.hostnames.iter().cloned());
    names
}

fn acme(cfg: &Config, tasks: JoinSet<()>) -> anyhow::Result<(Tls, TcpTls, JoinSet<()>)> {
    use futures_util::StreamExt as _;
    if cfg.acme_domains.is_empty() {
        anyhow::bail!("--tls acme needs --acme-domain (or use --dev / --tls self-signed)");
    }
    let cache = cfg.data_dir.join("acme");
    std::fs::create_dir_all(&cache).with_context(|| format!("creating {}", cache.display()))?;
    let mut state = tokio_rustls_acme::AcmeConfig::new(cfg.acme_domains.clone())
        .contact(cfg.acme_contact.clone())
        .directory_lets_encrypt(!cfg.acme_staging)
        .cache(tokio_rustls_acme::caches::DirCache::new(cache))
        .state();
    let resolver = state.resolver();
    let acceptor = state.acceptor();
    let mut tcp_cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(resolver.clone());
    tcp_cfg.alpn_protocols = vec![tls::HTTP1_ALPN.to_vec()];
    let mut tasks = tasks;
    tasks.spawn(async move {
        while let Some(ev) = state.next().await {
            match ev {
                Ok(ok) => tracing::info!(event = ?ok, "acme"),
                Err(err) => tracing::error!(error = ?err, "acme"),
            }
        }
    });
    Ok((
        Tls::Acme(resolver),
        TcpTls::Acme {
            acceptor,
            config: Arc::new(tcp_cfg),
        },
        tasks,
    ))
}

/// QUIC address discovery (iroh QAD) on its own UDP port, via the stock
/// iroh-relay server with only the QUIC part enabled.
fn spawn_qad(tls: &Tls, addr: SocketAddr, tasks: &mut JoinSet<()>) -> anyhow::Result<()> {
    let server_config = quic_tls(tls)?;
    let mut quic = iroh_relay::server::QuicConfig::new(addr);
    quic.server_config = Some(server_config);
    let mut config = iroh_relay::server::ServerConfig::default();
    config.quic = Some(quic);
    tasks.spawn(async move {
        match iroh_relay::server::Server::spawn(config).await {
            Ok(mut srv) => {
                tracing::info!(addr = ?srv.quic_addr(), "qad: listening");
                let _ = srv.join().await;
            }
            Err(e) => tracing::error!(error = %e, "qad: failed to start"),
        }
    });
    Ok(())
}
