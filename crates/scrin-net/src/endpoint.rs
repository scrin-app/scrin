//! The iroh endpoint, keyed by the scrin device identity.
//!
//! The iroh secret key IS the Ed25519 identity seed, so a [`DeviceId`] and the
//! iroh `EndpointId` are the same 32 bytes. That is what lets the handshake
//! take the peer's identity from the authenticated QUIC connection instead of
//! trusting anything the peer says about itself.

use std::net::{Ipv4Addr, SocketAddr};

use iroh::endpoint::{Connection, Incoming, presets};
use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, SecretKey};
use scrin_crypto::identity::DeviceId;
use zeroize::Zeroize;

use crate::{ALPN, NetError, Result};

/// Which relays the endpoint may use when no direct path exists.
#[derive(Debug, Clone, Default)]
pub enum RelayConfig {
    /// n0's public relays and DNS address lookup. Development only.
    #[default]
    Default,
    /// Self-hosted relays. No n0 address lookup: peers are dialled by ticket.
    Custom(Vec<RelayUrl>),
    /// No relays and no address lookup: direct addresses only (tests, LAN).
    Disabled,
}

#[derive(Debug, Clone, Default)]
pub struct NetConfig {
    pub relay: RelayConfig,
    /// Bind only this socket instead of `0.0.0.0:0` + `[::]:0`.
    pub bind_addr: Option<SocketAddr>,
}

impl NetConfig {
    /// Loopback only, relays disabled. For tests: no internet, no firewall prompt.
    #[must_use]
    pub fn loopback() -> Self {
        Self {
            relay: RelayConfig::Disabled,
            bind_addr: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))),
        }
    }
}

/// A bound endpoint speaking ALPN [`ALPN`].
#[derive(Debug, Clone)]
pub struct NetEndpoint {
    endpoint: Endpoint,
    device_id: DeviceId,
}

impl NetEndpoint {
    /// Binds an endpoint whose key is the scrin identity `identity_seed`.
    pub async fn bind(identity_seed: [u8; 32], config: NetConfig) -> Result<Self> {
        let mut seed = identity_seed;
        let secret = SecretKey::from_bytes(&seed);
        seed.zeroize();

        let builder = match config.relay {
            RelayConfig::Default => Endpoint::builder(presets::N0),
            RelayConfig::Custom(urls) => {
                if urls.is_empty() {
                    return Err(NetError::Config("custom relay list is empty"));
                }
                Endpoint::builder(presets::Minimal).relay_mode(RelayMode::custom(urls))
            }
            RelayConfig::Disabled => Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Disabled)
                .clear_address_lookup(),
        };
        let mut builder = builder.secret_key(secret).alpns(vec![ALPN.to_vec()]);
        if let Some(addr) = config.bind_addr {
            builder = builder
                .clear_ip_transports()
                .bind_addr(addr)
                .map_err(|e| NetError::Bind(e.to_string()))?;
        }
        let endpoint = builder
            .bind()
            .await
            .map_err(|e| NetError::Bind(e.to_string()))?;
        let device_id = DeviceId(*endpoint.id().as_bytes());
        Ok(Self {
            endpoint,
            device_id,
        })
    }

    #[must_use]
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Current address (id + known direct addresses + home relay) for a ticket.
    #[must_use]
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    /// Sockets actually bound; with [`NetConfig::loopback`] this is dialable.
    #[must_use]
    pub fn bound_sockets(&self) -> Vec<SocketAddr> {
        self.endpoint.bound_sockets()
    }

    /// Escape hatch for features this wrapper does not cover yet.
    #[must_use]
    pub fn inner(&self) -> &Endpoint {
        &self.endpoint
    }

    pub async fn connect(&self, addr: impl Into<EndpointAddr>) -> Result<Connection> {
        self.endpoint
            .connect(addr, ALPN)
            .await
            .map_err(|e| NetError::Connect(e.to_string()))
    }

    /// Next incoming connection, before the TLS handshake. `None` once closed.
    pub async fn accept(&self) -> Option<Incoming> {
        self.endpoint.accept().await
    }

    /// Next incoming connection with the QUIC handshake completed.
    pub async fn accept_connection(&self) -> Option<Result<Connection>> {
        let incoming = self.endpoint.accept().await?;
        Some(match incoming.accept() {
            Ok(accepting) => accepting
                .await
                .map_err(|e| NetError::Connection(e.to_string())),
            Err(e) => Err(NetError::Connection(e.to_string())),
        })
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

/// The authenticated identity of the remote end of `conn`.
#[must_use]
pub fn remote_device_id(conn: &Connection) -> DeviceId {
    DeviceId(*conn.remote_id().as_bytes())
}
