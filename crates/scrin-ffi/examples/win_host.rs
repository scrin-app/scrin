//! Live interop runner: this Windows PC as a scrin **host** through `scrin-ffi` (the same
//! core the Android app links), so a phone can control it over LAN or relay.
//!
//! DXGI capture → (½ scale above 1920 px) → best H.264 encoder (hardware MFT, else
//! openh264) → `ScrinCore::send_video_frame` (FEC shards → QUIC datagrams); input from the
//! controller is injected with `SendInput`.
//!
//! ```powershell
//! cargo run -p scrin-ffi --example win_host -- --out .copilot-tmp\win-host.txt --auto-accept
//! ```
//!
//! `--out` receives the ticket and the one-time code so a test harness can type them on the
//! phone (gitignored scratch; the code is single use and expires in 10 minutes).
//! `--auto-accept` accepts View + Input once the anti-scam delay has passed — a test switch;
//! without it the runner asks on stdin.
//! `--safe-input` injects only pointer moves and logs clicks, keys and text without injecting
//! them, so a live test on a desktop someone is using cannot click or type into their windows.

#[cfg(windows)]
mod run {
    // Rates printed for a human (Mb/s with two decimals): f64 precision is irrelevant.
    #![expect(clippy::cast_precision_loss)]

    use std::io::Write as _;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use scrin_ffi::{
        CoreConfig, EndInfo, IncomingRequest, MouseButtonKind, Notice, RemoteInput, SasInfo,
        ScrinCore, SessionListener, SessionPermission, SessionState, SessionStats, TouchPhase,
        VideoCodec, VideoConfigInfo,
    };
    use scrin_win::win::capture_dxgi::DxgiCapture;
    use scrin_win::win::input::SendInputInjector;
    use scrin_win::win::{EncoderSettings, best_encoder};
    use scrin_win::{CaptureSource, CapturedFrame, InputInjector, MouseButton, PixelData};

    type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

    const FPS: u32 = 30;
    const BITRATE: u32 = 6_000_000;

    enum Ev {
        State(SessionState),
        Request(IncomingRequest),
        Input(RemoteInput),
        Keyframe,
        Ended(EndInfo),
    }

    struct Listener {
        tx: Mutex<Sender<Ev>>,
        keyframe: Arc<AtomicBool>,
    }

    impl Listener {
        fn send(&self, e: Ev) {
            let _ = self
                .tx
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send(e);
        }
    }

    impl SessionListener for Listener {
        fn on_state(&self, state: SessionState) {
            self.send(Ev::State(state));
        }
        fn on_sas(&self, sas: SasInfo) {
            println!("SAS: {}", sas.names.join(" "));
        }
        fn on_incoming_request(&self, request: IncomingRequest) {
            self.send(Ev::Request(request));
        }
        fn on_permissions(&self, granted: Vec<SessionPermission>) {
            println!("PERMISSIONS: {granted:?}");
        }
        fn on_permission_asked(&self, permission: SessionPermission) {
            println!("ASKED: {permission:?} (ignored)");
        }
        fn on_notice(&self, notice: Notice) {
            println!("NOTICE: {notice:?}");
        }
        fn on_stats(&self, s: SessionStats) {
            println!(
                "STATS: out {:.1} fps {:.2} Mb/s rtt {} ms direct={} frames={}",
                s.fps,
                s.bitrate_bps as f64 / 1e6,
                s.rtt_ms,
                s.direct,
                s.frames
            );
        }
        fn on_registered(&self, scrin_id: String) {
            println!("REGISTERED: {scrin_id}");
        }
        fn on_video_config(&self, _config: VideoConfigInfo) {}
        fn on_video_frame(&self, _data: Vec<u8>, _keyframe: bool, _frame_id: u32, _pts_us: u64) {}
        fn on_keyframe_request(&self) {
            self.keyframe.store(true, Ordering::Release);
            self.send(Ev::Keyframe);
        }
        fn on_input(&self, event: RemoteInput) {
            self.send(Ev::Input(event));
        }
        fn on_ended(&self, end: EndInfo) {
            self.send(Ev::Ended(end));
        }
        fn on_error(&self, message: String) {
            println!("ERROR: {message}");
        }
    }

    struct Args {
        out: Option<String>,
        auto_accept: bool,
        safe_input: bool,
        server: Option<String>,
    }

    fn args() -> Args {
        let mut a = Args {
            out: None,
            auto_accept: false,
            safe_input: false,
            server: None,
        };
        let mut it = std::env::args().skip(1);
        while let Some(x) = it.next() {
            match x.as_str() {
                "--out" => a.out = it.next(),
                "--server" => a.server = it.next(),
                "--auto-accept" => a.auto_accept = true,
                "--safe-input" => a.safe_input = true,
                other => eprintln!("ignoring argument {other}"),
            }
        }
        a
    }

