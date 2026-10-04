//! Dev benchmark: capture the primary display for 3 s, encode with the best available encoder
//! (hardware MFT, else openh264), report fps / encode time / bitrate and write an Annex B
//! `.h264` file to `.copilot-tmp/`.
//!
//! ```powershell
//! cargo run -p scrin-win --release --example capture_encode [-- openh264|mf-sw|mf-hw]
//! ```
//!
//! A static desktop produces no new frames, so the last captured image is re-submitted at the
//! target rate: the numbers reflect steady 60 fps streaming load, not desktop activity.

#[cfg(windows)]
mod bench {
    use std::io::Write;
    use std::time::{Duration, Instant};

    use scrin_win::win::capture_dxgi::DxgiCapture;
    use scrin_win::win::encode_mf::MfEncoder;
    use scrin_win::win::encode_openh264::OpenH264Encoder;
    use scrin_win::win::{EncoderSettings, best_encoder};
    use scrin_win::{CaptureSource, CapturedFrame, PixelData, VideoEncoder};

    type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

    const FPS: u32 = 60;
    const RUN: Duration = Duration::from_secs(3);

    #[derive(Default)]
    struct Stats {
        submitted: u32,
        fresh: u32,
        encoded: u32,
        keyframes: u32,
        bytes: usize,
        encode_time: Duration,
        max_encode: Duration,
        capture_time: Duration,
    }

    struct LastImage {
        data: Vec<u8>,
        width: u32,
        height: u32,
        stride: usize,
    }

    fn make_encoder(choice: &str, settings: EncoderSettings) -> AnyResult<Box<dyn VideoEncoder>> {
        Ok(match choice {
            "openh264" => Box::new(OpenH264Encoder::new(settings)?),
            "mf-sw" => Box::new(MfEncoder::new_software(settings)?),
            "mf-hw" => Box::new(MfEncoder::new_hardware(settings)?),
            _ => best_encoder(settings)?,
        })
    }

    fn capture_into(
        cap: &mut DxgiCapture,
        timeout: Duration,
        last: &mut LastImage,
        stats: &mut Stats,
    ) -> AnyResult<()> {
        let t0 = Instant::now();
        if let Some(f) = cap.next_frame(timeout)?
            && let PixelData::Cpu(b) = f.data
        {
            last.data.clear();
            last.data.extend_from_slice(b);
            (last.width, last.height, last.stride) = (f.width, f.height, f.stride);
            stats.fresh += u32::from(f.image_updated);
        }
        stats.capture_time += t0.elapsed();
        Ok(())
    }

    fn encode_one(
        enc: &mut dyn VideoEncoder,
        last: &LastImage,
        pts_us: u64,
        force: bool,
        out: &mut impl Write,
        stats: &mut Stats,
    ) -> AnyResult<()> {
        let frame = CapturedFrame {
            width: last.width,
            height: last.height,
            stride: last.stride,
            data: PixelData::Cpu(&last.data),
            image_updated: true,
            dirty_rects: &[],
            move_rects: &[],
            pts_us,
            cursor: None,
            discontinuity: false,
        };
        stats.submitted += 1;
        let t = Instant::now();
        let encoded = enc.encode(&frame, force)?;
        let dt = t.elapsed();
        stats.encode_time += dt;
        stats.max_encode = stats.max_encode.max(dt);
        if let Some(e) = encoded {
            stats.encoded += 1;
            stats.keyframes += u32::from(e.keyframe);
            stats.bytes += e.data.len();
            out.write_all(&e.data)?;
        }
        Ok(())
    }

