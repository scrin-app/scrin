//! Host-side session state machine: request dialog, permission grants, limits, end-of-session log.

use crate::permissions::{Permission, Permissions};
use crate::policy::{PeerId, Policy, SessionKind};
use crate::reason::{EndReason, RejectReason, SessionLog};

/// Input to [`HostSession::handle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// A paired controller asks for a session.
    Request {
        /// Controller device.
        peer: PeerId,
        /// How the controller authenticated.
        kind: SessionKind,
        /// Permissions the controller asked for.
        requested: Permissions,
    },
    /// The host user clicked Accept with this permission selection.
    UserAccept(Permissions),
    /// The host user clicked Reject.
    UserReject,
    /// The host user removed a permission live.
    UserRevoke(Permission),
    /// The host user added a permission live.
    UserGrant(Permission),
    /// The host user asked to add the controller to the trust list.
    UserAddTrust,
    /// The controller asked for an extra permission.
    PeerRequestPermission(Permission),
    /// The host user stopped the session (or dismissed the pending request).
    UserStop,
    /// The host user stopped the session and reported the controller.
    StopAndReport,
    /// The controller ended the session cleanly.
    PeerEnded,
    /// The connection to the controller dropped.
    PeerDisconnected,
    /// Bytes moved since the last report (fed by the transport shell).
    Traffic {
        /// Bytes received from the controller.
        bytes_in: u64,
        /// Bytes sent to the controller.
        bytes_out: u64,
    },
    /// Time passed; deadlines are checked.
    Tick,
}

/// Output of [`HostSession::handle`]; the shell executes these in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAction {
    /// Show the pre-accept interstitial.
    ShowRequestDialog {
        /// Controller device.
        peer: PeerId,
        /// Session kind (drives the verified/unverified badge).
        kind: SessionKind,
        /// What the controller asked for.
        requested: Permissions,
        /// What policy allows for this kind; the dialog offers only these.
        allowed: Permissions,
        /// Millis when Accept becomes clickable.
        accept_enabled_at: u64,
        /// Millis when the request is auto-rejected.
        expires_at: u64,
    },
    /// Close the request dialog.
    HideRequestDialog,
    /// Accept the session with these permissions.
    SendAccept(Permissions),
    /// Refuse a request from `peer`.
    SendReject {
        /// Controller to refuse.
        peer: PeerId,
        /// Why.
        reason: RejectReason,
    },
    /// The live permission set changed (or a request for more was refused).
    SendPermissions(Permissions),
    /// Ask the host user whether to grant a permission the controller requested.
    AskGrant(Permission),
    /// A grant was refused because policy forbids it for this session kind.
    PolicyDenied(Permission),
    /// Add the controller to the trust list.
    AddToTrustList(PeerId),
    /// Adding to the trust list was refused by policy.
    TrustDenied,
    /// Update the always-visible session indicator.
    ShowIndicator {
        /// Session is being recorded.
        recording: bool,
        /// Someone is controlling (or viewing) this machine.
        controlled: bool,
        /// Privacy mode (screen blanked locally) is on.
        privacy: bool,
    },
    /// Tear down the session / connection.
    EndSession(EndReason),
    /// Report the controller for abuse.
    ReportAbuse {
        /// Reported controller.
        peer: PeerId,
    },
    /// Show the end-of-session summary.
    Notify(SessionLog),
}

/// Host state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostState {
    /// Waiting for a request.
    Idle,
    /// The request dialog is showing.
    IncomingRequest {
        /// Controller device.
        peer: PeerId,
        /// Session kind.
        kind: SessionKind,
        /// Requested permissions.
        requested: Permissions,
        /// Millis when the request arrived.
        received_at: u64,
    },
    /// A session is running.
    Active {
        /// Controller device.
        peer: PeerId,
        /// Session kind.
        kind: SessionKind,
        /// Currently granted permissions (always within policy).
        granted: Permissions,
        /// Millis when the session started.
        started_at: u64,
        /// Union of every permission granted so far.
        used: Permissions,
        /// Bytes received.
        bytes_in: u64,
        /// Bytes sent.
        bytes_out: u64,
    },
    /// The last request or session is over; a new request may arrive.
    Ended {
        /// Why it ended.
        reason: EndReason,
        /// Activity record, if a session actually ran.
        log: Option<SessionLog>,
    },
}

