//! Software H.264 encoding with Cisco's openh264 (BSD, built from source).
//!
//! Configured for interactive screen content: screen-content usage type, bitrate rate control
//! with frame skipping (openh264 only enforces the bitrate when it may skip; a skipped frame is
//! reported as "no output"), no B-frames (openh264 never emits them), periodic IDR disabled
//! (keyframes only on request or after a discontinuity), scene-change detection on (required for
//! screen content), and BT.709 limited-range VUI matching [`crate::color`].

use openh264::OpenH264API;
use openh264::Timestamp;
use openh264::encoder::{
    BitRate, Complexity, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod,
    RateControlMode, UsageType, VuiConfig,
};
use openh264::formats::YUVSlices;
use openh264_sys2::{
    ENCODER_OPTION_BITRATE, ENCODER_OPTION_FRAME_RATE, SBitrateInfo, SPATIAL_LAYER_ALL,
};

use super::EncoderSettings;
use crate::color::{BgraImage, I420, chroma_len};
use crate::{CapturedFrame, EncodedFrame, Error, PixelData, Result, VideoEncoder};

fn codec(e: &openh264::Error) -> Error {
    Error::Codec(format!("openh264: {e}"))
}

/// openh264 software encoder.
pub struct OpenH264Encoder {
    encoder: Encoder,
    settings: EncoderSettings,
    yuv: I420,
    out: Vec<u8>,
}

impl std::fmt::Debug for OpenH264Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenH264Encoder")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl OpenH264Encoder {
    /// Creates an encoder. Width/height only bound the first frame's allocation; the encoder
    /// re-initialises when the frame size changes.
    pub fn new(settings: EncoderSettings) -> Result<Self> {
        #[expect(clippy::cast_precision_loss, reason = "frame rates are small integers")]
        let fps = settings.fps.max(1) as f32;
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(settings.bitrate))
            .max_frame_rate(FrameRate::from_hz(fps))
            .skip_frames(true)
            .intra_frame_period(IntraFramePeriod::from_num_frames(0))
            .complexity(Complexity::Low)
            .scene_change_detect(true)
            // Not supported for screen content; openh264 would turn them off with a warning.
            .background_detection(false)
            .adaptive_quantization(false)
            .vui(VuiConfig::bt709());
        let encoder =
            Encoder::with_api_config(OpenH264API::from_source(), config).map_err(|e| codec(&e))?;
        Ok(Self {
            encoder,
            settings,
            yuv: I420::default(),
            out: Vec::new(),
        })
    }

    /// Encodes an already converted, tightly packed I420 frame (even dimensions).
    pub fn encode_i420(
        &mut self,
        frame: &I420,
        pts_us: u64,
        force_keyframe: bool,
    ) -> Result<EncodedFrame> {
        Self::encode_planes(
            &mut self.encoder,
            &mut self.out,
            frame,
            pts_us,
            force_keyframe,
        )
    }

    fn encode_planes(
        encoder: &mut Encoder,
        out: &mut Vec<u8>,
        frame: &I420,
        pts_us: u64,
        force_keyframe: bool,
    ) -> Result<EncodedFrame> {
        let (w, h) = (frame.width & !1, frame.height & !1);
        if w == 0 || h == 0 {
            return Err(Error::InvalidInput("frame smaller than 2x2".into()));
        }
        let cw = chroma_len(frame.width);
        // Odd sizes are cropped to even: openh264 needs even dimensions. The strides stay the
        // tight strides of the source planes.
        let yuv = YUVSlices::new(
            (
                &frame.y[..frame.width * h],
                &frame.u[..cw * (h / 2)],
                &frame.v[..cw * (h / 2)],
            ),
            (w, h),
            (frame.width, cw, cw),
        );
        if force_keyframe {
            encoder.force_intra_frame();
        }
        let bs = encoder
            .encode_at(&yuv, Timestamp::from_millis(pts_us / 1000))
            .map_err(|e| codec(&e))?;
        out.clear();
        bs.write_vec(out);
        let keyframe = matches!(bs.frame_type(), FrameType::IDR | FrameType::I);
        Ok(EncodedFrame {
            data: out.clone(),
            keyframe,
            pts_us,
        })
    }
}

