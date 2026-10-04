//! Controller-side session state machine: dial, pair, wait for the host, run, end.

use crate::permissions::{Permission, Permissions};
use crate::reason::{EndReason, RejectReason};

/// Why a controller session ended, from the controller's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControllerEnd {
    /// The local user cancelled or stopped.
    Cancelled,
    /// The host refused the request.
    Rejected(RejectReason),
    /// The host (or network) ended an accepted session.
    Ended(EndReason),
    /// The connection could not be established or dropped before acceptance.
    ConnectFailed,
    /// SPAKE2 / SAS pairing failed (wrong code or mismatch).
    PairingFailed,
    /// A local timeout expired (connect, pairing or waiting for accept).
    Timeout,
}

/// Input to [`ControllerSession::handle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerEvent {
    /// UI: connect to a host asking for `requested`.
    Connect {
        /// Permissions to ask for.
        requested: Permissions,
    },
    /// Network: transport connection established.
    Connected,
    /// Network: pairing finished; `sas` is the short authentication string to show.
    Paired {
        /// Short authentication string (e.g. emoji) for verbal verification.
        sas: String,
    },
    /// Network: pairing failed.
    PairingFailed,
    /// Network: the host accepted with these permissions.
    Accepted(Permissions),
    /// Network: the host rejected the request.
    Rejected(RejectReason),
    /// Network: the host changed the granted permissions.
    PermissionsChanged(Permissions),
    /// Network: the host ended the session.
    Ended(EndReason),
    /// Network: the connection dropped.
    Disconnected,
    /// UI: cancel / stop.
    Cancel,
    /// UI: ask the host for an extra permission.
    RequestPermission(Permission),
    /// Time passed; deadlines are checked.
    Tick,
}

/// Output of [`ControllerSession::handle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerAction {
    /// Open the transport connection.
    Dial,
    /// Start the PAKE handshake.
    StartPairing,
    /// Show the SAS to the user.
    ShowSas(String),
    /// Send the session request to the host.
    SendRequest(Permissions),
    /// Ask the host for one more permission.
    SendPermissionRequest(Permission),
    /// Show the current status line.
    ShowStatus(ControllerStatus),
    /// Show the currently granted permissions (enables/disables UI features).
    ShowPermissions(Permissions),
    /// The requested permission is not granted (yet); UI shows "asked host".
    PermissionPending(Permission),
    /// Close the connection.
    Disconnect,
    /// Show why it ended.
    ShowEnded(ControllerEnd),
}

/// Coarse status for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControllerStatus {
    /// Dialing the host.
    Connecting,
    /// Running the pairing handshake.
    Pairing,
    /// Waiting for the person at the host to accept.
    AwaitingAccept,
    /// Session running.
    Active,
}

/// Controller state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerState {
    /// Nothing happening.
    Idle,
    /// Dialing.
    Connecting {
        /// Requested permissions.
        requested: Permissions,
        /// Millis when this phase started.
        since: u64,
    },
    /// Pairing handshake running.
    Pairing {
        /// Requested permissions.
        requested: Permissions,
        /// Millis when this phase started.
        since: u64,
    },
    /// Request sent, waiting for the host user.
    AwaitingAccept {
        /// Requested permissions.
        requested: Permissions,
        /// SAS shown to the user.
        sas: String,
        /// Millis when this phase started.
        since: u64,
    },
    /// Session running.
    Active {
        /// Permissions granted by the host.
        granted: Permissions,
        /// Millis when the session started.
        started_at: u64,
    },
    /// Over; a new `Connect` may start again.
    Ended(ControllerEnd),
}

/// Controller timeouts in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerConfig {
    /// Max time to establish the connection.
    pub connect_timeout_ms: u64,
    /// Max time for pairing.
    pub pairing_timeout_ms: u64,
    /// Max time to wait for the host to answer (host auto-rejects at 60 s).
    pub accept_timeout_ms: u64,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 30_000,
            pairing_timeout_ms: 30_000,
            accept_timeout_ms: 75_000,
        }
    }
}