/// Pure host session state machine. No IO, no clock: pass `now` (millis) to every call.
#[derive(Debug, Clone)]
pub struct HostSession {
    policy: Policy,
    state: HostState,
}

impl HostSession {
    /// New idle session machine under `policy`.
    pub const fn new(policy: Policy) -> Self {
        Self {
            policy,
            state: HostState::Idle,
        }
    }

    /// Current state.
    pub const fn state(&self) -> &HostState {
        &self.state
    }

    /// The policy in force.
    pub const fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Currently granted permissions (empty unless active).
    pub const fn granted(&self) -> Permissions {
        match &self.state {
            HostState::Active { granted, .. } => *granted,
            _ => Permissions::empty(),
        }
    }

    /// Kind of the pending or active session.
    pub const fn kind(&self) -> Option<&SessionKind> {
        match &self.state {
            HostState::IncomingRequest { kind, .. } | HostState::Active { kind, .. } => Some(kind),
            _ => None,
        }
    }

    /// Shorthand for `handle(now, HostEvent::Tick)`.
    pub fn on_tick(&mut self, now: u64) -> Vec<HostAction> {
        self.handle(now, HostEvent::Tick)
    }

    /// Feed one event at time `now`; returns the actions to perform.
    pub fn handle(&mut self, now: u64, event: HostEvent) -> Vec<HostAction> {
        let mut out = Vec::new();
        self.check_deadlines(now, &mut out);
        match event {
            HostEvent::Request {
                peer,
                kind,
                requested,
            } => {
                self.on_request(now, peer, kind, requested, &mut out);
            }
            HostEvent::UserAccept(perms) => self.on_accept(now, perms, &mut out),
            HostEvent::UserReject => self.on_reject(now, RejectReason::UserRejected, &mut out),
            HostEvent::UserRevoke(perm) => self.on_revoke(perm, &mut out),
            HostEvent::UserGrant(perm) => self.on_grant(perm, &mut out),
            HostEvent::UserAddTrust => self.on_add_trust(&mut out),
            HostEvent::PeerRequestPermission(perm) => self.on_peer_request(perm, &mut out),
            HostEvent::UserStop => self.on_stop(now, false, &mut out),
            HostEvent::StopAndReport => self.on_stop(now, true, &mut out),
            HostEvent::PeerEnded => self.on_peer_gone(now, EndReason::PeerEnded, &mut out),
            HostEvent::PeerDisconnected => {
                self.on_peer_gone(now, EndReason::PeerDisconnected, &mut out);
            }
            HostEvent::Traffic {
                bytes_in: i,
                bytes_out: o,
            } => {
                if let HostState::Active {
                    bytes_in,
                    bytes_out,
                    ..
                } = &mut self.state
                {
                    *bytes_in = bytes_in.saturating_add(i);
                    *bytes_out = bytes_out.saturating_add(o);
                }
            }
            HostEvent::Tick => {}
        }
        out
    }

    fn check_deadlines(&mut self, now: u64, out: &mut Vec<HostAction>) {
        match &self.state {
            HostState::IncomingRequest { received_at, .. }
                if now >= received_at.saturating_add(self.policy.request_timeout_ms) =>
            {
                self.on_reject(now, RejectReason::Timeout, out);
            }
            HostState::Active {
                kind, started_at, ..
            } => {
                if let Some(max) = self.policy.max_duration_ms(kind)
                    && now >= started_at.saturating_add(max)
                {
                    self.finish(now, EndReason::TimeLimit, out);
                }
            }
            _ => {}
        }
    }

