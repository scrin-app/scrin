//! Windows capture, encode, decode, input, audio and clipboard for scrin.
//!
//! The traits and data types at the top level are portable so the session engine can be written
//! against them on every target. The Windows implementations live in `win` (compiled only on
//! Windows); on other targets the crate builds as the portable core plus `Unsupported` stubs.
//!
//! Pure helpers (colour conversion, HID → scancode table, resampling, cursor shape conversion) are
//! portable and unit-tested on every target.

pub mod color;
pub mod coords;
pub mod cursor;
pub mod hid;
pub mod resample;
pub mod sas;

#[cfg(windows)]
pub mod win;

use std::time::Duration;

/// Errors from the platform layer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The operation is not available on this platform or with this hardware.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
    /// An OS call failed.
    #[error("{context} failed: {message} (0x{code:08X})")]
    Os {
        /// The API that failed.
        context: &'static str,
        /// Raw `HRESULT` / error code.
        code: u32,
        /// System message for the code.
        message: String,
    },
    /// The encoder or decoder rejected the data or its configuration.
    #[error("codec: {0}")]
    Codec(String),
    /// The caller passed data that does not match the expected shape.
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Axis-aligned rectangle in pixels; `right`/`bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// Left edge (inclusive).
    pub left: i32,
    /// Top edge (inclusive).
    pub top: i32,
    /// Right edge (exclusive).
    pub right: i32,
    /// Bottom edge (exclusive).
    pub bottom: i32,
}

impl Rect {
    /// Width in pixels (0 when inverted).
    #[must_use]
    pub fn width(&self) -> u32 {
        u32::try_from(self.right.saturating_sub(self.left)).unwrap_or(0)
    }

    /// Height in pixels (0 when inverted).
    #[must_use]
    pub fn height(&self) -> u32 {
        u32::try_from(self.bottom.saturating_sub(self.top)).unwrap_or(0)
    }
}

/// A region that moved between two frames (scrolling, window drag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveRect {
    /// Top-left of the source region in the previous frame.
    pub source: (i32, i32),
    /// Destination region in the current frame.
    pub destination: Rect,
}

/// A monitor that can be captured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayInfo {
    /// Index in enumeration order; pass to the capture constructor.
    pub index: u32,
    /// OS device name (e.g. `\\.\DISPLAY1`).
    pub name: String,
    /// GPU the output is attached to.
    pub adapter: String,
    /// Position and size on the virtual desktop.
    pub bounds: Rect,
    /// Whether this is the primary display (origin at 0,0).
    pub primary: bool,
    /// Current refresh rate in Hz (0 when unknown).
    pub refresh_hz: u32,
}

/// Pixels of a captured frame. Always BGRA8 (`B, G, R, A` byte order).
#[derive(Debug)]
pub enum PixelData<'a> {
    /// GPU texture owned by the capture source; valid until the next `next_frame` call.
    #[cfg(windows)]
    Texture(&'a windows::Win32::Graphics::Direct3D11::ID3D11Texture2D),
    /// CPU copy; `stride` bytes per row.
    Cpu(&'a [u8]),
}

/// Cursor shape in straight (non-premultiplied) RGBA8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorShape {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Hotspot within the shape.
    pub hotspot: (i32, i32),
    /// `width * height * 4` bytes, RGBA.
    pub rgba: Vec<u8>,
}

/// Cursor state accompanying a frame; sent on its own channel and drawn client-side.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CursorInfo {
    /// Position of the shape's top-left relative to the display, when it changed.
    pub position: Option<(i32, i32)>,
    /// Whether the cursor is visible on this display.
    pub visible: bool,
    /// New shape, only when the shape changed.
    pub shape: Option<CursorShape>,
}

/// One captured desktop frame.
#[derive(Debug)]
pub struct CapturedFrame<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row of [`PixelData::Cpu`] (0 for textures).
    pub stride: usize,
    /// The pixels.
    pub data: PixelData<'a>,
    /// `false` when only the cursor changed; `data` then holds the previous image.
    pub image_updated: bool,
    /// Regions that changed since the previous frame (empty = unknown/whole frame).
    pub dirty_rects: &'a [Rect],
    /// Regions that moved since the previous frame.
    pub move_rects: &'a [MoveRect],
    /// Microseconds since the capture source started.
    pub pts_us: u64,
    /// Cursor update, if any.
    pub cursor: Option<CursorInfo>,
    /// The source was recreated (desktop switch, mode change): encoders must emit a keyframe.
    pub discontinuity: bool,
}

/// A source of desktop frames (one display).
pub trait CaptureSource {
    /// Waits up to `timeout` for the next frame. `Ok(None)` means nothing changed.
    fn next_frame(&mut self, timeout: Duration) -> Result<Option<CapturedFrame<'_>>>;
    /// Displays this source can capture.
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
}

/// One compressed access unit (Annex B H.264).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    /// Annex B bytes (start codes included; SPS/PPS precede keyframes).
    pub data: Vec<u8>,
    /// Whether this is an IDR frame.
    pub keyframe: bool,
    /// Presentation timestamp in microseconds (copied from the input frame).
    pub pts_us: u64,
}

/// A video encoder.
pub trait VideoEncoder {
    /// Human-readable encoder name (MFT friendly name or `openh264`).
    fn name(&self) -> &str;
    /// Encodes a frame. Asynchronous hardware encoders may return the output of an earlier frame
    /// or `None` while the pipeline fills.
    fn encode(
        &mut self,
        frame: &CapturedFrame<'_>,
        force_keyframe: bool,
    ) -> Result<Option<EncodedFrame>>;
    /// Changes the target bitrate in bits per second.
    fn set_bitrate(&mut self, bps: u32) -> Result<()>;
    /// Changes the target frame rate.
    fn set_fps(&mut self, fps: u32) -> Result<()>;
}

/// A decoded picture in tightly packed I420 (BT.709 limited range).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodedFrame {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Luma plane, `width * height`.
    pub y: Vec<u8>,
    /// Cb plane, `ceil(w/2) * ceil(h/2)`.
    pub u: Vec<u8>,
    /// Cr plane, `ceil(w/2) * ceil(h/2)`.
    pub v: Vec<u8>,
}

/// A video decoder.
pub trait VideoDecoder {
    /// Decodes one access unit; `None` when no picture is ready yet.
    fn decode(&mut self, data: &[u8]) -> Result<Option<&DecodedFrame>>;
}

/// Mouse buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Primary button.
    Left,
    /// Secondary button.
    Right,
    /// Wheel button.
    Middle,
    /// Back.
    X1,
    /// Forward.
    X2,
}

/// Injects keyboard and mouse input into the local session.
pub trait InputInjector {
    /// Presses or releases a key by USB HID usage (`page << 16 | id`; a bare id is page 0x07).
    fn key(&mut self, hid_usage: u32, down: bool) -> Result<()>;
    /// Types text independent of the keyboard layout.
    fn unicode(&mut self, text: &str) -> Result<()>;
    /// Moves to a normalised position (0..=1) on the target display.
    fn mouse_move_abs(&mut self, x: f32, y: f32) -> Result<()>;
    /// Moves relative to the current position (games, pointer lock).
    fn mouse_move_rel(&mut self, dx: i32, dy: i32) -> Result<()>;
    /// Presses or releases a button.
    fn button(&mut self, button: MouseButton, down: bool) -> Result<()>;
    /// Scrolls; 120 = one notch, smaller values for high-resolution wheels/touchpads.
    fn wheel(&mut self, dx: i32, dy: i32) -> Result<()>;
}

/// Audio sample rate of [`AudioFrame`]s.
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
/// Channels of [`AudioFrame`]s (interleaved).
pub const AUDIO_CHANNELS: usize = 2;
/// Samples per channel in one 10 ms [`AudioFrame`].
pub const AUDIO_FRAME_SAMPLES: usize = 480;

/// 10 ms of interleaved stereo 48 kHz PCM.
#[derive(Debug)]
pub struct AudioFrame<'a> {
    /// `AUDIO_FRAME_SAMPLES * AUDIO_CHANNELS` samples, interleaved L/R.
    pub samples: &'a [i16],
    /// Microseconds since capture started (sample-clock based).
    pub pts_us: u64,
}

/// Captures what the machine is playing.
pub trait AudioCapture {
    /// Waits up to `timeout` for the next 10 ms frame. `None` when nothing arrived in time.
    fn next_frame(&mut self, timeout: Duration) -> Result<Option<AudioFrame<'_>>>;
}

/// Clipboard payload; any combination of formats may be present.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipboardContent {
    /// Unicode text.
    pub text: Option<String>,
    /// PNG-encoded image (registered `PNG` format).
    pub png: Option<Vec<u8>>,
    /// Device-independent bitmap (`CF_DIB`: `BITMAPINFOHEADER` + pixels).
    pub dib: Option<Vec<u8>>,
}

impl ClipboardContent {
    /// True when no format is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.png.is_none() && self.dib.is_none()
    }
}

/// Watches the local clipboard and writes remote content into it.
pub trait ClipboardWatcher {
    /// Waits up to `timeout` for a local clipboard change (our own writes are suppressed).
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<ClipboardContent>>;
    /// Replaces the clipboard content.
    fn set(&mut self, content: &ClipboardContent) -> Result<()>;
}

/// A video encoder available on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderInfo {
    /// Friendly name.
    pub name: String,
    /// PCI vendor tag (e.g. `VEN_10DE`), empty for software.
    pub vendor: String,
    /// Whether the encoder runs on the GPU.
    pub hardware: bool,
}

/// Lists displays that can be captured.
#[cfg(windows)]
pub fn list_displays() -> Result<Vec<DisplayInfo>> {
    win::capture_dxgi::list_displays()
}

/// Lists displays that can be captured.
#[cfg(not(windows))]
pub fn list_displays() -> Result<Vec<DisplayInfo>> {
    Err(Error::Unsupported(
        "display capture is Windows-only in scrin-win",
    ))
}

/// Lists hardware H.264 encoders (Media Foundation MFTs).
#[cfg(windows)]
pub fn probe_hardware_encoders() -> Result<Vec<EncoderInfo>> {
    win::encode_mf::probe_encoders(true)
}

/// Lists hardware H.264 encoders (Media Foundation MFTs).
#[cfg(not(windows))]
pub fn probe_hardware_encoders() -> Result<Vec<EncoderInfo>> {
    Err(Error::Unsupported("Media Foundation is Windows-only"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_size_saturates() {
        let r = Rect {
            left: 10,
            top: 20,
            right: 30,
            bottom: 25,
        };
        assert_eq!((r.width(), r.height()), (20, 5));
        let inverted = Rect {
            left: 5,
            top: 5,
            right: 0,
            bottom: 0,
        };
        assert_eq!((inverted.width(), inverted.height()), (0, 0));
    }

    #[test]
    fn clipboard_empty() {
        assert!(ClipboardContent::default().is_empty());
        let c = ClipboardContent {
            text: Some("x".into()),
            ..ClipboardContent::default()
        };
        assert!(!c.is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn stubs_report_unsupported() {
        assert!(matches!(list_displays(), Err(Error::Unsupported(_))));
        assert!(matches!(
            probe_hardware_encoders(),
            Err(Error::Unsupported(_))
        ));
    }
}