impl VideoEncoder for OpenH264Encoder {
    fn name(&self) -> &'static str {
        "openh264"
    }

    fn encode(
        &mut self,
        frame: &CapturedFrame<'_>,
        force_keyframe: bool,
    ) -> Result<Option<EncodedFrame>> {
        let PixelData::Cpu(bytes) = frame.data else {
            return Err(Error::InvalidInput(
                "openh264 needs CPU frames (open the capture with want_cpu)".into(),
            ));
        };
        self.yuv.fill_from_bgra(&BgraImage {
            data: bytes,
            width: frame.width as usize,
            height: frame.height as usize,
            stride: frame.stride,
        })?;
        let force = force_keyframe || frame.discontinuity;
        let ef = Self::encode_planes(
            &mut self.encoder,
            &mut self.out,
            &self.yuv,
            frame.pts_us,
            force,
        )?;
        // A skipped frame produces no bytes; report it as "no output" rather than an empty unit.
        Ok((!ef.data.is_empty()).then_some(ef))
    }

    fn set_bitrate(&mut self, bps: u32) -> Result<()> {
        let mut info = SBitrateInfo {
            iLayer: SPATIAL_LAYER_ALL,
            iBitrate: i32::try_from(bps)
                .map_err(|_| Error::InvalidInput("bitrate too large".into()))?,
        };
        // SAFETY: ENCODER_OPTION_BITRATE takes a pointer to SBitrateInfo, which outlives the call.
        // We only change the target bitrate, which the encoder supports at runtime.
        let rc = unsafe {
            self.encoder
                .raw_api()
                .set_option(ENCODER_OPTION_BITRATE, (&raw mut info).cast())
        };
        if rc != 0 {
            return Err(Error::Codec(format!("openh264 SetOption(BITRATE) = {rc}")));
        }
        self.settings.bitrate = bps;
        Ok(())
    }

    fn set_fps(&mut self, fps: u32) -> Result<()> {
        #[expect(clippy::cast_precision_loss, reason = "frame rates are small integers")]
        let mut rate = fps.max(1) as f32;
        // SAFETY: ENCODER_OPTION_FRAME_RATE takes a pointer to a float that outlives the call.
        let rc = unsafe {
            self.encoder
                .raw_api()
                .set_option(ENCODER_OPTION_FRAME_RATE, (&raw mut rate).cast())
        };
        if rc != 0 {
            return Err(Error::Codec(format!(
                "openh264 SetOption(FRAME_RATE) = {rc}"
            )));
        }
        self.settings.fps = fps;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Rect;

    #[expect(
        clippy::many_single_char_names,
        reason = "pixel coordinates and channels"
    )]
    pub(crate) fn synthetic_bgra(w: usize, h: usize, t: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let b = u8::try_from((x + t * 3) % 256).expect("b");
                let g = u8::try_from((y * 2 + t) % 256).expect("g");
                let r = u8::try_from(((x / 16 + y / 16) % 2) * 160 + 40).expect("r");
                v.extend_from_slice(&[b, g, r, 255]);
            }
        }
        v
    }

    pub(crate) fn frame(data: &[u8], w: u32, h: u32, pts_us: u64) -> CapturedFrame<'_> {
        CapturedFrame {
            width: w,
            height: h,
            stride: w as usize * 4,
            data: PixelData::Cpu(data),
            image_updated: true,
            dirty_rects: &[] as &[Rect],
            move_rects: &[],
            pts_us,
            cursor: None,
            discontinuity: false,
        }
    }

    #[test]
    fn first_frame_is_idr_and_force_works() {
        let mut enc = OpenH264Encoder::new(EncoderSettings {
            width: 320,
            height: 240,
            fps: 30,
            bitrate: 1_000_000,
        })
        .expect("encoder");
        let img = synthetic_bgra(320, 240, 0);
        let f0 = enc
            .encode(&frame(&img, 320, 240, 0), false)
            .expect("encode")
            .expect("output");
        assert!(f0.keyframe);
        assert_eq!(&f0.data[..4], &[0, 0, 0, 1], "Annex B start code");
        // Rate control may skip a frame or two right after the large IDR.
        let img1 = synthetic_bgra(320, 240, 1);
        let f1 = (1..10u64)
            .find_map(|i| {
                enc.encode(&frame(&img1, 320, 240, i * 33_333), false)
                    .expect("encode")
            })
            .expect("a P-frame within 10 frames");
        assert!(!f1.keyframe);
        assert!(f1.data.len() < f0.data.len());
        enc.set_bitrate(500_000).expect("bitrate");
        enc.set_fps(60).expect("fps");
        let f2 = enc
            .encode(&frame(&img1, 320, 240, 666_666), true)
            .expect("encode")
            .expect("forced IDR");
        assert!(f2.keyframe);
        assert_eq!(f2.pts_us, 666_666);
    }

    #[test]
    fn rejects_texture_less_input_shape() {
        let mut enc = OpenH264Encoder::new(EncoderSettings {
            width: 64,
            height: 64,
            fps: 30,
            bitrate: 300_000,
        })
        .expect("encoder");
        let short = vec![0u8; 16];
        assert!(enc.encode(&frame(&short, 64, 64, 0), false).is_err());
    }
}