    fn on_request(
        &mut self,
        now: u64,
        peer: PeerId,
        kind: SessionKind,
        requested: Permissions,
        out: &mut Vec<HostAction>,
    ) {
        if matches!(
            self.state,
            HostState::IncomingRequest { .. } | HostState::Active { .. }
        ) {
            out.push(HostAction::SendReject {
                peer,
                reason: RejectReason::Busy,
            });
            return;
        }
        let allowed = self.policy.allowed(&kind);
        if self.policy.auto_accepts(&kind) {
            // Unattended: nobody is there to consent to recording.
            let grant = (requested & allowed).without(Permission::Record);
            self.start(now, peer, kind, grant, out);
            return;
        }
        out.push(HostAction::ShowRequestDialog {
            peer,
            kind: kind.clone(),
            requested,
            allowed,
            accept_enabled_at: now.saturating_add(self.policy.accept_delay_ms(&kind)),
            expires_at: now.saturating_add(self.policy.request_timeout_ms),
        });
        self.state = HostState::IncomingRequest {
            peer,
            kind,
            requested,
            received_at: now,
        };
    }

    fn on_accept(&mut self, now: u64, perms: Permissions, out: &mut Vec<HostAction>) {
        let HostState::IncomingRequest {
            peer,
            kind,
            received_at,
            ..
        } = &self.state
        else {
            return;
        };
        if now < received_at.saturating_add(self.policy.accept_delay_ms(kind)) {
            return;
        }
        let (peer, kind) = (*peer, kind.clone());
        let allowed = self.policy.allowed(&kind);
        out.extend((perms - allowed).iter().map(HostAction::PolicyDenied));
        out.push(HostAction::HideRequestDialog);
        self.start(now, peer, kind, perms & allowed, out);
    }

    fn start(
        &mut self,
        now: u64,
        peer: PeerId,
        kind: SessionKind,
        granted: Permissions,
        out: &mut Vec<HostAction>,
    ) {
        out.push(HostAction::SendAccept(granted));
        out.push(indicator(granted, true));
        self.state = HostState::Active {
            peer,
            kind,
            granted,
            started_at: now,
            used: granted,
            bytes_in: 0,
            bytes_out: 0,
        };
    }

    fn on_reject(&mut self, _now: u64, reason: RejectReason, out: &mut Vec<HostAction>) {
        let HostState::IncomingRequest { peer, .. } = &self.state else {
            return;
        };
        let peer = *peer;
        out.push(HostAction::HideRequestDialog);
        out.push(HostAction::SendReject { peer, reason });
        if reason == RejectReason::Reported {
            out.push(HostAction::ReportAbuse { peer });
        }
        let end = EndReason::Rejected(reason);
        out.push(HostAction::EndSession(end));
        self.state = HostState::Ended {
            reason: end,
            log: None,
        };
    }

    fn on_revoke(&mut self, perm: Permission, out: &mut Vec<HostAction>) {
        if let HostState::Active { granted, .. } = &mut self.state
            && granted.contains(perm)
        {
            *granted = granted.without(perm);
            out.push(HostAction::SendPermissions(*granted));
            out.push(indicator(*granted, true));
        }
    }

    fn on_grant(&mut self, perm: Permission, out: &mut Vec<HostAction>) {
        let HostState::Active {
            kind,
            granted,
            used,
            ..
        } = &mut self.state
        else {
            return;
        };
        if granted.contains(perm) {
            return;
        }
        if !self.policy.permits(kind, perm) {
            out.push(HostAction::PolicyDenied(perm));
            return;
        }
        *granted = granted.with(perm);
        *used = used.with(perm);
        out.push(HostAction::SendPermissions(*granted));
        out.push(indicator(*granted, true));
    }

