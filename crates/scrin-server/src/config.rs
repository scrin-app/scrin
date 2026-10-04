//! Command-line flags and `SCRIN_*` environment variables.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// Which services this process runs. Any combination works in one process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Role {
    /// HTTPS JSON API: register, presence, resolve, reports.
    Rendezvous,
    /// iroh relay on `/relay` of the TCP listener (+ optional QAD on UDP).
    Relay,
    /// WebTransport `/v1/gw` (UDP) and WebSocket `/v1/ws` (TCP) bridge to iroh hosts.
    Gateway,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TlsMode {
    /// Plain HTTP on the TCP listener (localhost / behind a TLS proxy only).
    /// WebTransport still needs TLS and gets a self-signed certificate.
    None,
    /// Generate an ECDSA P-256 certificate valid for 14 days (dev; usable
    /// from browsers through `serverCertificateHashes`).
    SelfSigned,
    /// PEM certificate chain and key from `--cert` / `--key`.
    Manual,
    /// Let's Encrypt via TLS-ALPN-01 on the TCP listener (must be port 443).
    Acme,
}

#[derive(Debug, Clone, Parser)]
#[command(
    name = "scrin-server",
    version,
    about = "scrin rendezvous, relay and browser gateway"
)]
#[allow(clippy::struct_excessive_bools)] // independent CLI switches, not a state machine
pub struct Config {
    /// Services to run (comma separated).
    #[arg(
        long,
        env = "SCRIN_ROLES",
        value_delimiter = ',',
        default_value = "rendezvous,relay,gateway"
    )]
    pub roles: Vec<Role>,

    /// Developer mode: self-signed TLS on 127.0.0.1:4433 (TCP + UDP), relay open to
    /// unregistered devices, debug logging. Prints the certificate hash.
    #[arg(long, env = "SCRIN_DEV")]
    pub dev: bool,

    /// TCP listener: HTTPS API, `/relay`, `/v1/ws`.
    #[arg(long, env = "SCRIN_LISTEN")]
    pub listen: Option<SocketAddr>,

    /// UDP listener for WebTransport (`/v1/gw`). Usually the same port as `--listen`.
    #[arg(long, env = "SCRIN_WT_LISTEN")]
    pub wt_listen: Option<SocketAddr>,

    /// Optional plain-HTTP listener (port 80) for captive-portal checks (`/generate_204`).
    #[arg(long, env = "SCRIN_HTTP_LISTEN")]
    pub http_listen: Option<SocketAddr>,

    /// Optional UDP listener for iroh QUIC address discovery (QAD), normally port 7842.
    #[arg(long, env = "SCRIN_QAD_LISTEN")]
    pub qad_listen: Option<SocketAddr>,

    #[arg(long, env = "SCRIN_TLS", value_enum)]
    pub tls: Option<TlsMode>,

    /// PEM certificate chain (`--tls manual`).
    #[arg(long, env = "SCRIN_TLS_CERT")]
    pub cert: Option<PathBuf>,

    /// PEM private key (`--tls manual`).
    #[arg(long, env = "SCRIN_TLS_KEY")]
    pub key: Option<PathBuf>,

    /// Extra DNS names / IPs for the self-signed certificate.
    #[arg(long = "hostname", env = "SCRIN_HOSTNAMES", value_delimiter = ',')]
    pub hostnames: Vec<String>,

    /// Domains for Let's Encrypt (`--tls acme`).
    #[arg(
        long = "acme-domain",
        env = "SCRIN_ACME_DOMAINS",
        value_delimiter = ','
    )]
    pub acme_domains: Vec<String>,

    /// ACME account contact, e.g. `mailto:ops@example.org`.
    #[arg(
        long = "acme-contact",
        env = "SCRIN_ACME_CONTACT",
        value_delimiter = ','
    )]
    pub acme_contact: Vec<String>,

    /// Use the Let's Encrypt staging directory (untrusted certs, generous limits).
    #[arg(long, env = "SCRIN_ACME_STAGING")]
    pub acme_staging: bool,

    /// Directory for ACME account + certificate cache.
    #[arg(long, env = "SCRIN_DATA_DIR", default_value = "./data")]
    pub data_dir: PathBuf,

    /// SQLite database path. Without it, state lives in memory and is lost on restart.
    #[arg(long, env = "SCRIN_DB")]
    pub db: Option<PathBuf>,

    /// Public relay URLs of this deployment (e.g. `https://relay.example.org`).
    /// Advertised at `/v1/info`, and used by the gateway to reach hosts.
    #[arg(long = "relay-url", env = "SCRIN_RELAY_URLS", value_delimiter = ',')]
    pub relay_urls: Vec<String>,

    /// Let unregistered devices use the relay. Never on a public instance.
    #[arg(long, env = "SCRIN_RELAY_OPEN")]
    pub relay_open: bool,

    /// Per-client relay receive limit in bytes/second (0 = unlimited).
    #[arg(long, env = "SCRIN_RELAY_BPS", default_value_t = 0)]
    pub relay_bps: u32,

    /// Take the client IP from `X-Forwarded-For` (only behind a trusted proxy).
    #[arg(long, env = "SCRIN_TRUST_FORWARDED")]
    pub trust_forwarded: bool,

    /// Presence time-to-live in seconds.
    #[arg(long, env = "SCRIN_PRESENCE_TTL", default_value_t = 60)]
    pub presence_ttl: u64,

    /// Lifetime of a passphrase locator in seconds (`POST /v1/locator`).
    #[arg(long, env = "SCRIN_LOCATOR_TTL", default_value_t = 600)]
    pub locator_ttl: u64,

    /// Distinct reporters needed to block a device key.
    #[arg(long, env = "SCRIN_ABUSE_BLOCK_THRESHOLD", default_value_t = 3)]
    pub abuse_block_threshold: u64,

    /// Maximum gateway session length in seconds (anonymous cap, ADR-0009).
    #[arg(long, env = "SCRIN_GW_MAX_SECS", default_value_t = 3600)]
    pub gw_max_secs: u64,

    /// Close a gateway session after this many seconds without traffic.
    #[arg(long, env = "SCRIN_GW_IDLE_SECS", default_value_t = 120)]
    pub gw_idle_secs: u64,

    /// Per-session gateway bandwidth cap in bytes/second, both directions (0 = unlimited).
    #[arg(long, env = "SCRIN_GW_MAX_BPS", default_value_t = 3_000_000)]
    pub gw_max_bps: u64,

    /// Per-session total byte cap (0 = unlimited).
    #[arg(long, env = "SCRIN_GW_MAX_BYTES", default_value_t = 0)]
    pub gw_max_bytes: u64,

    /// Local socket for the gateway's iroh endpoint (default: any port, all interfaces).
    #[arg(long, env = "SCRIN_GW_IROH_BIND")]
    pub gw_iroh_bind: Option<SocketAddr>,

    /// `tracing` filter, e.g. `info,scrin_server=debug`.
    #[arg(long, env = "SCRIN_LOG", default_value = "info")]
    pub log: String,
}