/// Pure controller session state machine; pass `now` (millis) to every call.
#[derive(Debug, Clone)]
pub struct ControllerSession {
    config: ControllerConfig,
    state: ControllerState,
}

impl Default for ControllerSession {
    fn default() -> Self {
        Self::new(ControllerConfig::default())
    }
}

impl ControllerSession {
    /// New idle controller.
    pub const fn new(config: ControllerConfig) -> Self {
        Self {
            config,
            state: ControllerState::Idle,
        }
    }

    /// Current state.
    pub const fn state(&self) -> &ControllerState {
        &self.state
    }

    /// Granted permissions (empty unless active).
    pub const fn granted(&self) -> Permissions {
        match &self.state {
            ControllerState::Active { granted, .. } => *granted,
            _ => Permissions::empty(),
        }
    }

    /// Shorthand for `handle(now, ControllerEvent::Tick)`.
    pub fn on_tick(&mut self, now: u64) -> Vec<ControllerAction> {
        self.handle(now, ControllerEvent::Tick)
    }

    /// Feed one event at time `now`; returns the actions to perform.
    pub fn handle(&mut self, now: u64, event: ControllerEvent) -> Vec<ControllerAction> {
        let mut out = Vec::new();
        self.check_deadline(now, &mut out);
        let state = std::mem::replace(&mut self.state, ControllerState::Idle);
        self.state = Self::step(now, state, event, &mut out);
        out
    }

    fn check_deadline(&mut self, now: u64, out: &mut Vec<ControllerAction>) {
        let c = self.config;
        let expired = match &self.state {
            ControllerState::Connecting { since, .. } => since.saturating_add(c.connect_timeout_ms),
            ControllerState::Pairing { since, .. } => since.saturating_add(c.pairing_timeout_ms),
            ControllerState::AwaitingAccept { since, .. } => {
                since.saturating_add(c.accept_timeout_ms)
            }
            _ => return,
        } <= now;
        if expired {
            self.state = end(ControllerEnd::Timeout, true, out);
        }
    }

    fn step(
        now: u64,
        state: ControllerState,
        event: ControllerEvent,
        out: &mut Vec<ControllerAction>,
    ) -> ControllerState {
        use ControllerEvent as E;
        use ControllerState as S;
        match (state, event) {
            (S::Idle | S::Ended(_), E::Connect { requested }) => {
                out.push(ControllerAction::Dial);
                out.push(ControllerAction::ShowStatus(ControllerStatus::Connecting));
                S::Connecting {
                    requested,
                    since: now,
                }
            }
            (S::Connecting { requested, .. }, E::Connected) => {
                out.push(ControllerAction::StartPairing);
                out.push(ControllerAction::ShowStatus(ControllerStatus::Pairing));
                S::Pairing {
                    requested,
                    since: now,
                }
            }
            (S::Pairing { requested, .. }, E::Paired { sas }) => {
                out.push(ControllerAction::ShowSas(sas.clone()));
                out.push(ControllerAction::SendRequest(requested));
                out.push(ControllerAction::ShowStatus(
                    ControllerStatus::AwaitingAccept,
                ));
                S::AwaitingAccept {
                    requested,
                    sas,
                    since: now,
                }
            }
            (S::Pairing { .. }, E::PairingFailed) => end(ControllerEnd::PairingFailed, true, out),
            (S::AwaitingAccept { .. }, E::Accepted(granted)) => {
                out.push(ControllerAction::ShowStatus(ControllerStatus::Active));
                out.push(ControllerAction::ShowPermissions(granted));
                S::Active {
                    granted,
                    started_at: now,
                }
            }
            (S::AwaitingAccept { .. }, E::Rejected(reason)) => {
                end(ControllerEnd::Rejected(reason), true, out)
            }
            (
                S::Active {
                    started_at,
                    granted,
                },
                E::PermissionsChanged(new),
            ) => {
                if new != granted {
                    out.push(ControllerAction::ShowPermissions(new));
                }
                S::Active {
                    granted: new,
                    started_at,
                }
            }
            (
                S::Active {
                    started_at,
                    granted,
                },
                E::RequestPermission(perm),
            ) => {
                if !granted.contains(perm) {
                    out.push(ControllerAction::SendPermissionRequest(perm));
                    out.push(ControllerAction::PermissionPending(perm));
                }
                S::Active {
                    granted,
                    started_at,
                }
            }
            (S::Active { .. }, E::Ended(reason)) => end(ControllerEnd::Ended(reason), true, out),
            (S::Active { .. }, E::Disconnected) => end(
                ControllerEnd::Ended(EndReason::PeerDisconnected),
                false,
                out,
            ),
            (
                S::Connecting { .. } | S::Pairing { .. } | S::AwaitingAccept { .. },
                E::Disconnected | E::Ended(_),
            ) => end(ControllerEnd::ConnectFailed, false, out),
            (
                S::Connecting { .. }
                | S::Pairing { .. }
                | S::AwaitingAccept { .. }
                | S::Active { .. },
                E::Cancel,
            ) => end(ControllerEnd::Cancelled, true, out),
            (state, _) => state,
        }
    }
}

