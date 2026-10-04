//! Two `ScrinCore`s on loopback: quick connect, SAS match, accept, input, video, end.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use scrin_ffi::{
    CoreConfig, EndInfo, EndKind, IncomingRequest, Notice, RemoteInput, SasInfo, ScrinCore,
    SessionListener, SessionPermission, SessionState, SessionStats, TouchPhase, VideoCodec,
    VideoConfigInfo,
};

#[derive(Debug, Clone)]
enum Ev {
    State(SessionState),
    Sas(SasInfo),
    Request(IncomingRequest),
    Perms(Vec<SessionPermission>),
    Input(RemoteInput),
    VideoConfig(VideoConfigInfo),
    Frame(Vec<u8>, bool),
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
    fn on_permissions(&self, p: Vec<SessionPermission>) {
        self.send(Ev::Perms(p));
    }
    fn on_permission_asked(&self, _: SessionPermission) {
        self.send(Ev::Other);
    }
    fn on_notice(&self, _: Notice) {
        self.send(Ev::Other);
    }
    fn on_stats(&self, _: SessionStats) {}
    fn on_video_config(&self, c: VideoConfigInfo) {
        self.send(Ev::VideoConfig(c));
    }
    fn on_video_frame(&self, d: Vec<u8>, k: bool) {
        self.send(Ev::Frame(d, k));
    }
    fn on_keyframe_request(&self) {}
    fn on_input(&self, e: RemoteInput) {
        self.send(Ev::Input(e));
    }
    fn on_ended(&self, e: EndInfo) {
        self.send(Ev::Ended(e));
    }
    fn on_error(&self, _: String) {
        self.send(Ev::Other);
    }
}

const T: Duration = Duration::from_secs(10);

fn wait<T>(rx: &Receiver<Ev>, mut f: impl FnMut(&Ev) -> Option<T>) -> T {
    let deadline = std::time::Instant::now() + T;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let ev = rx.recv_timeout(left).expect("event in time");
        if let Some(v) = f(&ev) {
            return v;
        }
    }
}

fn core(seed: u8, name: &str) -> Arc<ScrinCore> {
    let dir = std::env::temp_dir().join(format!("scrin-ffi-test-{}-{seed}", std::process::id()));
    ScrinCore::new(
        dir.to_string_lossy().into_owned(),
        Some(vec![seed; 32]),
        CoreConfig {
            device_name: name.into(),
            relay_urls: Vec::new(),
            loopback_only: true,
        },
    )
    .expect("core")
}

