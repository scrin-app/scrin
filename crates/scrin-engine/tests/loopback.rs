//! Two engines in one process over loopback (relays disabled), with the
//! synthetic media backend: pair with the code, compare SAS, accept, stream,
//! inject input, end cleanly; and a wrong code is refused.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use scrin_engine::backend::InputEvent;
use scrin_engine::resolve::{StaticResolver, parse_ticket};
use scrin_engine::secret::FileStore;
use scrin_engine::{
    Command, EngineConfig, EngineHandle, Event, NetConfig, Reply, SessionState, SyntheticBackend,
};
use scrin_proto::v1;
use tokio::sync::mpsc::UnboundedReceiver;

const T: Duration = Duration::from_secs(10);

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let mut n = [0u8; 8];
    getrandom::fill(&mut n).expect("rng");
    std::env::temp_dir().join(format!("scrin-engine-it-{tag}-{}", u64::from_le_bytes(n)))
}

async fn engine(
    tag: &str,
    backend: &SyntheticBackend,
    resolver: &StaticResolver,
) -> (EngineHandle, UnboundedReceiver<Event>) {
    let dir = temp_dir(tag);
    let mut cfg = EngineConfig::new(&dir);
    cfg.secrets = Box::new(FileStore::new(dir.join("secrets")));
    cfg.net = NetConfig::loopback();
    cfg.backend = Arc::new(backend.clone());
    cfg.resolver = Arc::new(resolver.clone());
    cfg.device_name = tag.into();
    // Accept is clickable right away so the test does not sleep 5 s.
    cfg.policy.anonymous_accept_delay_ms = 0;
    scrin_engine::start(cfg).await.expect("engine starts")
}

/// Next event matching `f`, within the test deadline.
async fn wait_for<T>(
    rx: &mut UnboundedReceiver<Event>,
    mut f: impl FnMut(&Event) -> Option<T>,
) -> T {
    tokio::time::timeout(T, async {
        loop {
            let e = rx.recv().await.expect("engine alive");
            if let Some(v) = f(&e) {
                return v;
            }
        }
    })
    .await
    .expect("event in time")
}