    fn on_peer_request(&self, perm: Permission, out: &mut Vec<HostAction>) {
        let HostState::Active { kind, granted, .. } = &self.state else {
            return;
        };
        if granted.contains(perm) {
            return;
        }
        if self.policy.permits(kind, perm) {
            out.push(HostAction::AskGrant(perm));
        } else {
            out.push(HostAction::PolicyDenied(perm));
            out.push(HostAction::SendPermissions(*granted));
        }
    }

    fn on_add_trust(&self, out: &mut Vec<HostAction>) {
        if let HostState::Active { peer, kind, .. } = &self.state {
            out.push(if self.policy.can_add_trust(kind) {
                HostAction::AddToTrustList(*peer)
            } else {
                HostAction::TrustDenied
            });
        }
    }

    fn on_stop(&mut self, now: u64, report: bool, out: &mut Vec<HostAction>) {
        match &self.state {
            HostState::IncomingRequest { .. } => {
                let reason = if report {
                    RejectReason::Reported
                } else {
                    RejectReason::UserRejected
                };
                self.on_reject(now, reason, out);
            }
            HostState::Active { peer, .. } => {
                if report {
                    out.push(HostAction::ReportAbuse { peer: *peer });
                }
                let reason = if report {
                    EndReason::Reported
                } else {
                    EndReason::HostStopped
                };
                self.finish(now, reason, out);
            }
            HostState::Idle | HostState::Ended { .. } => {}
        }
    }

    fn on_peer_gone(&mut self, now: u64, reason: EndReason, out: &mut Vec<HostAction>) {
        match &self.state {
            HostState::IncomingRequest { .. } => {
                out.push(HostAction::HideRequestDialog);
                out.push(HostAction::EndSession(reason));
                self.state = HostState::Ended { reason, log: None };
            }
            HostState::Active { .. } => self.finish(now, reason, out),
            HostState::Idle | HostState::Ended { .. } => {}
        }
    }

    fn finish(&mut self, now: u64, reason: EndReason, out: &mut Vec<HostAction>) {
        let HostState::Active {
            peer,
            kind,
            started_at,
            used,
            bytes_in,
            bytes_out,
            ..
        } = &self.state
        else {
            return;
        };
        let log = SessionLog {
            peer: *peer,
            kind: kind.clone(),
            started_at: *started_at,
            ended_at: now,
            end_reason: reason,
            permissions_used: *used,
            bytes_in: *bytes_in,
            bytes_out: *bytes_out,
        };
        out.push(HostAction::EndSession(reason));
        out.push(indicator(Permissions::empty(), false));
        out.push(HostAction::Notify(log.clone()));
        self.state = HostState::Ended {
            reason,
            log: Some(log),
        };
    }
}