#[test]
fn quick_connect_end_to_end_on_loopback() {
    let host = core(31, "host");
    let ctl = core(32, "laptop");
    let code = host.new_one_time_code().unwrap();
    assert_eq!(code.display.len(), 9);
    let ticket = host.host_info().unwrap().ticket;

    let (hl, hrx) = L::new();
    let (cl, crx) = L::new();
    host.start_host(hl).unwrap();
    wait(&hrx, |e| {
        matches!(e, Ev::State(SessionState::Listening)).then_some(())
    });
    ctl.connect(ticket, code.display.to_lowercase(), cl)
        .unwrap();

    let h_sas = wait(&hrx, |e| match e {
        Ev::Sas(s) => Some(s.clone()),
        _ => None,
    });
    let c_sas = wait(&crx, |e| match e {
        Ev::Sas(s) => Some(s.clone()),
        _ => None,
    });
    assert_eq!(h_sas, c_sas);
    assert_eq!(h_sas.emoji.len(), 5);

    let req = wait(&hrx, |e| match e {
        Ev::Request(r) => Some(r.clone()),
        _ => None,
    });
    assert!(!req.verified, "quick connect is anonymous");
    assert_eq!(req.controller_name, "laptop");
    assert!(req.accept_in_ms >= 4_000, "anti-scam accept delay");
    assert!(!req.allowed.contains(&SessionPermission::FilesIn));
    assert!(!code_valid_after_use(&host));

    // Accept before the delay is ignored by the state machine…
    host.host_accept(vec![SessionPermission::View, SessionPermission::Input])
        .unwrap();
    std::thread::sleep(Duration::from_millis(req.accept_in_ms + 100));
    // …and honoured after it.
    host.host_accept(vec![SessionPermission::View, SessionPermission::Input])
        .unwrap();
    // The controller reports Active, then the granted set.
    wait(&crx, |e| {
        matches!(e, Ev::State(SessionState::Active)).then_some(())
    });
    let granted = wait(&crx, |e| match e {
        Ev::Perms(p) if !p.is_empty() => Some(p.clone()),
        _ => None,
    });
    assert!(granted.contains(&SessionPermission::Input));

    let touch = RemoteInput::Touch {
        pointer_id: 0,
        phase: TouchPhase::Down,
        x: 0.5,
        y: 0.25,
    };
    ctl.send_input(touch.clone()).unwrap();
    let got = wait(&hrx, |e| match e {
        Ev::Input(i) => Some(i.clone()),
        _ => None,
    });
    assert_eq!(got, touch);

    let cfg = VideoConfigInfo {
        codec: VideoCodec::H264,
        width: 1080,
        height: 2400,
        fps: 30,
        bitrate_bps: 4_000_000,
        codec_config: vec![0, 0, 0, 1, 0x67],
    };
    host.send_video_config(cfg.clone()).unwrap();
    assert_eq!(
        wait(&crx, |e| match e {
            Ev::VideoConfig(c) => Some(c.clone()),
            _ => None,
        }),
        cfg
    );
    let frame: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
    host.send_video_frame(frame.clone(), true).unwrap();
    let (data, kf) = wait(&crx, |e| match e {
        Ev::Frame(d, k) => Some((d.clone(), *k)),
        _ => None,
    });
    assert!(kf);
    assert_eq!(data, frame);

    ctl.end_session();
    let c_end = wait(&crx, |e| match e {
        Ev::Ended(x) => Some(x.clone()),
        _ => None,
    });
    assert_eq!(c_end.kind, EndKind::Cancelled);
    let h_end = wait(&hrx, |e| match e {
        Ev::Ended(x) => Some(x.clone()),
        _ => None,
    });
    assert_eq!(h_end.kind, EndKind::PeerEnded);
    assert!(h_end.duration_ms.is_some());
}

#[test]
fn wrong_code_fails_pairing_and_burns_the_code() {
    let host = core(41, "host");
    let ctl = core(42, "ctl");
    let code = host.new_one_time_code().unwrap();
    let ticket = host.host_info().unwrap().ticket;
    let (hl, hrx) = L::new();
    let (cl, crx) = L::new();
    host.start_host(hl).unwrap();
    wait(&hrx, |e| {
        matches!(e, Ev::State(SessionState::Listening)).then_some(())
    });
    let wrong = if code.display.starts_with('A') {
        "BBBB-BBBB"
    } else {
        "AAAA-AAAA"
    };
    ctl.connect(ticket, wrong.into(), cl).unwrap();
    let end = wait(&crx, |e| match e {
        Ev::Ended(x) => Some(x.clone()),
        _ => None,
    });
    assert_eq!(end.kind, EndKind::PairingFailed);
    assert!(!host.code_valid());
}

#[test]
fn invalid_inputs_are_rejected_before_dialling() {
    let ctl = core(51, "ctl");
    let (cl, _rx) = L::new();
    assert!(
        ctl.connect("nope".into(), "AAAA-AAAA".into(), cl.clone())
            .is_err()
    );
    let hex = ctl.device_id();
    assert!(ctl.connect(hex, "short".into(), cl).is_err());
    assert!(scrin_ffi::is_valid_code("abcd efgh"));
    assert!(!scrin_ffi::is_valid_code("ABCD-EFG0"));
}

#[test]
fn trust_list_crud_persists_across_restarts() {
    let a = core(61, "a");
    let other = core(62, "b").device_id();
    a.add_trusted(
        other.clone(),
        "Office PC".into(),
        scrin_ffi::TrustProfile::Full,
        None,
    )
    .unwrap();
    assert_eq!(a.list_trusted().len(), 1);
    drop(a);
    let a = core(61, "a");
    let list = a.list_trusted();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].label, "Office PC");
    assert!(a.remove_trusted(other.clone()).unwrap());
    assert!(!a.remove_trusted(other).unwrap());
    assert_eq!(a.list_trusted().len(), 0);
}

fn code_valid_after_use(c: &ScrinCore) -> bool {
    c.code_valid()
}
