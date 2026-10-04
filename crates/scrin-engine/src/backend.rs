//! The seam between the engine and the platform media stack.
//!
//! The engine never touches OS capture/codec/input APIs itself; it asks a
//! [`MediaBackend`] for a [`Capturer`], a [`VideoEncoder`] and a
//! [`VideoDecoder`], and hands it input events to inject. `scrin-win`
//! (DXGI + Media Foundation/openh264 + `SendInput`) implements this behind the
//! `win` feature; [`SyntheticBackend`] is a deterministic test pattern so the
//! whole pipeline runs in tests and on machines without a GPU.

use std::fmt::Debug;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub use scrin_media::adapt::EncoderSettings;
use scrin_proto::v1;

#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum BackendError {
    #[error("not supported by the {0} backend")]
    Unsupported(&'static str),
    #[error("backend failure: {0}")]
    Failed(String),
    #[error("corrupt bitstream")]
    Corrupt,
}

/// One monitor of the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

/// A captured BGRA8 frame (top-down rows, `stride` bytes per row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
    /// Monotonic capture time, µs since the capturer was opened.
    pub timestamp_us: u64,
}

/// One encoded access unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

/// A decoded BGRA8 frame ready to present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
}

/// Encoder parameters fixed at open time; bitrate/fps/scale change live.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub settings: EncoderSettings,
}

/// Decoder parameters from the host's `VideoConfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderConfig {
    pub codec: v1::Codec,
    pub width: u32,
    pub height: u32,
    pub codec_config: Vec<u8>,
}

/// Input the controller sends; mirrors the `scrin.v1` input messages.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    Key(v1::KeyEvent),
    MouseMove(v1::MouseMove),
    MouseButton(v1::MouseButton),
    MouseWheel(v1::MouseWheel),
}

/// Capturers, encoders and decoders are opened *on* the media thread that
/// uses them (COM objects are apartment-bound), so they need not be `Send`.
pub trait Capturer: Debug {
    /// Current output size.
    fn size(&self) -> (u32, u32);
    /// The next frame, or `None` if nothing changed within `timeout`.
    fn capture(&mut self, timeout: Duration) -> Result<Option<CapturedFrame>, BackendError>;
}

pub trait VideoEncoder: Debug {
    /// Codec this encoder produces.
    fn codec(&self) -> v1::Codec;
    /// Out-of-band configuration (avcC …) for `VideoConfig.codec_config`.
    fn codec_config(&self) -> Vec<u8>;
    /// Apply new target bitrate / fps / scale.
    fn reconfigure(&mut self, settings: EncoderSettings) -> Result<(), BackendError>;
    /// Encode one frame; `force_keyframe` after loss or at start.
    fn encode(
        &mut self,
        frame: &CapturedFrame,
        force_keyframe: bool,
    ) -> Result<Option<EncodedFrame>, BackendError>;
}

pub trait VideoDecoder: Debug {
    fn decode(&mut self, data: &[u8], keyframe: bool)
    -> Result<Option<DecodedFrame>, BackendError>;
}

/// Platform media stack. Methods are called from several threads.
pub trait MediaBackend: Send + Sync + Debug + 'static {
    fn name(&self) -> &'static str;
    fn displays(&self) -> Vec<Display>;
    fn open_capture(&self, display: u32) -> Result<Box<dyn Capturer>, BackendError>;
    fn open_encoder(&self, config: EncoderConfig) -> Result<Box<dyn VideoEncoder>, BackendError>;
    fn open_decoder(&self, config: &DecoderConfig) -> Result<Box<dyn VideoDecoder>, BackendError>;
    /// Inject one controller input event on the host.
    fn inject(&self, event: &InputEvent) -> Result<(), BackendError>;
}

/// No media at all: a controller-only build or a host whose capture is not
/// available. Every media call fails with [`BackendError::Unsupported`].
#[derive(Debug, Default, Clone, Copy)]
pub struct NullBackend;

