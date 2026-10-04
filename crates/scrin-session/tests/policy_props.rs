//! Property: whatever the event sequence, the host never holds a permission policy forbids.

use proptest::prelude::*;
use scrin_session::{
    HostAction, HostEvent, HostSession, HostState, PeerId, Permission, Permissions, Policy,
    SessionKind,
};

fn kind() -> impl Strategy<Value = SessionKind> {
    prop_oneof![
        Just(SessionKind::Anonymous),
        Just(SessionKind::Verified {
            org: None,
            name: "x".into()
        }),
        Just(SessionKind::Trusted),
        Just(SessionKind::OrgPolicy),
    ]
}

fn perm() -> impl Strategy<Value = Permission> {
    (0..Permission::ALL.len()).prop_map(|i| Permission::ALL[i])
}

fn perms() -> impl Strategy<Value = Permissions> {
    any::<u32>().prop_map(Permissions::from_bits_truncate)
}

fn event() -> impl Strategy<Value = HostEvent> {
    prop_oneof![
        (kind(), perms()).prop_map(|(kind, requested)| HostEvent::Request {
            peer: PeerId([1; 32]),
            kind,
            requested
        }),
        perms().prop_map(HostEvent::UserAccept),
        Just(HostEvent::UserReject),
        perm().prop_map(HostEvent::UserRevoke),
        perm().prop_map(HostEvent::UserGrant),
        perm().prop_map(HostEvent::PeerRequestPermission),
        Just(HostEvent::UserAddTrust),
        Just(HostEvent::UserStop),
        Just(HostEvent::StopAndReport),
        Just(HostEvent::PeerDisconnected),
        Just(HostEvent::Tick),
    ]
}

fn policy() -> impl Strategy<Value = Policy> {
    (perms(), perms(), any::<bool>()).prop_map(|(forbidden, org, auto)| Policy {
        anonymous_forbidden: forbidden,
        org_allowed: org,
        unattended_auto_accept: auto,
        ..Policy::default()
    })
}

proptest! {
    #[test]
    fn granted_is_always_within_policy(
        policy in policy(),
        steps in prop::collection::vec((0u64..20_000, event()), 1..60),
    ) {
        let mut host = HostSession::new(policy.clone());
        let mut now = 0u64;
        for (dt, ev) in steps {
            now += dt;
            let actions = host.handle(now, ev);
            let ceiling = host.kind().map_or(Permissions::empty(), |k| policy.allowed(k));
            prop_assert!(ceiling.contains_all(host.granted()));
            for a in &actions {
                if let HostAction::SendAccept(p) | HostAction::SendPermissions(p) = a {
                    prop_assert!(ceiling.contains_all(*p), "{a:?} exceeds {ceiling:?}");
                }
            }
            if let HostState::Active { kind: SessionKind::Anonymous, started_at, .. } = host.state() {
                prop_assert!(now < started_at + policy.anonymous_max_duration_ms);
            }
        }
    }
}
