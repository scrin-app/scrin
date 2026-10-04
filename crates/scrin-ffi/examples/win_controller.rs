//! Live interop runner: this Windows PC as a scrin **controller** through `scrin-ffi`, to
//! view and drive an Android host.
//!
//! Connects by ticket + code, decodes the H.264 access units with openh264, saves the first
//! decoded picture as a PNG and optionally sends one gesture once the picture is in.
//!
//! ```powershell
//! cargo run -p scrin-ffi --example win_controller -- --ticket <scrin:…> --code ABCD-EFGH `
//!     --png .copilot-tmp\a51-host-frame.png [--swipe 0.5,0.85,0.5,0.25] [--tap 0.5,0.5] [--seconds 20]
//! ```

#[cfg(windows)]
mod run {
    // Rates printed for a human (Mb/s with two decimals): f64 precision is irrelevant.
    #![expect(clippy::cast_precision_loss)]

    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use scrin_ffi::{
        CoreConfig, EndInfo, IncomingRequest, Notice, RemoteInput, SasInfo, ScrinCore,
        SessionListener, SessionPermission, SessionState, SessionStats, TouchPhase,
        VideoConfigInfo,
    };
    use scrin_win::VideoDecoder as _;
    use scrin_win::color::{I420, i420_to_bgra};
    use scrin_win::win::decode_openh264::OpenH264Decoder;

    type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

    enum Ev {
        State(SessionState),
        Perms(Vec<SessionPermission>),
        Config(VideoConfigInfo),
        Frame(Vec<u8>, bool, u32),
        Stats(SessionStats),
        Ended(EndInfo),
    }

    struct Listener(Mutex<Sender<Ev>>);

    impl Listener {
        fn send(&self, e: Ev) {
            let _ = self
                .0
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
        fn on_incoming_request(&self, _request: IncomingRequest) {}
        fn on_permissions(&self, granted: Vec<SessionPermission>) {
            self.send(Ev::Perms(granted));
        }
        fn on_permission_asked(&self, _permission: SessionPermission) {}
        fn on_notice(&self, notice: Notice) {
            println!("NOTICE: {notice:?}");
        }
        fn on_stats(&self, stats: SessionStats) {
            self.send(Ev::Stats(stats));
        }
        fn on_registered(&self, _scrin_id: String) {}
        fn on_video_config(&self, config: VideoConfigInfo) {
            self.send(Ev::Config(config));
        }
        fn on_video_frame(&self, data: Vec<u8>, keyframe: bool, frame_id: u32, _pts_us: u64) {
            self.send(Ev::Frame(data, keyframe, frame_id));
        }
        fn on_keyframe_request(&self) {}
        fn on_input(&self, _event: RemoteInput) {}
        fn on_ended(&self, end: EndInfo) {
            self.send(Ev::Ended(end));
        }
        fn on_error(&self, message: String) {
            println!("ERROR: {message}");
        }
    }

    #[derive(Default)]
    struct Args {
        ticket: String,
        code: String,
        png: String,
        tap: Option<(f32, f32)>,
        swipe: Option<[f32; 4]>,
        seconds: u64,
    }

    fn floats(s: &str) -> Vec<f32> {
        s.split(',').filter_map(|v| v.trim().parse().ok()).collect()
    }

    fn args() -> AnyResult<Args> {
        let mut a = Args {
            png: ".copilot-tmp/host-frame.png".into(),
            seconds: 20,
            ..Args::default()
        };
        let mut it = std::env::args().skip(1);
        while let Some(x) = it.next() {
            let v = it.next().unwrap_or_default();
            match x.as_str() {
                "--ticket" => a.ticket = v,
                "--code" => a.code = v,
                "--png" => a.png = v,
                "--seconds" => a.seconds = v.parse()?,
                "--tap" => {
                    if let [x, y] = floats(&v)[..] {
                        a.tap = Some((x, y));
                    }
                }
                "--swipe" => {
                    if let [x1, y1, x2, y2] = floats(&v)[..] {
                        a.swipe = Some([x1, y1, x2, y2]);
                    }
                }
                other => eprintln!("ignoring argument {other}"),
            }
        }
        if a.ticket.is_empty() || a.code.is_empty() {
            return Err("--ticket and --code are required".into());
        }
        Ok(a)
    }

    fn save_png(path: &str, w: usize, h: usize, bgra: &[u8]) -> AnyResult<()> {
        let mut rgba = Vec::with_capacity(bgra.len());
        for px in bgra.as_chunks::<4>().0 {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(file, u32::try_from(w)?, u32::try_from(h)?);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&rgba)?;
        Ok(())
    }

    fn touch(core: &ScrinCore, phase: TouchPhase, x: f32, y: f32) -> AnyResult<()> {
        core.send_input(RemoteInput::Touch {
            pointer_id: 0,
            phase,
            x,
            y,
        })?;
        Ok(())
    }

