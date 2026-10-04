//! [`MediaBackend`] over `scrin-win`: DXGI Desktop Duplication capture (CPU
//! copy), the best H.264 encoder (hardware MFT, else openh264), openh264
//! decoding and `SendInput`. Built only with the `win` feature on Windows.
//!
//! Scaling (`EncoderSettings::scale`) is not applied yet: scrin-win encoders
//! are opened at the capture size and only bitrate/fps change live.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use scrin_proto::v1;
use scrin_win::win::capture_dxgi::{DxgiCapture, virtual_desktop};
use scrin_win::win::decode_openh264::OpenH264Decoder;
use scrin_win::win::encode_openh264::OpenH264Encoder;
use scrin_win::win::input::SendInputInjector;
use scrin_win::win::{EncoderSettings as WinSettings, best_encoder};
use scrin_win::{
    CaptureSource, InputInjector, MouseButton as WinButton, PixelData, VideoDecoder as _,
};

use crate::backend::{
    BackendError, CapturedFrame, Capturer, DecodedFrame, DecoderConfig, Display, EncodedFrame,
    EncoderConfig, EncoderSettings, InputEvent, MediaBackend, VideoDecoder, VideoEncoder,
};

fn err(e: &scrin_win::Error) -> BackendError {
    BackendError::Failed(e.to_string())
}

#[derive(Debug)]
pub struct WinBackend {
    injector: Mutex<SendInputInjector>,
    /// Spots Ctrl+Alt+Del so it goes to the service as a real SAS.
    chord: Mutex<scrin_win::sas::ChordDetector>,
}

impl WinBackend {
    /// `None` when there is no display to capture (service session, CI).
    #[must_use]
    pub fn open() -> Option<Self> {
        let injector = SendInputInjector::primary().ok()?;
        Some(Self {
            injector: Mutex::new(injector),
            chord: Mutex::default(),
        })
    }
}

#[derive(Debug)]
struct WinCapturer {
    inner: DxgiCapture,
    last: Option<CapturedFrame>,
}

impl Capturer for WinCapturer {
    fn size(&self) -> (u32, u32) {
        let b = self.inner.display().bounds;
        (b.width(), b.height())
    }

    fn capture(&mut self, timeout: Duration) -> Result<Option<CapturedFrame>, BackendError> {
        let Some(f) = self.inner.next_frame(timeout).map_err(|e| err(&e))? else {
            // Static desktop: repeat the last image so fps-paced encoders
            // still produce (cheap) frames and late keyframes stay possible.
            return Ok(self.last.clone());
        };
        let PixelData::Cpu(bytes) = f.data else {
            return Err(BackendError::Unsupported("win capture without CPU copy"));
        };
        let frame = CapturedFrame {
            width: f.width,
            height: f.height,
            stride: u32::try_from(f.stride).unwrap_or(f.width * 4),
            bgra: bytes.to_vec(),
            timestamp_us: f.pts_us,
        };
        self.last = Some(frame.clone());
        Ok(Some(frame))
    }
}

struct WinEncoder {
    inner: Box<dyn scrin_win::VideoEncoder>,
    settings: EncoderSettings,
    /// Open size, for re-opening a software encoder.
    width: u32,
    height: u32,
    /// Already on openh264 (a hardware MFT failed while encoding).
    software: bool,
}

impl WinEncoder {
    fn win_settings(&self) -> WinSettings {
        WinSettings {
            width: self.width,
            height: self.height,
            fps: self.settings.fps,
            bitrate: self.settings.bitrate_bps,
        }
    }
}

impl std::fmt::Debug for WinEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinEncoder")
            .field("name", &self.inner.name())
            .finish_non_exhaustive()
    }
}

impl VideoEncoder for WinEncoder {
    fn codec(&self) -> v1::Codec {
        v1::Codec::H264
    }

    fn codec_config(&self) -> Vec<u8> {
        // Annex B with SPS/PPS in every keyframe: nothing out of band.
        Vec::new()
    }

    fn reconfigure(&mut self, s: EncoderSettings) -> Result<(), BackendError> {
        if s.bitrate_bps != self.settings.bitrate_bps {
            self.inner.set_bitrate(s.bitrate_bps).map_err(|e| err(&e))?;
        }
        if s.fps != self.settings.fps {
            self.inner.set_fps(s.fps).map_err(|e| err(&e))?;
        }
        self.settings = s;
        Ok(())
    }

    fn encode(
        &mut self,
        frame: &CapturedFrame,
        force_keyframe: bool,
    ) -> Result<Option<EncodedFrame>, BackendError> {
        let f = scrin_win::CapturedFrame {
            width: frame.width,
            height: frame.height,
            stride: frame.stride as usize,
            data: PixelData::Cpu(&frame.bgra),
            image_updated: true,
            dirty_rects: &[],
            move_rects: &[],
            pts_us: frame.timestamp_us,
            cursor: None,
            discontinuity: false,
        };
        let out = match self.inner.encode(&f, force_keyframe) {
            Ok(o) => o,
            Err(e) if !self.software => {
                // Some hardware MFTs (Intel Quick Sync async MFT seen on a
                // laptop: ProcessOutput 0x8000FFFF on the first frame) open
                // fine and then fail. Switch to openh264 for the rest of the
                // stream; the new encoder starts with a keyframe.
                tracing::warn!(error = %e, encoder = self.inner.name(), "hardware encoder failed; switching to openh264");
                self.inner =
                    Box::new(OpenH264Encoder::new(self.win_settings()).map_err(|e| err(&e))?);
                self.software = true;
                self.inner.encode(&f, true).map_err(|e| err(&e))?
            }
            Err(e) => return Err(err(&e)),
        };
        Ok(out.map(|e| EncodedFrame {
            data: e.data,
            keyframe: e.keyframe,
        }))
    }
}