const fn indicator(granted: Permissions, controlled: bool) -> HostAction {
    HostAction::ShowIndicator {
        recording: granted.contains(Permission::Record),
        controlled,
        privacy: granted.contains(Permission::PrivacyMode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [HostAction; 0] = [];
    use crate::policy::ANONYMOUS_MAX_DURATION_MS;

    const PEER: PeerId = PeerId([7; 32]);

    fn host() -> HostSession {
        HostSession::new(Policy::default())
    }

    fn request(kind: SessionKind, requested: Permissions) -> HostEvent {
        HostEvent::Request {
            peer: PEER,
            kind,
            requested,
        }
    }

    fn verified() -> SessionKind {
        SessionKind::Verified {
            org: None,
            name: "Ana".into(),
        }
    }

    /// Anonymous request at t=0, accepted at t=5000 with `perms`.
    fn active_anonymous(perms: Permissions) -> HostSession {
        let mut h = host();
        h.handle(0, request(SessionKind::Anonymous, perms));
        h.handle(5_000, HostEvent::UserAccept(perms));
        assert!(matches!(h.state(), HostState::Active { .. }));
        h
    }

    fn active_verified(perms: Permissions) -> HostSession {
        let mut h = host();
        h.handle(0, request(verified(), perms));
        h.handle(1, HostEvent::UserAccept(perms));
        h
    }

    #[test]
    fn request_shows_dialog_with_five_second_accept_delay_for_anonymous() {
        let mut h = host();
        let out = h.handle(1_000, request(SessionKind::Anonymous, Permissions::full()));
        assert_eq!(
            out,
            vec![HostAction::ShowRequestDialog {
                peer: PEER,
                kind: SessionKind::Anonymous,
                requested: Permissions::full(),
                allowed: Policy::default().allowed(&SessionKind::Anonymous),
                accept_enabled_at: 6_000,
                expires_at: 61_000,
            }]
        );
        assert!(matches!(
            h.state(),
            HostState::IncomingRequest {
                received_at: 1_000,
                ..
            }
        ));
    }

    #[test]
    fn verified_request_has_no_accept_delay() {
        let mut h = host();
        let out = h.handle(1_000, request(verified(), Permissions::support()));
        assert!(matches!(
            out.as_slice(),
            [HostAction::ShowRequestDialog {
                accept_enabled_at: 1_000,
                ..
            }]
        ));
    }

    #[test]
    fn accept_before_delay_is_ignored() {
        let mut h = host();
        h.handle(0, request(SessionKind::Anonymous, Permissions::support()));
        let out = h.handle(4_999, HostEvent::UserAccept(Permissions::support()));
        assert_eq!(out, NONE);
        assert!(matches!(h.state(), HostState::IncomingRequest { .. }));
        let out = h.handle(5_000, HostEvent::UserAccept(Permissions::support()));
        assert!(out.contains(&HostAction::SendAccept(Permissions::support())));
    }

    #[test]
    fn auto_reject_after_sixty_second_timeout() {
        let mut h = host();
        h.handle(0, request(SessionKind::Anonymous, Permissions::support()));
        assert_eq!(h.on_tick(59_999), NONE);
        let out = h.on_tick(60_000);
        assert_eq!(
            out,
            vec![
                HostAction::HideRequestDialog,
                HostAction::SendReject {
                    peer: PEER,
                    reason: RejectReason::Timeout
                },
                HostAction::EndSession(EndReason::Rejected(RejectReason::Timeout)),
            ]
        );
        assert_eq!(
            h.state(),
            &HostState::Ended {
                reason: EndReason::Rejected(RejectReason::Timeout),
                log: None
            }
        );
    }

    #[test]
    fn accept_after_timeout_is_too_late() {
        let mut h = host();
        h.handle(0, request(SessionKind::Anonymous, Permissions::support()));
        let out = h.handle(70_000, HostEvent::UserAccept(Permissions::support()));
        assert!(out.contains(&HostAction::SendReject {
            peer: PEER,
            reason: RejectReason::Timeout
        }));
        assert!(!out.iter().any(|a| matches!(a, HostAction::SendAccept(_))));
        assert_eq!(h.granted(), Permissions::empty());
    }

    #[test]
    fn user_reject_sends_reject_and_ends() {
        let mut h = host();
        h.handle(0, request(verified(), Permissions::support()));
        let out = h.handle(10, HostEvent::UserReject);
        assert!(out.contains(&HostAction::SendReject {
            peer: PEER,
            reason: RejectReason::UserRejected
        }));
        assert!(matches!(h.state(), HostState::Ended { log: None, .. }));
    }

    #[test]
    fn anonymous_cannot_get_files_in_even_if_user_tries() {
        let mut h = host();
        let wanted = Permissions::support() | Permission::FilesIn;
        h.handle(0, request(SessionKind::Anonymous, wanted));
        let out = h.handle(5_000, HostEvent::UserAccept(wanted));
        assert!(out.contains(&HostAction::PolicyDenied(Permission::FilesIn)));
        assert!(out.contains(&HostAction::SendAccept(Permissions::support())));
        assert!(!h.granted().contains(Permission::FilesIn));

        let out = h.handle(6_000, HostEvent::UserGrant(Permission::FilesIn));
        assert_eq!(out, vec![HostAction::PolicyDenied(Permission::FilesIn)]);
        assert!(!h.granted().contains(Permission::FilesIn));
    }

    #[test]
    fn anonymous_is_denied_every_forbidden_permission_on_live_grant() {
        let mut h = active_anonymous(Permissions::view_only());
        for perm in Policy::default().anonymous_forbidden.iter() {
            assert_eq!(
                h.handle(6_000, HostEvent::UserGrant(perm)),
                vec![HostAction::PolicyDenied(perm)]
            );
        }
        assert_eq!(h.granted(), Permissions::view_only());
    }

    #[test]
    fn trusted_can_get_privacy_mode() {
        let mut h = host();
        let out = h.handle(
            0,
            request(SessionKind::Trusted, Permission::View | Permission::Input),
        );
        assert!(out.contains(&HostAction::SendAccept(
            Permission::View | Permission::Input
        )));
        let out = h.handle(1, HostEvent::UserGrant(Permission::PrivacyMode));
        assert!(h.granted().contains(Permission::PrivacyMode));
        assert!(out.contains(&HostAction::ShowIndicator {
            recording: false,
            controlled: true,
            privacy: true
        }));
    }

    #[test]
    fn verified_cannot_get_privacy_mode() {
        let mut h = active_verified(Permissions::support());
        assert_eq!(
            h.handle(2, HostEvent::UserGrant(Permission::PrivacyMode)),
            vec![HostAction::PolicyDenied(Permission::PrivacyMode)]
        );
    }

    #[test]
    fn trusted_request_auto_accepts_without_dialog_and_without_record() {
        let mut h = host();
        let out = h.handle(0, request(SessionKind::Trusted, Permissions::full()));
        let expected = Permissions::full().without(Permission::Record);
        assert_eq!(out[0], HostAction::SendAccept(expected));
        assert!(
            !out.iter()
                .any(|a| matches!(a, HostAction::ShowRequestDialog { .. }))
        );
        assert_eq!(h.granted(), expected);
    }

    #[test]
    fn trusted_request_shows_dialog_when_auto_accept_disabled() {
        let mut h = HostSession::new(Policy {
            unattended_auto_accept: false,
            ..Policy::default()
        });
        let out = h.handle(0, request(SessionKind::Trusted, Permissions::support()));
        assert!(matches!(
            out.as_slice(),
            [HostAction::ShowRequestDialog { .. }]
        ));
    }

    #[test]
    fn org_policy_ceiling_limits_auto_accept() {
        let mut h = HostSession::new(Policy {
            org_allowed: Permissions::view_only(),
            ..Policy::default()
        });
        h.handle(0, request(SessionKind::OrgPolicy, Permissions::full()));
        assert_eq!(h.granted(), Permissions::view_only());
    }

    #[test]
    fn record_shows_recording_indicator_when_user_grants_it() {
        let mut h = active_verified(Permissions::view_only());
        let out = h.handle(2, HostEvent::UserGrant(Permission::Record));
        assert!(out.contains(&HostAction::ShowIndicator {
            recording: true,
            controlled: true,
            privacy: false
        }));
    }

    #[test]
    fn peer_request_for_record_asks_host_instead_of_granting() {
        let mut h = active_verified(Permissions::view_only());
        let out = h.handle(2, HostEvent::PeerRequestPermission(Permission::Record));
        assert_eq!(out, vec![HostAction::AskGrant(Permission::Record)]);
        assert!(!h.granted().contains(Permission::Record));
    }

    #[test]
    fn peer_request_for_forbidden_permission_is_policy_denied() {
        let mut h = active_anonymous(Permissions::view_only());
        let out = h.handle(
            6_000,
            HostEvent::PeerRequestPermission(Permission::Terminal),
        );
        assert_eq!(
            out,
            vec![
                HostAction::PolicyDenied(Permission::Terminal),
                HostAction::SendPermissions(Permissions::view_only()),
            ]
        );
    }

    #[test]
    fn peer_request_for_already_granted_permission_is_noop() {
        let mut h = active_verified(Permissions::support());
        assert_eq!(
            h.handle(2, HostEvent::PeerRequestPermission(Permission::Input)),
            NONE
        );
    }

    #[test]
    fn max_duration_ends_anonymous_session_with_time_limit() {
        let mut h = active_anonymous(Permissions::support());
        let deadline = 5_000 + ANONYMOUS_MAX_DURATION_MS;
        assert_eq!(h.on_tick(deadline - 1), NONE);
        let out = h.on_tick(deadline);
        assert_eq!(out[0], HostAction::EndSession(EndReason::TimeLimit));
        let HostState::Ended {
            reason,
            log: Some(log),
        } = h.state()
        else {
            panic!("expected ended with log, got {:?}", h.state());
        };
        assert_eq!(*reason, EndReason::TimeLimit);
        assert_eq!(log.duration_ms(), ANONYMOUS_MAX_DURATION_MS);
    }

    #[test]
    fn verified_session_has_no_time_limit_by_default() {
        let mut h = active_verified(Permissions::support());
        assert_eq!(h.on_tick(10 * ANONYMOUS_MAX_DURATION_MS), NONE);
        assert!(matches!(h.state(), HostState::Active { .. }));
    }

    #[test]
    fn revoke_removes_permission_and_emits_send_permissions() {
        let mut h = active_verified(Permissions::support());
        let out = h.handle(2, HostEvent::UserRevoke(Permission::Input));
        let expected = Permissions::support().without(Permission::Input);
        assert_eq!(out[0], HostAction::SendPermissions(expected));
        assert_eq!(h.granted(), expected);
        assert_eq!(h.handle(3, HostEvent::UserRevoke(Permission::Input)), NONE);
    }

    #[test]
    fn grant_adds_permission_and_is_idempotent() {
        let mut h = active_verified(Permissions::view_only());
        let out = h.handle(2, HostEvent::UserGrant(Permission::FilesOut));
        assert_eq!(
            out[0],
            HostAction::SendPermissions(Permission::View | Permission::FilesOut)
        );
        assert_eq!(
            h.handle(3, HostEvent::UserGrant(Permission::FilesOut)),
            NONE
        );
    }

    #[test]
    fn stop_and_report_emits_report_abuse_and_end_session() {
        let mut h = active_anonymous(Permissions::support());
        let out = h.handle(9_000, HostEvent::StopAndReport);
        assert_eq!(out[0], HostAction::ReportAbuse { peer: PEER });
        assert_eq!(out[1], HostAction::EndSession(EndReason::Reported));
        assert!(matches!(out.last(), Some(HostAction::Notify(_))));
    }

    #[test]
    fn stop_and_report_on_pending_request_rejects_and_reports() {
        let mut h = host();
        h.handle(0, request(SessionKind::Anonymous, Permissions::support()));
        let out = h.handle(1_000, HostEvent::StopAndReport);
        assert!(out.contains(&HostAction::SendReject {
            peer: PEER,
            reason: RejectReason::Reported
        }));
        assert!(out.contains(&HostAction::ReportAbuse { peer: PEER }));
        assert!(out.contains(&HostAction::EndSession(EndReason::Rejected(
            RejectReason::Reported
        ))));
    }

    #[test]
    fn user_stop_ends_session_turns_off_indicator_and_notifies_summary() {
        let mut h = active_verified(Permissions::support());
        h.handle(2, HostEvent::UserGrant(Permission::FilesOut));
        h.handle(3, HostEvent::UserRevoke(Permission::FilesOut));
        h.handle(
            4,
            HostEvent::Traffic {
                bytes_in: 10,
                bytes_out: 500,
            },
        );
        h.handle(
            5,
            HostEvent::Traffic {
                bytes_in: 5,
                bytes_out: 0,
            },
        );
        let out = h.handle(61_001, HostEvent::UserStop);
        assert_eq!(out[0], HostAction::EndSession(EndReason::HostStopped));
        assert_eq!(
            out[1],
            HostAction::ShowIndicator {
                recording: false,
                controlled: false,
                privacy: false
            }
        );
        let HostAction::Notify(log) = &out[2] else {
            panic!("expected Notify")
        };
        assert_eq!(log.peer, PEER);
        assert_eq!(log.started_at, 1);
        assert_eq!(log.ended_at, 61_001);
        assert_eq!(log.duration_ms(), 61_000);
        assert_eq!(
            log.permissions_used,
            Permissions::support() | Permission::FilesOut
        );
        assert_eq!((log.bytes_in, log.bytes_out), (15, 500));
        assert_eq!(log.kind, verified());
    }

    #[test]
    fn peer_disconnect_during_session_ends_with_log() {
        let mut h = active_verified(Permissions::support());
        let out = h.handle(100, HostEvent::PeerDisconnected);
        assert_eq!(out[0], HostAction::EndSession(EndReason::PeerDisconnected));
        assert!(matches!(h.state(), HostState::Ended { log: Some(_), .. }));
    }

    #[test]
    fn peer_disconnect_during_request_closes_dialog_without_log() {
        let mut h = host();
        h.handle(0, request(verified(), Permissions::support()));
        let out = h.handle(100, HostEvent::PeerEnded);
        assert_eq!(
            out,
            vec![
                HostAction::HideRequestDialog,
                HostAction::EndSession(EndReason::PeerEnded)
            ]
        );
        assert!(matches!(h.state(), HostState::Ended { log: None, .. }));
    }

    #[test]
    fn second_request_while_busy_is_rejected_busy() {
        let mut h = active_verified(Permissions::support());
        let other = PeerId([9; 32]);
        let out = h.handle(
            5,
            HostEvent::Request {
                peer: other,
                kind: verified(),
                requested: Permissions::full(),
            },
        );
        assert_eq!(
            out,
            vec![HostAction::SendReject {
                peer: other,
                reason: RejectReason::Busy
            }]
        );
        assert!(matches!(h.state(), HostState::Active { peer: PEER, .. }));
    }

    #[test]
    fn new_request_is_accepted_after_previous_session_ended() {
        let mut h = active_verified(Permissions::support());
        h.handle(5, HostEvent::UserStop);
        let out = h.handle(6, request(verified(), Permissions::support()));
        assert!(matches!(
            out.as_slice(),
            [HostAction::ShowRequestDialog { .. }]
        ));
    }

    #[test]
    fn anonymous_cannot_be_added_to_trust_list_during_session() {
        let mut h = active_anonymous(Permissions::support());
        assert_eq!(
            h.handle(6_000, HostEvent::UserAddTrust),
            vec![HostAction::TrustDenied]
        );
        let mut h = active_verified(Permissions::support());
        assert_eq!(
            h.handle(2, HostEvent::UserAddTrust),
            vec![HostAction::AddToTrustList(PEER)]
        );
    }

    #[test]
    fn user_events_while_idle_do_nothing() {
        let mut h = host();
        for ev in [
            HostEvent::UserAccept(Permissions::full()),
            HostEvent::UserReject,
            HostEvent::UserGrant(Permission::View),
            HostEvent::UserRevoke(Permission::View),
            HostEvent::UserStop,
            HostEvent::StopAndReport,
            HostEvent::UserAddTrust,
            HostEvent::PeerRequestPermission(Permission::View),
            HostEvent::PeerDisconnected,
            HostEvent::Tick,
        ] {
            assert_eq!(h.handle(0, ev), NONE);
        }
        assert_eq!(h.state(), &HostState::Idle);
    }
}
