//! Software H.264 decoding with openh264 → tightly packed I420.
//!
//! Used by the Windows client when no hardware decoder is available and by tests to verify the
//! encoders' output. Output planes are copied out of openh264's padded buffers into a reused
//! [`DecodedFrame`].

use openh264::OpenH264API;
use openh264::decoder::{DecodeOptions, Decoder, DecoderConfig, Flush};
use openh264::formats::YUVSource;

use crate::color::chroma_len;
use crate::{DecodedFrame, Error, Result, VideoDecoder};

/// openh264 software decoder.
pub struct OpenH264Decoder {
    decoder: Decoder,
    frame: DecodedFrame,
}

impl std::fmt::Debug for OpenH264Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenH264Decoder").finish_non_exhaustive()
    }
}

impl OpenH264Decoder {
    /// Creates a decoder that outputs each picture as soon as it is complete (no reordering
    /// delay — scrin streams never contain B-frames).
    pub fn new() -> Result<Self> {
        let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
        let decoder = Decoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(|e| Error::Codec(format!("openh264 decoder: {e}")))?;
        Ok(Self {
            decoder,
            frame: DecodedFrame::default(),
        })
    }
}

fn copy_plane(dst: &mut Vec<u8>, src: &[u8], stride: usize, width: usize, height: usize) {
    dst.clear();
    for row in src.chunks(stride).take(height) {
        dst.extend_from_slice(&row[..width.min(row.len())]);
    }
}

impl VideoDecoder for OpenH264Decoder {
    fn decode(&mut self, data: &[u8]) -> Result<Option<&DecodedFrame>> {
        let pic = self
            .decoder
            .decode_with_options(
                data,
                DecodeOptions::new().flush_after_decode(Flush::NoFlush),
            )
            .map_err(|e| Error::Codec(format!("openh264 decode: {e}")))?;
        let Some(pic) = pic else { return Ok(None) };
        let (w, h) = pic.dimensions();
        let (sy, su, sv) = pic.strides();
        let (cw, ch) = (chroma_len(w), chroma_len(h));
        let f = &mut self.frame;
        f.width = w;
        f.height = h;
        copy_plane(&mut f.y, pic.y(), sy, w, h);
        copy_plane(&mut f.u, pic.u(), su, cw, ch);
        copy_plane(&mut f.v, pic.v(), sv, cw, ch);
        Ok(Some(&self.frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VideoEncoder;
    use crate::color::{BgraImage, I420, psnr};
    use crate::win::EncoderSettings;
    use crate::win::encode_openh264::OpenH264Encoder;

    #[expect(
        clippy::many_single_char_names,
        reason = "pixel coordinates and channels"
    )]
    fn synthetic(w: usize, h: usize, t: usize) -> Vec<u8> {
        // Smooth gradients plus a moving box: realistic screen-ish content.
        let mut v = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let inside = (x + w - (t * 4) % w) % w < 40 && y > 40 && y < 100;
                let (b, g, r) = if inside {
                    (30, 200, 240)
                } else {
                    (x * 255 / w, y * 255 / h, (x + y) * 127 / (w + h) + 64)
                };
                v.extend_from_slice(&[
                    u8::try_from(b).expect("b"),
                    u8::try_from(g).expect("g"),
                    u8::try_from(r).expect("r"),
                    255,
                ]);
            }
        }
        v
    }

    #[test]
    fn encode_decode_round_trip_psnr_above_30db() {
        let (w, h) = (320usize, 240usize);
        let mut enc = OpenH264Encoder::new(EncoderSettings {
            width: 320,
            height: 240,
            fps: 30,
            bitrate: 2_000_000,
        })
        .expect("encoder");
        let mut dec = OpenH264Decoder::new().expect("decoder");
        let mut worst = f64::INFINITY;
        let (mut encoded, mut decoded) = (0, 0);
        for t in 0..20 {
            let img = synthetic(w, h, t);
            let mut src = I420::default();
            src.fill_from_bgra(&BgraImage {
                data: &img,
                width: w,
                height: h,
                stride: w * 4,
            })
            .expect("yuv");
            let Some(ef) = enc
                .encode(
                    &crate::win::encode_openh264::tests::frame(&img, 320, 240, t as u64 * 33_333),
                    false,
                )
                .expect("encode")
            else {
                continue; // skipped by rate control
            };
            assert_eq!(ef.keyframe, encoded == 0);
            encoded += 1;
            let pic = dec
                .decode(&ef.data)
                .expect("decode")
                .expect("picture per access unit");
            assert_eq!((pic.width, pic.height), (w, h));
            let p = psnr(&src.y, &pic.y);
            worst = worst.min(p);
            decoded += 1;
        }
        assert!(
            encoded >= 15,
            "only {encoded}/20 frames survived rate control"
        );
        assert_eq!(decoded, encoded);
        assert!(worst > 30.0, "worst luma PSNR {worst:.1} dB");
    }

    #[test]
    fn garbage_is_an_error_or_no_picture_not_a_panic() {
        let mut dec = OpenH264Decoder::new().expect("decoder");
        let r = dec.decode(&[0, 0, 0, 1, 0x65, 0xFF, 0x00, 0x13]);
        assert!(matches!(r, Ok(None) | Err(_)));
    }
}