    /// Copies (div = 1) or 2×2-box-averages (div = 2) a BGRA image into `out` (`w`×`h`, tight).
    fn downscale(px: &[u8], stride: usize, div: usize, w: usize, h: usize, out: &mut Vec<u8>) {
        out.resize(w * h * 4, 0);
        for y in 0..h {
            let dst = &mut out[y * w * 4..(y + 1) * w * 4];
            if div == 1 {
                dst.copy_from_slice(&px[y * stride..y * stride + w * 4]);
                continue;
            }
            let r0 = &px[2 * y * stride..];
            let r1 = &px[(2 * y + 1) * stride..];
            for x in 0..w {
                for c in 0..4 {
                    let s = u16::from(r0[8 * x + c])
                        + u16::from(r0[8 * x + 4 + c])
                        + u16::from(r1[8 * x + c])
                        + u16::from(r1[8 * x + 4 + c]);
                    dst[4 * x + c] = u8::try_from(s / 4).unwrap_or(u8::MAX);
                }
            }
        }
    }

    /// Capture → encode → send until `stop`.
    fn stream(core: &ScrinCore, stop: &AtomicBool, keyframe: &AtomicBool) -> AnyResult<()> {
        let mut cap = DxgiCapture::primary(true)?;
        let b = cap.display().bounds;
        let (sw, sh) = (b.width() as usize, b.height() as usize);
        let div = if sw > 1920 { 2 } else { 1 };
        let (w, h) = ((sw / div) & !1, (sh / div) & !1);
        let (w32, h32) = (u32::try_from(w)?, u32::try_from(h)?);
        let mut enc = best_encoder(EncoderSettings {
            width: w32,
            height: h32,
            fps: FPS,
            bitrate: BITRATE,
        })?;
        println!(
            "STREAM: {sw}x{sh} -> {w}x{h} with {} at {FPS} fps",
            enc.name()
        );
        core.send_video_config(VideoConfigInfo {
            codec: VideoCodec::H264,
            width: w32,
            height: h32,
            fps: FPS,
            bitrate_bps: BITRATE,
            codec_config: Vec::new(),
        })?;
        let interval = Duration::from_secs(1) / FPS;
        let started = Instant::now();
        let mut img: Vec<u8> = Vec::new();
        let mut force = true;
        let mut next = Instant::now();
        let (mut frames, mut bytes, mut keys) = (0u64, 0usize, 0u32);
        let mut report = Instant::now();
        while !stop.load(Ordering::Acquire) {
            if let Some(f) = cap.next_frame(interval)?
                && let PixelData::Cpu(px) = f.data
            {
                downscale(px, f.stride, div, w, h, &mut img);
            }
            if img.is_empty() {
                continue;
            }
            if keyframe.swap(false, Ordering::AcqRel) {
                force = true;
            }
            let frame = CapturedFrame {
                width: w32,
                height: h32,
                stride: w * 4,
                data: PixelData::Cpu(&img),
                image_updated: true,
                dirty_rects: &[],
                move_rects: &[],
                pts_us: u64::try_from(started.elapsed().as_micros())?,
                cursor: None,
                discontinuity: false,
            };
            if let Some(e) = enc.encode(&frame, force)? {
                if e.keyframe {
                    force = false;
                    keys += 1;
                }
                frames += 1;
                bytes += e.data.len();
                core.send_video_frame(e.data, e.keyframe)?;
            }
            if report.elapsed() >= Duration::from_secs(2) {
                let dt = report.elapsed().as_secs_f64();
                println!(
                    "ENCODE: {:.1} fps {:.2} Mb/s keyframes {keys}",
                    frames as f64 / dt,
                    bytes as f64 * 8.0 / dt / 1e6
                );
                (frames, bytes, keys) = (0, 0, 0);
                report = Instant::now();
            }
            next += interval;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
        Ok(())
    }

    fn inject(inj: &mut SendInputInjector, e: &RemoteInput) -> scrin_win::Result<()> {
        match e {
            RemoteInput::Touch { phase, x, y, .. } => {
                inj.mouse_move_abs(*x, *y)?;
                match phase {
                    TouchPhase::Down => inj.button(MouseButton::Left, true),
                    TouchPhase::Up | TouchPhase::Cancel => inj.button(MouseButton::Left, false),
                    TouchPhase::Move => Ok(()),
                }
            }
            RemoteInput::MouseMove { x, y } => inj.mouse_move_abs(*x, *y),
            RemoteInput::MouseButton { button, down } => {
                let b = match button {
                    MouseButtonKind::Left => MouseButton::Left,
                    MouseButtonKind::Right => MouseButton::Right,
                    MouseButtonKind::Middle => MouseButton::Middle,
                    MouseButtonKind::Back => MouseButton::X1,
                    MouseButtonKind::Forward => MouseButton::X2,
                };
                inj.button(b, *down)
            }
            RemoteInput::Wheel { dx, dy } => inj.wheel(*dx, *dy),
            RemoteInput::Key {
                hid_usage, down, ..
            } => inj.key(*hid_usage, *down),
            RemoteInput::Text { text } => inj.unicode(text),
        }
    }

    fn confirm(prompt: &str) -> bool {
        print!("{prompt} [y/N] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).is_ok() && line.trim().eq_ignore_ascii_case("y")
    }

    pub fn main() -> AnyResult<()> {
        let a = args();
        let dir = std::env::temp_dir().join("scrin-ffi-win-host");
        let core = ScrinCore::new(
            dir.to_string_lossy().into_owned(),
            None,
            CoreConfig {
                device_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows".into()),
                relay_urls: Vec::new(),
                loopback_only: false,
                server_url: a.server.clone(),
            },
        )?;
        let otc = core.new_one_time_code()?;
        let (tx, rx): (Sender<Ev>, Receiver<Ev>) = channel();
        let keyframe = Arc::new(AtomicBool::new(false));
        let listener = Arc::new(Listener {
            tx: Mutex::new(tx),
            keyframe: Arc::clone(&keyframe),
        });
        core.start_host(listener)?;
        let info = core.host_info()?;
        println!("TICKET: {}", info.ticket);
        println!("FINGERPRINT: {}", info.fingerprint);
        println!("CODE: {}", otc.display);
        if let Some(out) = &a.out {
            std::fs::write(out, format!("{}\n{}\n", info.ticket, otc.display))?;
        }

        let mut inj = SendInputInjector::primary()?;
        let stop = Arc::new(AtomicBool::new(false));
        let mut media: Option<std::thread::JoinHandle<()>> = None;
        let mut inputs = 0u32;
        while let Ok(ev) = rx.recv() {
            match ev {
                Ev::State(s) => {
                    println!("STATE: {s:?}");
                    if s == SessionState::Active && media.is_none() {
                        let core = Arc::clone(&core);
                        let stop = Arc::clone(&stop);
                        let key = Arc::clone(&keyframe);
                        media = Some(std::thread::spawn(move || {
                            if let Err(e) = stream(&core, &stop, &key) {
                                println!("STREAM ERROR: {e}");
                            }
                        }));
                    }
                }
                Ev::Request(r) => {
                    println!(
                        "REQUEST from {} ({}) wants {:?}; accept possible in {} ms",
                        r.controller_name, r.peer_fingerprint, r.requested, r.accept_in_ms
                    );
                    let ok = a.auto_accept || confirm("Accept view + input?");
                    if ok {
                        std::thread::sleep(Duration::from_millis(r.accept_in_ms + 200));
                        core.host_accept(vec![SessionPermission::View, SessionPermission::Input])?;
                        println!("ACCEPTED");
                    } else {
                        core.host_reject()?;
                    }
                }
                Ev::Input(e) => {
                    inputs += 1;
                    let pointer_only = match &e {
                        RemoteInput::MouseMove { .. } => true,
                        RemoteInput::Touch { phase, .. } => *phase == TouchPhase::Move,
                        _ => false,
                    };
                    if !pointer_only || inputs <= 40 || inputs.is_power_of_two() {
                        println!("INPUT #{inputs}: {e:?}");
                    }
                    if a.safe_input && !pointer_only {
                        if let RemoteInput::Touch { x, y, .. } = &e
                            && let Err(err) = inj.mouse_move_abs(*x, *y)
                        {
                            println!("INJECT ERROR: {err}");
                        }
                    } else if let Err(err) = inject(&mut inj, &e) {
                        println!("INJECT ERROR: {err}");
                    }
                }
                Ev::Keyframe => println!("KEYFRAME REQUEST"),
                Ev::Ended(end) => {
                    println!(
                        "ENDED: {:?} {} ({:?} ms)",
                        end.kind, end.detail, end.duration_ms
                    );
                    break;
                }
            }
        }
        stop.store(true, Ordering::Release);
        if let Some(m) = media {
            let _ = m.join();
        }
        println!("INPUT EVENTS: {inputs}");
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    run::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("win_host runs on Windows only");
}