struct WinDecoder {
    inner: OpenH264Decoder,
    i420: scrin_win::color::I420,
}

impl std::fmt::Debug for WinDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WinDecoder(openh264)")
    }
}

impl VideoDecoder for WinDecoder {
    fn decode(
        &mut self,
        data: &[u8],
        _keyframe: bool,
    ) -> Result<Option<DecodedFrame>, BackendError> {
        let Some(pic) = self.inner.decode(data).map_err(|e| err(&e))? else {
            return Ok(None);
        };
        self.i420.width = pic.width;
        self.i420.height = pic.height;
        self.i420.y.clone_from(&pic.y);
        self.i420.u.clone_from(&pic.u);
        self.i420.v.clone_from(&pic.v);
        let mut bgra = Vec::new();
        scrin_win::color::i420_to_bgra(&self.i420, &mut bgra).map_err(|e| err(&e))?;
        let width = u32::try_from(pic.width).map_err(|_| BackendError::Corrupt)?;
        let height = u32::try_from(pic.height).map_err(|_| BackendError::Corrupt)?;
        Ok(Some(DecodedFrame {
            width,
            height,
            stride: width * 4,
            bgra,
        }))
    }
}

impl MediaBackend for WinBackend {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn displays(&self) -> Vec<Display> {
        scrin_win::list_displays()
            .unwrap_or_default()
            .into_iter()
            .map(|d| Display {
                id: d.index,
                name: d.name,
                width: d.bounds.width(),
                height: d.bounds.height(),
                primary: d.primary,
            })
            .collect()
    }

    fn open_capture(&self, display: u32) -> Result<Box<dyn Capturer>, BackendError> {
        let inner = DxgiCapture::new(display, true)
            .or_else(|_| DxgiCapture::primary(true))
            .map_err(|e| err(&e))?;
        if let Ok(desk) = virtual_desktop() {
            self.injector
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .set_display(inner.display().bounds, desk);
        }
        Ok(Box::new(WinCapturer { inner, last: None }))
    }

    fn open_encoder(&self, config: EncoderConfig) -> Result<Box<dyn VideoEncoder>, BackendError> {
        let (width, height) = (config.width & !1, config.height & !1);
        let inner = best_encoder(WinSettings {
            width,
            height,
            fps: config.settings.fps,
            bitrate: config.settings.bitrate_bps,
        })
        .map_err(|e| err(&e))?;
        Ok(Box::new(WinEncoder {
            inner,
            settings: config.settings,
            width,
            height,
            software: false,
        }))
    }

    fn open_decoder(&self, config: &DecoderConfig) -> Result<Box<dyn VideoDecoder>, BackendError> {
        if config.codec != v1::Codec::H264 {
            return Err(BackendError::Unsupported("windows decoder: H.264 only"));
        }
        Ok(Box::new(WinDecoder {
            inner: OpenH264Decoder::new().map_err(|e| err(&e))?,
            i420: scrin_win::color::I420::default(),
        }))
    }

    fn inject(&self, event: &InputEvent) -> Result<(), BackendError> {
        if let InputEvent::Key(k) = event
            && k.text.is_none()
            && self
                .chord
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .on_key(k.hid_usage, k.down)
        {
            // Injected Ctrl+Alt+Del does nothing; the service can send the
            // real one. Without the service, fall through and inject anyway.
            match scrin_win::win::sas_client::request_sas() {
                Ok(()) => return Ok(()),
                Err(e) => tracing::debug!(error = %e, "secure attention sequence unavailable"),
            }
        }
        let mut inj = self.injector.lock().unwrap_or_else(PoisonError::into_inner);
        let r = match event {
            InputEvent::Key(k) => match &k.text {
                Some(t) if k.down && !t.is_empty() => inj.unicode(t),
                Some(_) => Ok(()),
                None => inj.key(k.hid_usage, k.down),
            },
            InputEvent::MouseMove(m) => match m.motion {
                Some(v1::mouse_move::Motion::Absolute(a)) => inj.mouse_move_abs(a.x, a.y),
                Some(v1::mouse_move::Motion::Relative(r)) => inj.mouse_move_rel(r.dx, r.dy),
                None => Ok(()),
            },
            InputEvent::MouseButton(b) => {
                let button = match v1::MouseButtonKind::try_from(b.button) {
                    Ok(v1::MouseButtonKind::Right) => WinButton::Right,
                    Ok(v1::MouseButtonKind::Middle) => WinButton::Middle,
                    Ok(v1::MouseButtonKind::Back) => WinButton::X1,
                    Ok(v1::MouseButtonKind::Forward) => WinButton::X2,
                    _ => WinButton::Left,
                };
                inj.button(button, b.down)
            }
            InputEvent::MouseWheel(w) => inj.wheel(w.delta_x, w.delta_y),
        };
        r.map_err(|e| err(&e))
    }
}