impl Config {
    #[must_use]
    pub fn has(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }

    #[must_use]
    pub fn tls_mode(&self) -> TlsMode {
        self.tls.unwrap_or(if self.dev {
            TlsMode::SelfSigned
        } else {
            TlsMode::Acme
        })
    }

    #[must_use]
    pub fn tcp_addr(&self) -> SocketAddr {
        self.listen.unwrap_or_else(|| {
            if self.dev {
                SocketAddr::from(([127, 0, 0, 1], 4433))
            } else {
                SocketAddr::from(([0, 0, 0, 0], 443))
            }
        })
    }

    #[must_use]
    pub fn udp_addr(&self) -> SocketAddr {
        self.wt_listen.unwrap_or_else(|| self.tcp_addr())
    }

    /// Defaults for an in-process test server: everything on 127.0.0.1:0,
    /// plain HTTP on TCP, relay access control enforced (not dev mode).
    #[must_use]
    pub fn for_tests() -> Self {
        let mut c = Self::parse_from(["scrin-server"]);
        c.listen = Some(SocketAddr::from(([127, 0, 0, 1], 0)));
        c.wt_listen = Some(SocketAddr::from(([127, 0, 0, 1], 0)));
        c.gw_iroh_bind = Some(SocketAddr::from(([127, 0, 0, 1], 0)));
        c.tls = Some(TlsMode::None);
        c.relay_open = false;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_run_every_role_with_acme_on_443() {
        let c = Config::parse_from(["scrin-server"]);
        assert!(c.has(Role::Rendezvous) && c.has(Role::Relay) && c.has(Role::Gateway));
        assert_eq!(c.tls_mode(), TlsMode::Acme);
        assert_eq!(c.tcp_addr().port(), 443);
        assert_eq!(c.udp_addr().port(), 443);
    }

    #[test]
    fn dev_mode_is_local_and_self_signed() {
        let c = Config::parse_from(["scrin-server", "--dev", "--roles", "rendezvous,gateway"]);
        assert_eq!(c.tls_mode(), TlsMode::SelfSigned);
        assert!(c.tcp_addr().ip().is_loopback());
        assert!(!c.has(Role::Relay));
    }
}