    fn gesture(core: &ScrinCore, a: &Args) -> AnyResult<()> {
        if let Some([x1, y1, x2, y2]) = a.swipe {
            println!("SWIPE: ({x1},{y1}) -> ({x2},{y2})");
            touch(core, TouchPhase::Down, x1, y1)?;
            for i in 1..=10u8 {
                let t = f32::from(i) / 10.0;
                std::thread::sleep(Duration::from_millis(25));
                touch(
                    core,
                    TouchPhase::Move,
                    x1 + (x2 - x1) * t,
                    y1 + (y2 - y1) * t,
                )?;
            }
            touch(core, TouchPhase::Up, x2, y2)?;
        }
        if let Some((x, y)) = a.tap {
            println!("TAP: ({x},{y})");
            touch(core, TouchPhase::Down, x, y)?;
            std::thread::sleep(Duration::from_millis(60));
            touch(core, TouchPhase::Up, x, y)?;
        }
        Ok(())
    }

    /// The one event loop of the runner: linear on purpose, so a live-test log reads top to bottom.
    #[expect(clippy::too_many_lines)]
    pub fn main() -> AnyResult<()> {
        let a = args()?;
        let dir = std::env::temp_dir().join("scrin-ffi-win-controller");
        let core = ScrinCore::new(
            dir.to_string_lossy().into_owned(),
            None,
            CoreConfig {
                device_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows".into()),
                relay_urls: Vec::new(),
                loopback_only: false,
                server_url: None,
            },
        )?;
        let (tx, rx): (Sender<Ev>, Receiver<Ev>) = channel();
        let started = Instant::now();
        core.connect(
            a.ticket.clone(),
            a.code.clone(),
            Arc::new(Listener(Mutex::new(tx))),
        )?;
        let mut dec = OpenH264Decoder::new()?;
        let mut i420 = I420::default();
        let mut bgra = Vec::new();
        let (mut decoded, mut saved, mut can_input, mut gestured) = (0u32, false, false, false);
        let mut active_at = None::<Instant>;
        let deadline = Duration::from_secs(a.seconds);
        loop {
            let left = deadline.saturating_sub(started.elapsed());
            let Ok(ev) = rx.recv_timeout(left.max(Duration::from_millis(1))) else {
                println!("TIME UP after {} s", a.seconds);
                break;
            };
            match ev {
                Ev::State(s) => {
                    println!("STATE: {s:?} at {} ms", started.elapsed().as_millis());
                    if s == SessionState::Active {
                        active_at = Some(Instant::now());
                    }
                }
                Ev::Perms(p) => {
                    println!("GRANTED: {p:?}");
                    can_input = p.contains(&SessionPermission::Input);
                }
                Ev::Config(c) => println!(
                    "VIDEO CONFIG: {:?} {}x{} {} fps {} b/s, {} bytes codec config",
                    c.codec,
                    c.width,
                    c.height,
                    c.fps,
                    c.bitrate_bps,
                    c.codec_config.len()
                ),
                Ev::Frame(data, key, id) => {
                    let Ok(Some(pic)) = dec.decode(&data) else {
                        if key {
                            println!(
                                "FRAME {id}: keyframe {} bytes did not decode yet",
                                data.len()
                            );
                        }
                        continue;
                    };
                    decoded += 1;
                    if !saved {
                        i420.width = pic.width;
                        i420.height = pic.height;
                        i420.y.clone_from(&pic.y);
                        i420.u.clone_from(&pic.u);
                        i420.v.clone_from(&pic.v);
                        i420_to_bgra(&i420, &mut bgra)?;
                        save_png(&a.png, pic.width, pic.height, &bgra)?;
                        saved = true;
                        println!(
                            "FIRST PICTURE: {}x{} frame {id} after {} ms of Active -> {}",
                            pic.width,
                            pic.height,
                            active_at.map_or(0, |t| t.elapsed().as_millis()),
                            a.png
                        );
                    }
                }
                Ev::Stats(s) => println!(
                    "STATS: in {:.1} fps {:.2} Mb/s rtt {} ms direct={} frames={} lost={} decoded={decoded}",
                    s.fps,
                    s.bitrate_bps as f64 / 1e6,
                    s.rtt_ms,
                    s.direct,
                    s.frames,
                    s.frames_lost
                ),
                Ev::Ended(e) => {
                    println!("ENDED: {:?} {}", e.kind, e.detail);
                    return Ok(());
                }
            }
            if saved && can_input && !gestured && (a.tap.is_some() || a.swipe.is_some()) {
                gestured = true;
                gesture(&core, &a)?;
            }
        }
        println!("DECODED: {decoded}");
        core.end_session();
        std::thread::sleep(Duration::from_millis(700));
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    run::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("win_controller runs on Windows only");
}