impl MediaBackend for NullBackend {
    fn name(&self) -> &'static str {
        "null"
    }
    fn displays(&self) -> Vec<Display> {
        Vec::new()
    }
    fn open_capture(&self, _display: u32) -> Result<Box<dyn Capturer>, BackendError> {
        Err(BackendError::Unsupported("null"))
    }
    fn open_encoder(&self, _config: EncoderConfig) -> Result<Box<dyn VideoEncoder>, BackendError> {
        Err(BackendError::Unsupported("null"))
    }
    fn open_decoder(&self, _config: &DecoderConfig) -> Result<Box<dyn VideoDecoder>, BackendError> {
        Err(BackendError::Unsupported("null"))
    }
    fn inject(&self, _event: &InputEvent) -> Result<(), BackendError> {
        Err(BackendError::Unsupported("null"))
    }
}

/// Marks a synthetic bitstream in `VideoConfig.codec_config` (the codec enum
/// itself stays `UNSPECIFIED`: this is not a real codec).
pub const SYNTHETIC_CODEC_CONFIG: &[u8] = b"scrin-synthetic/1";

const SYN_MAGIC: &[u8; 4] = b"SYN1";
const SYN_HEADER: usize = 4 + 4 + 4 + 8 + 1;

/// Deterministic moving test pattern + a fake codec that sizes its output from
/// the target bitrate (so FEC sharding and BWE see realistic traffic) and a
/// decoder that re-renders the exact pattern. Injected input is recorded.
#[derive(Debug, Clone)]
pub struct SyntheticBackend {
    width: u32,
    height: u32,
    injected: Arc<Mutex<Vec<InputEvent>>>,
}

impl Default for SyntheticBackend {
    fn default() -> Self {
        Self::new(640, 360)
    }
}

impl SyntheticBackend {
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: width.max(16),
            height: height.max(16),
            injected: Arc::default(),
        }
    }

    /// Every input event injected so far.
    #[must_use]
    pub fn injected(&self) -> Vec<InputEvent> {
        self.injected
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Renders frame `n` of the test pattern into `out` (BGRA, stride = w*4).
pub fn render_pattern(width: u32, height: u32, n: u64, out: &mut Vec<u8>) {
    let (cols, rows) = (width as usize, height as usize);
    out.clear();
    out.resize(cols * rows * 4, 0);
    let bar = usize::try_from(n % u64::from(width.max(1))).unwrap_or(0);
    for (row_idx, row) in out.chunks_exact_mut(cols * 4).enumerate() {
        // Truncation is the point: an 8-bit gradient.
        #[expect(clippy::cast_possible_truncation)]
        let green = (row_idx * 255 / rows.max(1)) as u8;
        for (col, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            #[expect(clippy::cast_possible_truncation)]
            let blue = (col * 255 / cols.max(1)) as u8;
            *px = if col.abs_diff(bar) < 8 {
                [255, 255, 255, 255]
            } else {
                [blue, green, 96, 255]
            };
        }
    }
}

#[derive(Debug)]
struct SynCapturer {
    width: u32,
    height: u32,
    n: u64,
    started: Instant,
}

impl Capturer for SynCapturer {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn capture(&mut self, _timeout: Duration) -> Result<Option<CapturedFrame>, BackendError> {
        let mut bgra = Vec::new();
        render_pattern(self.width, self.height, self.n, &mut bgra);
        self.n += 1;
        Ok(Some(CapturedFrame {
            width: self.width,
            height: self.height,
            stride: self.width * 4,
            bgra,
            timestamp_us: u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX),
        }))
    }
}

#[derive(Debug)]
struct SynEncoder {
    width: u32,
    height: u32,
    settings: EncoderSettings,
    n: u64,
}

impl VideoEncoder for SynEncoder {
    fn codec(&self) -> v1::Codec {
        v1::Codec::Unspecified
    }

    fn codec_config(&self) -> Vec<u8> {
        SYNTHETIC_CODEC_CONFIG.to_vec()
    }

    fn reconfigure(&mut self, settings: EncoderSettings) -> Result<(), BackendError> {
        self.settings = settings;
        Ok(())
    }

    fn encode(
        &mut self,
        _frame: &CapturedFrame,
        force_keyframe: bool,
    ) -> Result<Option<EncodedFrame>, BackendError> {
        let fps = self.settings.fps.max(1);
        let budget = (self.settings.bitrate_bps / 8 / fps) as usize;
        let size = if force_keyframe { budget * 3 } else { budget }.clamp(64, 60_000);
        let mut data = Vec::with_capacity(SYN_HEADER + size);
        data.extend_from_slice(SYN_MAGIC);
        data.extend_from_slice(&self.width.to_be_bytes());
        data.extend_from_slice(&self.height.to_be_bytes());
        data.extend_from_slice(&self.n.to_be_bytes());
        data.push(u8::from(force_keyframe));
        // Filler stands in for entropy-coded residuals.
        data.extend((0..size).map(|i| (i as u64 ^ self.n).to_le_bytes()[0]));
        self.n += 1;
        Ok(Some(EncodedFrame {
            data,
            keyframe: force_keyframe,
        }))
    }
}