fn end(
    reason: ControllerEnd,
    disconnect: bool,
    out: &mut Vec<ControllerAction>,
) -> ControllerState {
    if disconnect {
        out.push(ControllerAction::Disconnect);
    }
    out.push(ControllerAction::ShowEnded(reason));
    ControllerState::Ended(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [ControllerAction; 0] = [];
    use ControllerAction as A;
    use ControllerEvent as E;

    fn awaiting() -> ControllerSession {
        let mut c = ControllerSession::default();
        c.handle(
            0,
            E::Connect {
                requested: Permissions::support(),
            },
        );
        c.handle(10, E::Connected);
        c.handle(
            20,
            E::Paired {
                sas: "🐱🌵🚀🍋🎲".into(),
            },
        );
        c
    }

    fn active() -> ControllerSession {
        let mut c = awaiting();
        c.handle(30, E::Accepted(Permissions::support()));
        c
    }

    #[test]
    fn happy_path_reaches_active_with_granted_permissions() {
        let mut c = ControllerSession::default();
        assert_eq!(
            c.handle(
                0,
                E::Connect {
                    requested: Permissions::support()
                }
            ),
            vec![A::Dial, A::ShowStatus(ControllerStatus::Connecting)]
        );
        assert_eq!(
            c.handle(10, E::Connected),
            vec![A::StartPairing, A::ShowStatus(ControllerStatus::Pairing)]
        );
        assert_eq!(
            c.handle(20, E::Paired { sas: "abc".into() }),
            vec![
                A::ShowSas("abc".into()),
                A::SendRequest(Permissions::support()),
                A::ShowStatus(ControllerStatus::AwaitingAccept),
            ]
        );
        let granted = Permission::View | Permission::Input;
        assert_eq!(
            c.handle(30, E::Accepted(granted)),
            vec![
                A::ShowStatus(ControllerStatus::Active),
                A::ShowPermissions(granted)
            ]
        );
        assert_eq!(
            c.state(),
            &ControllerState::Active {
                granted,
                started_at: 30
            }
        );
        assert_eq!(c.granted(), granted);
    }

    #[test]
    fn rejection_ends_with_reason() {
        let mut c = awaiting();
        let out = c.handle(40, E::Rejected(RejectReason::UserRejected));
        assert_eq!(
            out,
            vec![
                A::Disconnect,
                A::ShowEnded(ControllerEnd::Rejected(RejectReason::UserRejected))
            ]
        );
        assert_eq!(
            c.state(),
            &ControllerState::Ended(ControllerEnd::Rejected(RejectReason::UserRejected))
        );
    }

    #[test]
    fn pairing_failure_ends_session() {
        let mut c = ControllerSession::default();
        c.handle(
            0,
            E::Connect {
                requested: Permissions::view_only(),
            },
        );
        c.handle(1, E::Connected);
        let out = c.handle(2, E::PairingFailed);
        assert_eq!(
            out,
            vec![A::Disconnect, A::ShowEnded(ControllerEnd::PairingFailed)]
        );
    }

    #[test]
    fn waiting_for_accept_times_out() {
        let mut c = awaiting();
        assert_eq!(c.on_tick(20 + 74_999), NONE);
        let out = c.on_tick(20 + 75_000);
        assert_eq!(
            out,
            vec![A::Disconnect, A::ShowEnded(ControllerEnd::Timeout)]
        );
    }

    #[test]
    fn connect_timeout_fires_before_late_connected_event() {
        let mut c = ControllerSession::default();
        c.handle(
            0,
            E::Connect {
                requested: Permissions::view_only(),
            },
        );
        let out = c.handle(30_000, E::Connected);
        assert_eq!(
            out,
            vec![A::Disconnect, A::ShowEnded(ControllerEnd::Timeout)]
        );
        assert_eq!(c.state(), &ControllerState::Ended(ControllerEnd::Timeout));
    }

    #[test]
    fn cancel_while_waiting_disconnects() {
        let mut c = awaiting();
        assert_eq!(
            c.handle(25, E::Cancel),
            vec![A::Disconnect, A::ShowEnded(ControllerEnd::Cancelled)]
        );
    }

    #[test]
    fn permissions_changed_updates_ui_only_when_different() {
        let mut c = active();
        assert_eq!(
            c.handle(40, E::PermissionsChanged(Permissions::support())),
            NONE
        );
        let fewer = Permissions::view_only();
        assert_eq!(
            c.handle(50, E::PermissionsChanged(fewer)),
            vec![A::ShowPermissions(fewer)]
        );
        assert_eq!(c.granted(), fewer);
    }

    #[test]
    fn request_permission_sends_only_when_not_granted() {
        let mut c = active();
        assert_eq!(c.handle(40, E::RequestPermission(Permission::View)), NONE);
        assert_eq!(
            c.handle(41, E::RequestPermission(Permission::FilesOut)),
            vec![
                A::SendPermissionRequest(Permission::FilesOut),
                A::PermissionPending(Permission::FilesOut)
            ]
        );
    }

    #[test]
    fn host_end_shows_reason() {
        let mut c = active();
        let out = c.handle(100, E::Ended(EndReason::TimeLimit));
        assert_eq!(
            out,
            vec![
                A::Disconnect,
                A::ShowEnded(ControllerEnd::Ended(EndReason::TimeLimit))
            ]
        );
    }

    #[test]
    fn disconnect_before_accept_is_connect_failed() {
        let mut c = awaiting();
        assert_eq!(
            c.handle(30, E::Disconnected),
            vec![A::ShowEnded(ControllerEnd::ConnectFailed)]
        );
    }

    #[test]
    fn active_session_has_no_local_timeout() {
        let mut c = active();
        assert_eq!(c.on_tick(u64::MAX / 2), NONE);
    }

    #[test]
    fn out_of_order_network_events_are_ignored() {
        let mut c = ControllerSession::default();
        assert_eq!(c.handle(0, E::Accepted(Permissions::full())), NONE);
        assert_eq!(c.handle(0, E::Paired { sas: String::new() }), NONE);
        assert_eq!(c.state(), &ControllerState::Idle);
        let mut c = awaiting();
        assert_eq!(
            c.handle(25, E::PermissionsChanged(Permissions::full())),
            NONE
        );
        assert_eq!(c.granted(), Permissions::empty());
    }

    #[test]
    fn can_reconnect_after_end() {
        let mut c = awaiting();
        c.handle(25, E::Cancel);
        assert_eq!(
            c.handle(
                30,
                E::Connect {
                    requested: Permissions::view_only()
                }
            )[0],
            A::Dial
        );
    }
}
