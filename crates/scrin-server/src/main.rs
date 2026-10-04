//! `scrin-server` binary: parse config, start, wait for Ctrl-C / SIGTERM.

use clap::Parser as _;
use scrin_server::{Config, Server, tls};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::parse();
    let filter = if cfg.dev && cfg.log == "info" {
        "info,scrin_server=debug".to_owned()
    } else {
        cfg.log.clone()
    };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(&filter).unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let server = Server::start(cfg.clone()).await?;
    let scheme = if cfg.tls_mode() == scrin_server::TlsMode::None {
        "http"
    } else {
        "https"
    };
    tracing::info!(roles = ?cfg.roles, "listening on {scheme}://{}", server.tcp_addr);
    if let Some(wt) = server.wt_addr {
        tracing::info!("webtransport on https://{wt}/v1/gw?id=<scrin id>");
    }
    if let Some(h) = server.cert_sha256 {
        // Printed on stdout so scripts can pick it up.
        println!("cert-sha256: {}", data_encoding::HEXLOWER.encode(&h));
        println!("cert-sha256-colon: {}", tls::colon_hex(&h));
        println!("cert-sha256-base64: {}", data_encoding::BASE64.encode(&h));
    }

    wait_for_signal().await;
    tracing::info!("shutting down");
    server.shutdown().await;
    Ok(())
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