#[derive(Debug, Default)]
struct SynDecoder {
    scratch: Vec<u8>,
}

impl VideoDecoder for SynDecoder {
    fn decode(
        &mut self,
        data: &[u8],
        _keyframe: bool,
    ) -> Result<Option<DecodedFrame>, BackendError> {
        let head = data.get(..SYN_HEADER).ok_or(BackendError::Corrupt)?;
        if &head[..4] != SYN_MAGIC {
            return Err(BackendError::Corrupt);
        }
        let be32 =
            |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
        let (width, height) = (be32(4), be32(8));
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err(BackendError::Corrupt);
        }
        let mut n = [0u8; 8];
        n.copy_from_slice(&head[12..20]);
        render_pattern(width, height, u64::from_be_bytes(n), &mut self.scratch);
        Ok(Some(DecodedFrame {
            width,
            height,
            stride: width * 4,
            bgra: self.scratch.clone(),
        }))
    }
}

impl MediaBackend for SyntheticBackend {
    fn name(&self) -> &'static str {
        "synthetic"
    }

    fn displays(&self) -> Vec<Display> {
        vec![Display {
            id: 1,
            name: "Synthetic test pattern".into(),
            width: self.width,
            height: self.height,
            primary: true,
        }]
    }

    fn open_capture(&self, _display: u32) -> Result<Box<dyn Capturer>, BackendError> {
        Ok(Box::new(SynCapturer {
            width: self.width,
            height: self.height,
            n: 0,
            started: Instant::now(),
        }))
    }

    fn open_encoder(&self, config: EncoderConfig) -> Result<Box<dyn VideoEncoder>, BackendError> {
        Ok(Box::new(SynEncoder {
            width: config.width,
            height: config.height,
            settings: config.settings,
            n: 0,
        }))
    }

    fn open_decoder(&self, config: &DecoderConfig) -> Result<Box<dyn VideoDecoder>, BackendError> {
        if config.codec_config != SYNTHETIC_CODEC_CONFIG {
            return Err(BackendError::Unsupported("synthetic"));
        }
        Ok(Box::new(SynDecoder::default()))
    }

    fn inject(&self, event: &InputEvent) -> Result<(), BackendError> {
        self.injected
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.clone());
        Ok(())
    }
}

/// The best backend this build has: DXGI + Media Foundation/openh264 +
/// `SendInput` (feature `win`, when a display is attached), otherwise the
/// synthetic pattern.
#[must_use]
pub fn default_backend() -> Arc<dyn MediaBackend> {
    #[cfg(all(windows, feature = "win"))]
    if let Some(b) = crate::win_backend::WinBackend::open() {
        return Arc::new(b);
    }
    Arc::new(SyntheticBackend::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_round_trip_renders_the_same_pattern() {
        let b = SyntheticBackend::new(64, 32);
        let mut cap = b.open_capture(1).expect("capture");
        let mut enc = b
            .open_encoder(EncoderConfig {
                width: 64,
                height: 32,
                settings: EncoderSettings {
                    bitrate_bps: 2_000_000,
                    fps: 30,
                    scale: 1.0,
                },
            })
            .expect("encoder");
        let mut dec = b
            .open_decoder(&DecoderConfig {
                codec: enc.codec(),
                width: 64,
                height: 32,
                codec_config: enc.codec_config(),
            })
            .expect("decoder");
        for _ in 0..3 {
            let f = cap.capture(Duration::ZERO).expect("cap").expect("frame");
            let e = enc.encode(&f, true).expect("enc").expect("au");
            let d = dec
                .decode(&e.data, e.keyframe)
                .expect("dec")
                .expect("frame");
            assert_eq!(d.bgra, f.bgra);
        }
    }

    #[test]
    fn decoder_rejects_garbage_and_null_backend_refuses() {
        let mut dec = SynDecoder::default();
        assert!(dec.decode(b"nope", false).is_err());
        assert!(NullBackend.open_capture(1).is_err());
    }
}
