//! Native host and client engine for scrin desktop.
//!
//! One [`Engine`](engine) actor plays both roles on one iroh endpoint:
//!
//! - **Host**: shows an ID + one-time code, pairs incoming controllers with
//!   SPAKE2 (or authenticates trusted ones by signature), drives the
//!   [`scrin_session::HostSession`] anti-scam state machine, then streams
//!   capture → encode → FEC shards → QUIC datagrams and injects input.
//! - **Controller**: resolves a scrin ID or ticket, dials, pairs, drives
//!   [`scrin_session::ControllerSession`], reassembles and decodes video and
//!   sends input and bandwidth feedback.
//!
//! The UI talks to it with [`Command`]s through an [`EngineHandle`] and
//! receives [`Event`]s. Media goes through the [`MediaBackend`] trait
//! (`scrin-win` on Windows, [`SyntheticBackend`] for tests).

pub mod api;
pub mod backend;
mod engine;
mod error;
mod media;
pub mod resolve;
pub mod secret;
#[cfg(all(windows, feature = "win"))]
pub mod win_backend;
mod wire;

pub use api::{Command, Event, Quality, Reply, Role, SessionId, SessionState, StatsInfo, Status};
pub use backend::{InputEvent, MediaBackend, NullBackend, SyntheticBackend};
pub use engine::{EngineConfig, EngineHandle, provisional_scrin_id, start};
pub use error::{EngineError, Result};
pub use scrin_net::{NetConfig, RelayConfig};
pub use scrin_session::Policy;
