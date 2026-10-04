//! Pure session state machine and permission model for scrin.
//!
//! No IO, no async, no system clock: every call takes `now` in milliseconds and returns a
//! `Vec` of actions for the platform shell to execute. The host machine ([`HostSession`])
//! enforces the anti-scam [`Policy`]: granted permissions are always a subset of what the
//! policy allows for the [`SessionKind`]. The controller machine ([`ControllerSession`])
//! drives dial → pair → wait for accept → active → ended.

mod controller;
mod host;
mod permissions;
mod policy;
mod reason;

pub use controller::{
    ControllerAction, ControllerConfig, ControllerEnd, ControllerEvent, ControllerSession,
    ControllerState, ControllerStatus,
};
pub use host::{HostAction, HostEvent, HostSession, HostState};
pub use permissions::{Permission, Permissions};
pub use policy::{ANONYMOUS_MAX_DURATION_MS, PeerId, Policy, SessionKind};
pub use reason::{EndReason, RejectReason, SessionLog};
