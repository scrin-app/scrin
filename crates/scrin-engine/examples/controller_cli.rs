//! Headless controller (and host) for live tests on real machines.
//!
//! Controller:
//! ```text
//! SCRIN_SERVER=http://<server>:<port> cargo run -p scrin-engine --example controller_cli \
//!     --features win --release -- connect <scrin id | ticket> <code> [--secs 10] [--png out.png]
//! ```
//! Connects by scrin ID + code, confirms the SAS, saves the first decoded
//! frame as PNG and prints one stats line per second.
//!
//! Host (prints ID + code, accepts the first request after its anti-scam delay):
//! ```text
//! SCRIN_SERVER=... cargo run -p scrin-engine --example controller_cli -- host [--secs 120]
//! ```
//!
//! `SCRIN_DATA` overrides the data directory (identity + registered ID);
//! default `%LOCALAPPDATA%\scrin-cli`.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use scrin_engine::{Command, EngineConfig, Event, Reply, SessionState, StatsInfo};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn data_dir(role: &str) -> PathBuf {
    if let Ok(d) = std::env::var("SCRIN_DATA") {
        return PathBuf::from(d);
    }
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join(format!("scrin-cli-{role}"))
}

fn config(role: &str) -> EngineConfig {
    let mut cfg = EngineConfig::new(data_dir(role));
    cfg.server = std::env::var("SCRIN_SERVER").ok().filter(|s| !s.is_empty());
    cfg
}

fn save_png(path: &str, f: &scrin_engine::api::VideoFrame) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), f.width, f.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    let stride = f.stride as usize;
    let mut rgb = Vec::with_capacity(f.width as usize * f.height as usize * 3);
    for row in 0..f.height as usize {
        let line = &f.bgra[row * stride..row * stride + f.width as usize * 4];
        for px in line.as_chunks::<4>().0 {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    w.write_image_data(&rgb).map_err(|e| e.to_string())
}

fn print_stats(s: &StatsInfo, t: f64) {
    println!(
        "STATS t={t:.1}s fps={:.1} rtt_ms={:.1} bitrate_kbps={} loss={:.3} decode_ms={:.2} size={}x{} frames={}",
        s.fps,
        s.rtt_ms,
        s.bitrate_bps / 1000,
        s.loss,
        s.decode_ms,
        s.width,
        s.height,
        s.frames_total
    );
}

async fn run_controller(args: &[String]) -> Result<(), String> {
    let target = args.get(2).ok_or("usage: connect <id> <code>")?.clone();
    let code = args.get(3).ok_or("usage: connect <id> <code>")?.clone();
    let run_for: u64 = arg(args, "--secs")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let png_path = arg(args, "--png").unwrap_or_else(|| "live-frame.png".into());
    let (h, mut ev) = scrin_engine::start(config("ctl"))
        .await
        .map_err(|e| e.to_string())?;
    let st = h.status().await.map_err(|e| e.to_string())?;
    println!(
        "controller device {} backend {}",
        st.fingerprint, st.backend
    );
    let started = Instant::now();
    let Reply::Session(sess) = h
        .call(Command::Connect {
            target,
            code,
            requested: Some(vec!["view".into(), "input".into()]),
        })
        .await
        .map_err(|e| e.to_string())?
    else {
        return Err("no session".into());
    };
    let mut active_at = None;
    let mut saved = false;
    let mut first_frame = None;
    let deadline = Duration::from_secs(90);
    loop {
        if let Some(a) = active_at
            && Instant::now().duration_since(a) > Duration::from_secs(run_for)
        {
            break;
        }
        if active_at.is_none() && started.elapsed() > deadline {
            return Err("timed out waiting for the host to accept".into());
        }
        let Ok(Some(e)) = tokio::time::timeout(Duration::from_secs(1), ev.recv()).await else {
            continue;
        };
        match e {
            Event::Sas { emoji, .. } => {
                println!("SAS {emoji:?} (confirming)");
                let _ = h
                    .call(Command::ConfirmSas {
                        session: sess.clone(),
                        matches: true,
                    })
                    .await;
            }
            Event::StateChanged { state, reason, .. } => {
                println!(
                    "STATE {state:?} {} (+{} ms)",
                    reason.unwrap_or_default(),
                    started.elapsed().as_millis()
                );
                match state {
                    SessionState::Active => active_at = Some(Instant::now()),
                    SessionState::Ended => break,
                    _ => {}
                }
            }
            Event::VideoFrame { frame, .. } => {
                if first_frame.is_none() {
                    first_frame = Some(started.elapsed());
                    println!(
                        "FIRST_FRAME {}x{} after {} ms from connect",
                        frame.width,
                        frame.height,
                        started.elapsed().as_millis()
                    );
                }
                // Skip the first few frames: the stream may start mid-refresh.
                if !saved && frame.frame_id >= 5 {
                    save_png(&png_path, &frame)?;
                    println!("PNG {png_path} frame_id={}", frame.frame_id);
                    saved = true;
                }
            }
            Event::Stats { stats, .. } => print_stats(&stats, started.elapsed().as_secs_f64()),
            Event::Error { code, message, .. } => println!("ERROR {code}: {message}"),
            _ => {}
        }
    }
    let _ = h.call(Command::EndSession { session: sess }).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    h.shutdown().await;
    if saved {
        Ok(())
    } else {
        Err("no frame saved".into())
    }
}

async fn run_host(args: &[String]) -> Result<(), String> {
    let secs: u64 = arg(args, "--secs")
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    let (h, mut ev) = scrin_engine::start(config("host"))
        .await
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let mut announced = String::new();
    while started.elapsed() < Duration::from_secs(secs) {
        let st = h.status().await.map_err(|e| e.to_string())?;
        let line = format!(
            "HOST id={} code={} online={} backend={}",
            st.scrin_id, st.code, st.online, st.backend
        );
        if line != announced {
            println!("{line}");
            announced = line;
        }
        let Ok(Some(e)) = tokio::time::timeout(Duration::from_secs(1), ev.recv()).await else {
            continue;
        };
        match e {
            Event::IncomingRequest {
                session,
                accept_enabled_at,
                allowed,
                ..
            } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0));
                let wait = accept_enabled_at.saturating_sub(now) + 100;
                println!("REQUEST {session}; accepting in {wait} ms");
                tokio::time::sleep(Duration::from_millis(wait)).await;
                let perms: Vec<String> = allowed
                    .into_iter()
                    .filter(|p| p == "view" || p == "input")
                    .collect();
                let r = h
                    .call(Command::Accept {
                        session,
                        permissions: perms,
                    })
                    .await;
                println!("ACCEPT {r:?}");
            }
            Event::StateChanged { state, reason, .. } => {
                println!("STATE {state:?} {}", reason.unwrap_or_default());
            }
            Event::Error { code, message, .. } => println!("ERROR {code}: {message}"),
            _ => {}
        }
    }
    h.shutdown().await;
    Ok(())
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,iroh=warn,noq=warn".into()),
        )
        .with_writer(std::io::stderr)
        .try_init();
    let args: Vec<String> = std::env::args().collect();
    let r = match args.get(1).map(String::as_str) {
        Some("connect") => run_controller(&args).await,
        Some("host") => run_host(&args).await,
        _ => Err(
            "usage: controller_cli connect <id> <code> [--secs N] [--png PATH] | host [--secs N]"
                .into(),
        ),
    };
    if let Err(e) = r {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    }
}
