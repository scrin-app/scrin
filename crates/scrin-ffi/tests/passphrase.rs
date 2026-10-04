//! D24 on the Android core: an FFI host shows five words, an FFI controller
//! types them (no diacritics, any case) and pairs through a real in-process
//! scrin-server (rendezvous + relay). A second use of the same words fails.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use scrin_ffi::{
    CoreConfig, EndInfo, EndKind, IncomingRequest, Notice, RemoteInput, SasInfo, ScrinCore,
    SessionListener, SessionPermission, SessionState, SessionStats, VideoConfigInfo,
};
use scrin_server::{Config, Role, Server, TlsMode};

#[derive(Debug, Clone)]
enum Ev {
    State(SessionState),
    Sas(SasInfo),
    Request(IncomingRequest),
    Registered,
    Ended(EndInfo),
    Other,
}

struct L(Mutex<Sender<Ev>>);

impl L {
    fn new() -> (Arc<Self>, Receiver<Ev>) {
        let (tx, rx) = channel();
        (Arc::new(Self(Mutex::new(tx))), rx)
    }
    fn send(&self, e: Ev) {
        let _ = self.0.lock().unwrap().send(e);
    }
}

impl SessionListener for L {
    fn on_state(&self, s: SessionState) {
        self.send(Ev::State(s));
    }
    fn on_sas(&self, s: SasInfo) {
        self.send(Ev::Sas(s));
    }
    fn on_incoming_request(&self, r: IncomingRequest) {
        self.send(Ev::Request(r));
    }
    fn on_permissions(&self, _: Vec<SessionPermission>) {
        self.send(Ev::Other);
    }
    fn on_permission_asked(&self, _: SessionPermission) {
        self.send(Ev::Other);
    }
    fn on_notice(&self, _: Notice) {
        self.send(Ev::Other);
    }
    fn on_stats(&self, _: SessionStats) {
        self.send(Ev::Other);
    }
    fn on_registered(&self, _: String) {
        self.send(Ev::Registered);
    }
    fn on_video_config(&self, _: VideoConfigInfo) {
        self.send(Ev::Other);
    }
    fn on_video_frame(&self, _: Vec<u8>, _: bool, _: u32, _: u64) {
        self.send(Ev::Other);
    }
    fn on_keyframe_request(&self) {
        self.send(Ev::Other);
    }
    fn on_input(&self, _: RemoteInput) {
        self.send(Ev::Other);
    }
    fn on_ended(&self, e: EndInfo) {
        self.send(Ev::Ended(e));
    }
    fn on_error(&self, _: String) {
        self.send(Ev::Other);
    }
}

const T: Duration = Duration::from_secs(15);

fn wait<R>(rx: &Receiver<Ev>, mut f: impl FnMut(&Ev) -> Option<R>) -> R {
    let deadline = std::time::Instant::now() + T;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let ev = rx.recv_timeout(left).expect("event in time");
        if let Some(v) = f(&ev) {
            return v;
        }
    }
}

fn core(seed: u8, name: &str, server: &str) -> Arc<ScrinCore> {
    let dir = std::env::temp_dir().join(format!("scrin-ffi-phrase-{}-{seed}", std::process::id()));
    ScrinCore::new(
        dir.to_string_lossy().into_owned(),
        Some(vec![seed; 32]),
        CoreConfig {
            device_name: name.into(),
            relay_urls: vec![server.to_owned()],
            loopback_only: true,
            server_url: Some(server.to_owned()),
        },
    )
    .expect("core")
}

#[test]
fn dictated_words_pair_two_cores_through_the_server_once() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    let srv = rt.block_on(async {
        let mut cfg = Config::for_tests();
        cfg.roles = vec![Role::Rendezvous, Role::Relay];
        cfg.tls = Some(TlsMode::None);
        Server::start(cfg).await.expect("server")
    });
    let base = format!("http://{}", srv.tcp_addr);

    let host = core(41, "phone", &base);
    let ctl = core(42, "laptop", &base);
    host.new_one_time_code().unwrap();
    let (hl, hrx) = L::new();
    host.start_host(hl).unwrap();
    wait(&hrx, |e| matches!(e, Ev::Registered).then_some(()));

    let phrase = host.new_passphrase("ro-RO".into()).unwrap();
    assert_eq!(phrase.words.split(' ').count(), 5);
    assert!(phrase.expires_in_s > 0);
    assert!(!format!("{phrase:?}").contains(&phrase.words));

    let typed = scrin_crypto::phrase::fold(&phrase.words).to_uppercase();
    let (cl, crx) = L::new();
    ctl.connect(typed.clone(), String::new(), cl).unwrap();
    let h_sas = wait(&hrx, |e| match e {
        Ev::Sas(s) => Some(s.clone()),
        _ => None,
    });
    let c_sas = wait(&crx, |e| match e {
        Ev::Sas(s) => Some(s.clone()),
        _ => None,
    });
    assert_eq!(h_sas, c_sas);
    let req = wait(&hrx, |e| match e {
        Ev::Request(r) => Some(r.clone()),
        _ => None,
    });
    assert!(!req.verified, "a passphrase session is anonymous");
    assert_eq!(req.controller_name, "laptop");
    ctl.end_session();
    wait(&crx, |e| matches!(e, Ev::Ended(_)).then_some(()));
    // The host must be idle again, or the next dial is refused as "busy".
    wait(&hrx, |e| matches!(e, Ev::Ended(_)).then_some(()));

    // Same words again: the secret was used, the host refuses.
    let (cl2, crx2) = L::new();
    ctl.connect(typed, String::new(), cl2).unwrap();
    let end = wait(&crx2, |e| match e {
        Ev::Ended(x) => Some(x.clone()),
        Ev::State(SessionState::Active) => panic!("reused passphrase must not pair"),
        _ => None,
    });
    assert_ne!(end.kind, EndKind::Cancelled, "{end:?}");
    assert!(end.duration_ms.is_none(), "no session ran: {end:?}");
    assert!(
        end.detail.contains("code") || end.detail.contains("used"),
        "refused because the words were used: {end:?}"
    );

    host.stop_host();
    drop((host, ctl));
    rt.block_on(srv.shutdown());
}