fn session_of(r: Reply) -> String {
    match r {
        Reply::Session(s) => s,
        other => panic!("expected a session, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_accept_stream_input_and_end() {
    let host_backend = SyntheticBackend::new(320, 180);
    let resolver = StaticResolver::default();
    let (host, mut host_ev) = engine("host", &host_backend, &resolver).await;
    let (ctl, mut ctl_ev) = engine("ctl", &SyntheticBackend::new(64, 64), &resolver).await;

    let status = host.status().await.expect("status");
    assert_eq!(status.scrin_id.len(), 9);
    assert_eq!(status.code.len(), 9, "display form ABCD-EFGH");
    // Dial by scrin ID through the resolver (the server path), loopback ticket underneath.
    resolver.insert(
        &status.scrin_id,
        parse_ticket(&status.ticket).expect("ticket"),
    );

    let started = Instant::now();
    let c_sess = session_of(
        ctl.call(Command::Connect {
            target: status.scrin_id.clone(),
            code: status.code.to_lowercase(),
            requested: None,
        })
        .await
        .expect("connect"),
    );

    let ctl_sas = wait_for(&mut ctl_ev, |e| match e {
        Event::Sas { emoji, .. } => Some(*emoji),
        _ => None,
    })
    .await;
    let (h_sess, host_sas, requested) = wait_for(&mut host_ev, |e| match e {
        Event::IncomingRequest {
            session,
            sas,
            requested,
            kind,
            ..
        } => {
            assert_eq!(kind, "anonymous");
            Some((session.clone(), *sas, requested.clone()))
        }
        _ => None,
    })
    .await;
    assert_eq!(host_sas, Some(ctl_sas), "both sides derive the same SAS");
    assert!(requested.contains(&"input".to_owned()));

    // The code is single use: it rotated the moment pairing consumed it.
    let rotated = host.status().await.expect("status");
    assert_ne!(rotated.code, status.code);

    ctl.call(Command::ConfirmSas {
        session: c_sess.clone(),
        matches: true,
    })
    .await
    .expect("confirm");
    host.call(Command::Accept {
        session: h_sess.clone(),
        permissions: vec!["view".into(), "input".into(), "clipboard".into()],
    })
    .await
    .expect("accept");

    wait_for(&mut ctl_ev, |e| match e {
        Event::StateChanged {
            state: SessionState::Active,
            ..
        } => Some(()),
        _ => None,
    })
    .await;
    let accepted_at = Instant::now();

    // Video: count decoded frames for up to 3 s.
    let mut frames = 0u32;
    let mut first = None;
    let mut size = (0, 0);
    let deadline = accepted_at + Duration::from_secs(3);
    while frames < 30 && Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, ctl_ev.recv()).await {
            Ok(Some(Event::VideoFrame { frame, .. })) => {
                frames += 1;
                first.get_or_insert_with(|| accepted_at.elapsed());
                size = (frame.width, frame.height);
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    let elapsed = accepted_at.elapsed();
    println!(
        "LOOPBACK: {frames} frames in {:.0} ms after accept (first after {:.0} ms, {}x{}); \
         connect->accepted {:.0} ms",
        elapsed.as_secs_f64() * 1000.0,
        first.unwrap_or_default().as_secs_f64() * 1000.0,
        size.0,
        size.1,
        (accepted_at - started).as_secs_f64() * 1000.0,
    );
    assert!(frames >= 30, "only {frames} frames in {elapsed:?}");
    assert!(elapsed <= Duration::from_secs(3));
    assert_eq!(size, (320, 180));

    // Input reaches the host backend.
    let key = v1::KeyEvent {
        hid_usage: 0x04,
        down: true,
        modifiers: 0,
        text: None,
        repeat: false,
    };
    ctl.call(Command::SendInput {
        session: c_sess.clone(),
        event: InputEvent::Key(key.clone()),
    })
    .await
    .expect("send input");
    tokio::time::timeout(T, async {
        while !host_backend
            .injected()
            .contains(&InputEvent::Key(key.clone()))
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("input injected on the host");

    // Stats arrive once per second.
    let session_stats = wait_for(&mut ctl_ev, |e| match e {
        Event::Stats { stats, .. } => Some(*stats),
        _ => None,
    })
    .await;
    println!("STATS: {session_stats:?}");
    assert!(session_stats.frames_total >= 30);

    // Revoking input is enforced on the controller.
    host.call(Command::Revoke {
        session: h_sess.clone(),
        permission: "input".into(),
    })
    .await
    .expect("revoke");
    wait_for(&mut ctl_ev, |e| match e {
        Event::PermissionsChanged { granted, .. } if !granted.contains(&"input".to_owned()) => {
            Some(())
        }
        _ => None,
    })
    .await;
    assert!(
        ctl.call(Command::SendInput {
            session: c_sess.clone(),
            event: InputEvent::Key(key),
        })
        .await
        .is_err()
    );

    // Controller ends; both sides see Ended.
    ctl.call(Command::EndSession {
        session: c_sess.clone(),
    })
    .await
    .expect("end");
    let c_reason = wait_for(&mut ctl_ev, |e| match e {
        Event::StateChanged {
            state: SessionState::Ended,
            reason,
            ..
        } => Some(reason.clone()),
        _ => None,
    })
    .await;
    let h_reason = wait_for(&mut host_ev, |e| match e {
        Event::StateChanged {
            state: SessionState::Ended,
            reason,
            ..
        } => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert_eq!(c_reason.as_deref(), Some("cancelled"));
    assert_eq!(h_reason.as_deref(), Some("peer-ended"));

    ctl.shutdown().await;
    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_code_is_rejected_and_burns_the_code() {
    let resolver = StaticResolver::default();
    let (host, mut host_ev) = engine("host-w", &SyntheticBackend::default(), &resolver).await;
    let (ctl, mut ctl_ev) = engine("ctl-w", &SyntheticBackend::default(), &resolver).await;
    let status = host.status().await.expect("status");

    let raw: String = status.code.chars().filter(|c| *c != '-').collect();
    let first = if raw.starts_with('A') { 'B' } else { 'A' };
    let wrong: String = std::iter::once(first).chain(raw.chars().skip(1)).collect();

    let sess = session_of(
        ctl.call(Command::Connect {
            target: status.ticket.clone(),
            code: wrong,
            requested: None,
        })
        .await
        .expect("connect"),
    );
    let code = wait_for(&mut ctl_ev, |e| match e {
        Event::Error { session, code, .. } if session.as_deref() == Some(sess.as_str()) => {
            Some(code.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(code, "wrong-code");
    let reason = wait_for(&mut ctl_ev, |e| match e {
        Event::StateChanged {
            state: SessionState::Ended,
            reason,
            ..
        } => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert_eq!(reason.as_deref(), Some("pairing-failed"));

    // The host never showed a request, and the guessed-at code is gone.
    let new_code = wait_for(&mut host_ev, |e| match e {
        Event::IncomingRequest { .. } => panic!("no request after a wrong code"),
        Event::Status(s) => Some(s.code.clone()),
        _ => None,
    })
    .await;
    assert_ne!(new_code, status.code);

    ctl.shutdown().await;
    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn identity_persists_and_unknown_id_is_offline() {
    let dir = temp_dir("persist");
    let mk = || {
        let mut cfg = EngineConfig::new(&dir);
        cfg.secrets = Box::new(FileStore::new(dir.join("secrets")));
        cfg.net = NetConfig::loopback();
        cfg
    };
    let (a, _ea) = scrin_engine::start(mk()).await.expect("start");
    let first = a.status().await.expect("status").device_id;
    a.shutdown().await;
    let (b, mut eb) = scrin_engine::start(mk()).await.expect("restart");
    assert_eq!(b.status().await.expect("status").device_id, first);

    let sess = session_of(
        b.call(Command::Connect {
            target: "999 999 999".into(),
            code: "ABCD-EFGH".into(),
            requested: None,
        })
        .await
        .expect("connect"),
    );
    let code = wait_for(&mut eb, |e| match e {
        Event::Error { session, code, .. } if session.as_deref() == Some(sess.as_str()) => {
            Some(code.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(code, "offline");
    b.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}