    fn report(stats: &Stats, secs: f64) {
        let per = |d: Duration, n: u32| {
            if n == 0 {
                0.0
            } else {
                d.as_secs_f64() * 1000.0 / f64::from(n)
            }
        };
        #[expect(clippy::cast_precision_loss, reason = "report only")]
        let kbps = stats.bytes as f64 * 8.0 / secs / 1000.0;
        println!(
            "submitted: {} (new desktop images: {}), encoded: {}, keyframes: {} in {secs:.2} s",
            stats.submitted, stats.fresh, stats.encoded, stats.keyframes
        );
        println!("encode fps: {:.1}", f64::from(stats.encoded) / secs);
        println!(
            "avg encode: {:.2} ms (max {:.2} ms), avg capture call: {:.2} ms",
            per(stats.encode_time, stats.submitted),
            stats.max_encode.as_secs_f64() * 1000.0,
            per(stats.capture_time, stats.submitted)
        );
        println!("bitrate: {kbps:.0} kbit/s ({} bytes)", stats.bytes);
    }

    /// Average CPU cost of the BGRA → NV12 / I420 conversions the encoders run per frame.
    fn conversion_cost(last: &LastImage) -> AnyResult<()> {
        use scrin_win::color::{BgraImage, I420, Nv12};
        let img = BgraImage {
            data: &last.data,
            width: last.width as usize & !1,
            height: last.height as usize & !1,
            stride: last.stride,
        };
        let (mut nv12, mut i420) = (Nv12::default(), I420::default());
        nv12.fill_from_bgra(&img)?;
        i420.fill_from_bgra(&img)?;
        let n = 20u32;
        let t = Instant::now();
        for _ in 0..n {
            nv12.fill_from_bgra(&img)?;
        }
        let nv = t.elapsed().as_secs_f64() * 1000.0 / f64::from(n);
        let t = Instant::now();
        for _ in 0..n {
            i420.fill_from_bgra(&img)?;
        }
        let i4 = t.elapsed().as_secs_f64() * 1000.0 / f64::from(n);
        println!("cpu conversion per frame: BGRA->NV12 {nv:.2} ms, BGRA->I420 {i4:.2} ms");
        Ok(())
    }

    pub fn run() -> AnyResult<()> {
        let choice = std::env::args().nth(1).unwrap_or_default();
        let mut cap = DxgiCapture::primary(true)?;
        let d = cap.display().clone();
        println!(
            "display: {} {}x{} @ {} Hz on {}",
            d.name,
            d.bounds.width(),
            d.bounds.height(),
            d.refresh_hz,
            d.adapter
        );
        println!(
            "hardware encoders: {:?}",
            scrin_win::probe_hardware_encoders()?
        );
        let settings = EncoderSettings {
            width: d.bounds.width() & !1,
            height: d.bounds.height() & !1,
            fps: FPS,
            bitrate: 12_000_000,
        };
        let mut enc = make_encoder(&choice, settings)?;
        println!("encoder: {}", enc.name());

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.copilot-tmp");
        std::fs::create_dir_all(&dir)?;
        let file_name = format!(
            "capture-{}.h264",
            enc.name()
                .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        );
        let path = dir.join(file_name);
        let mut file = std::io::BufWriter::new(std::fs::File::create(&path)?);

        let interval = Duration::from_secs_f64(1.0 / f64::from(FPS));
        let mut last = LastImage {
            data: Vec::new(),
            width: 0,
            height: 0,
            stride: 0,
        };
        let mut stats = Stats::default();
        let start = Instant::now();
        let mut next = start;
        while start.elapsed() < RUN {
            capture_into(&mut cap, interval, &mut last, &mut stats)?;
            if last.data.is_empty() {
                continue;
            }
            let pts = u64::try_from(start.elapsed().as_micros()).unwrap_or(0);
            encode_one(
                enc.as_mut(),
                &last,
                pts,
                stats.submitted == 0,
                &mut file,
                &mut stats,
            )?;
            next += interval;
            match next.checked_duration_since(Instant::now()) {
                Some(wait) => std::thread::sleep(wait),
                None => next = Instant::now(),
            }
        }
        file.flush()?;
        report(&stats, start.elapsed().as_secs_f64());
        if !last.data.is_empty() {
            conversion_cost(&last)?;
        }
        println!("wrote {}", path.display());
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    bench::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("capture_encode is Windows-only");
}
