//! BGRA ↔ YUV 4:2:0 conversion, BT.709 limited range ("TV" range, Y 16..=235, C 16..=240).
//!
//! Fixed-point Q16 coefficients derived from Kr = 0.2126, Kb = 0.0722. Chroma is the average of
//! each 2×2 block; odd widths/heights replicate the last column/row. The loops are written over
//! fixed-size chunks so the compiler can vectorise them; a 1920×1080 frame converts in a few
//! milliseconds in release builds.

// Colour math reads best with the textbook names (y, u, v, r, g, b, cb, cr, r0, r1).
#![allow(clippy::many_single_char_names, clippy::similar_names)]

use crate::{Error, Result};

// Luma: 219/255 · (Kr, Kg, Kb) in Q16; they sum to 219/255 · 65536.
const Y_R: i32 = 11_966;
const Y_G: i32 = 40_254;
const Y_B: i32 = 4_064;
// Chroma: 224/255 · (−Kr, −Kg, 1−Kb) / (2(1−Kb)) and the Cr equivalent, Q16; each row sums to 0.
const CB_R: i32 = -6_596;
const CB_G: i32 = -22_189;
const CB_B: i32 = 28_785;
const CR_R: i32 = 28_785;
const CR_G: i32 = -26_146;
const CR_B: i32 = -2_639;
// Inverse, Q16: 255/219, and the BT.709 chroma gains scaled by 255/224.
const INV_Y: i32 = 76_309;
const INV_CR_R: i32 = 117_489;
const INV_CB_G: i32 = -13_975;
const INV_CR_G: i32 = -34_925;
const INV_CB_B: i32 = 138_438;

const ROUND: i32 = 1 << 15;

/// Destination chroma layout.
#[derive(Debug)]
pub enum Chroma<'a> {
    /// I420: separate U and V planes, `chroma_stride` bytes per row each.
    Planar {
        /// Cb plane.
        u: &'a mut [u8],
        /// Cr plane.
        v: &'a mut [u8],
    },
    /// NV12: one interleaved UV plane, `chroma_stride` bytes per row (≥ 2·⌈w/2⌉).
    Interleaved {
        /// Interleaved Cb/Cr plane.
        uv: &'a mut [u8],
    },
}

/// Source BGRA image description.
#[derive(Debug, Clone, Copy)]
pub struct BgraImage<'a> {
    /// Pixels, `stride` bytes per row, `B, G, R, A` order.
    pub data: &'a [u8],
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Bytes per row (≥ 4·width).
    pub stride: usize,
}

impl BgraImage<'_> {
    fn check(&self) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Err(Error::InvalidInput("empty image".into()));
        }
        if self.stride < self.width * 4
            || self.data.len() < self.stride * (self.height - 1) + self.width * 4
        {
            return Err(Error::InvalidInput(format!(
                "BGRA buffer {} bytes too small for {}x{} stride {}",
                self.data.len(),
                self.width,
                self.height,
                self.stride
            )));
        }
        Ok(())
    }
}

/// Size of a chroma plane dimension for a luma dimension.
#[must_use]
pub const fn chroma_len(luma: usize) -> usize {
    luma.div_ceil(2)
}

#[inline]
fn clamp_u8(v: i32) -> u8 {
    #[expect(clippy::cast_sign_loss, reason = "clamped to 0..=255")]
    let b = v.clamp(0, 255) as u8;
    b
}

#[inline]
fn luma(b: i32, g: i32, r: i32) -> u8 {
    clamp_u8(((Y_R * r + Y_G * g + Y_B * b + ROUND) >> 16) + 16)
}

/// Cb/Cr from the sums of four B, G, R samples.
#[inline]
fn chroma4(b: i32, g: i32, r: i32) -> (u8, u8) {
    (
        clamp_u8(((CB_R * r + CB_G * g + CB_B * b + (ROUND << 2)) >> 18) + 128),
        clamp_u8(((CR_R * r + CR_G * g + CR_B * b + (ROUND << 2)) >> 18) + 128),
    )
}

/// One 2×2 block given as two BGRA pixels from each of two rows.
#[inline]
fn quad(a: [u8; 8], b: [u8; 8]) -> ([u8; 2], [u8; 2], u8, u8) {
    let c = |s: [u8; 8], i: usize| i32::from(s[i]);
    let ya = [
        luma(c(a, 0), c(a, 1), c(a, 2)),
        luma(c(a, 4), c(a, 5), c(a, 6)),
    ];
    let yb = [
        luma(c(b, 0), c(b, 1), c(b, 2)),
        luma(c(b, 4), c(b, 5), c(b, 6)),
    ];
    let (cb, cr) = chroma4(
        c(a, 0) + c(a, 4) + c(b, 0) + c(b, 4),
        c(a, 1) + c(a, 5) + c(b, 1) + c(b, 5),
        c(a, 2) + c(a, 6) + c(b, 2) + c(b, 6),
    );
    (ya, yb, cb, cr)
}

/// Converts one pair of source rows (each `4·w` bytes) into two luma rows (`w` bytes each) and
/// one chroma row. For an odd last row pass the same row twice and a scratch `y1`.
fn row_pair<'c>(
    r0: &[u8],
    r1: &[u8],
    y0: &mut [u8],
    y1: &mut [u8],
    mut chroma: impl Iterator<Item = (&'c mut u8, &'c mut u8)>,
) {
    let (s0, t0) = r0.as_chunks::<8>();
    let (s1, t1) = r1.as_chunks::<8>();
    let (d0, e0) = y0.as_chunks_mut::<2>();
    let (d1, e1) = y1.as_chunks_mut::<2>();
    for ((((a, b), la), lb), (cu, cv)) in s0
        .iter()
        .zip(s1)
        .zip(d0.iter_mut())
        .zip(d1.iter_mut())
        .zip(chroma.by_ref())
    {
        let (ya, yb, cb, cr) = quad(*a, *b);
        *la = ya;
        *lb = yb;
        *cu = cb;
        *cv = cr;
    }
    // Odd width: the last column forms a 2×2 block with itself.
    if let ([p0, ..], [p1, ..], [l0], [l1]) = (t0.as_chunks::<4>().0, t1.as_chunks::<4>().0, e0, e1)
    {
        let a = [p0[0], p0[1], p0[2], p0[3], p0[0], p0[1], p0[2], p0[3]];
        let b = [p1[0], p1[1], p1[2], p1[3], p1[0], p1[1], p1[2], p1[3]];
        let (ya, yb, cb, cr) = quad(a, b);
        *l0 = ya[0];
        *l1 = yb[0];
        if let Some((cu, cv)) = chroma.next() {
            *cu = cb;
            *cv = cr;
        }
    }
}

/// Converts BGRA to 4:2:0 YUV with BT.709 limited range.
///
/// `y` holds `y_stride · height` bytes; the chroma plane(s) hold `chroma_stride · ⌈height/2⌉`.
pub fn bgra_to_yuv420(
    src: &BgraImage<'_>,
    y: &mut [u8],
    y_stride: usize,
    chroma: Chroma<'_>,
    chroma_stride: usize,
) -> Result<()> {
    src.check()?;
    let (w, h) = (src.width, src.height);
    let (cw, ch) = (chroma_len(w), chroma_len(h));
    if y_stride < w || y.len() < y_stride * (h - 1) + w {
        return Err(Error::InvalidInput("luma plane too small".into()));
    }
    let chroma_row = match &chroma {
        Chroma::Planar { .. } => cw,
        Chroma::Interleaved { .. } => cw * 2,
    };
    let chroma_needed = chroma_stride * (ch - 1) + chroma_row;
    let chroma_ok = chroma_stride >= chroma_row
        && match &chroma {
            Chroma::Planar { u, v } => u.len() >= chroma_needed && v.len() >= chroma_needed,
            Chroma::Interleaved { uv } => uv.len() >= chroma_needed,
        };
    if !chroma_ok {
        return Err(Error::InvalidInput("chroma plane too small".into()));
    }

    let mut scratch = Vec::new();
    let mut chroma = chroma;
    for cy in 0..ch {
        let ra = cy * 2;
        let rb = (ra + 1).min(h - 1);
        let row0 = &src.data[ra * src.stride..ra * src.stride + w * 4];
        let row1 = &src.data[rb * src.stride..rb * src.stride + w * 4];
        let (top, bottom) = y.split_at_mut(ra * y_stride + y_stride.min(y.len() - ra * y_stride));
        let y0 = &mut top[ra * y_stride..ra * y_stride + w];
        let y1: &mut [u8] = if rb == ra {
            scratch.resize(w, 0);
            &mut scratch
        } else {
            &mut bottom[..w]
        };
        let c = cy * chroma_stride;
        match &mut chroma {
            Chroma::Planar { u, v } => {
                let it = u[c..c + cw].iter_mut().zip(v[c..c + cw].iter_mut());
                row_pair(row0, row1, y0, y1, it);
            }
            Chroma::Interleaved { uv } => {
                let it = uv[c..c + cw * 2]
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .map(|[a, b]| (a, b));
                row_pair(row0, row1, y0, y1, it);
            }
        }
    }
    Ok(())
}

/// Tightly packed I420 frame buffer, reused across frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct I420 {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Luma, `width · height`.
    pub y: Vec<u8>,
    /// Cb, `⌈w/2⌉ · ⌈h/2⌉`.
    pub u: Vec<u8>,
    /// Cr, `⌈w/2⌉ · ⌈h/2⌉`.
    pub v: Vec<u8>,
}

impl I420 {
    /// Resizes the planes (no reallocation when the size is unchanged).
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        let c = chroma_len(width) * chroma_len(height);
        self.y.resize(width * height, 0);
        self.u.resize(c, 0);
        self.v.resize(c, 0);
    }

    /// Fills this buffer from a BGRA image.
    pub fn fill_from_bgra(&mut self, src: &BgraImage<'_>) -> Result<()> {
        self.resize(src.width, src.height);
        let cw = chroma_len(src.width);
        bgra_to_yuv420(
            src,
            &mut self.y,
            src.width,
            Chroma::Planar {
                u: &mut self.u,
                v: &mut self.v,
            },
            cw,
        )
    }
}

/// Tightly packed NV12 frame buffer (Y plane followed by interleaved UV), reused across frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nv12 {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// `width · height` luma bytes then `2⌈w/2⌉ · ⌈h/2⌉` chroma bytes.
    pub data: Vec<u8>,
}

impl Nv12 {
    /// Bytes per row of both planes (luma row; the UV row has the same length for even widths).
    #[must_use]
    pub fn stride(&self) -> usize {
        chroma_len(self.width) * 2
    }

    /// Fills this buffer from a BGRA image. Both planes use [`Nv12::stride`] bytes per row.
    pub fn fill_from_bgra(&mut self, src: &BgraImage<'_>) -> Result<()> {
        self.width = src.width;
        self.height = src.height;
        let stride = self.stride();
        let luma_len = stride * src.height;
        self.data
            .resize(luma_len + stride * chroma_len(src.height), 0);
        let (y, uv) = self.data.split_at_mut(luma_len);
        bgra_to_yuv420(src, y, stride, Chroma::Interleaved { uv }, stride)
    }
}

/// Converts I420 planes (tight strides) back to BGRA, BT.709 limited range. Used for decoded
/// pictures and tests.
pub fn i420_to_bgra(frame: &I420, out: &mut Vec<u8>) -> Result<()> {
    let (w, h) = (frame.width, frame.height);
    let cw = chroma_len(w);
    if frame.y.len() < w * h
        || frame.u.len() < cw * chroma_len(h)
        || frame.v.len() < cw * chroma_len(h)
    {
        return Err(Error::InvalidInput("I420 planes too small".into()));
    }
    out.resize(w * h * 4, 0);
    for row in 0..h {
        let crow = row / 2;
        for col in 0..w {
            let c = (crow * cw) + col / 2;
            let yy = (i32::from(frame.y[row * w + col]) - 16) * INV_Y;
            let cb = i32::from(frame.u[c]) - 128;
            let cr = i32::from(frame.v[c]) - 128;
            let o = (row * w + col) * 4;
            out[o] = clamp_u8((yy + INV_CB_B * cb + ROUND) >> 16);
            out[o + 1] = clamp_u8((yy + INV_CB_G * cb + INV_CR_G * cr + ROUND) >> 16);
            out[o + 2] = clamp_u8((yy + INV_CR_R * cr + ROUND) >> 16);
            out[o + 3] = 255;
        }
    }
    Ok(())
}

/// Peak signal-to-noise ratio in dB between two equally sized 8-bit buffers (∞ when identical).
#[must_use]
pub fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let sse: u64 = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| u64::from(x.abs_diff(y)).pow(2))
        .sum();
    if sse == 0 {
        return f64::INFINITY;
    }
    // Precision loss is irrelevant at the magnitudes of an image-quality metric.
    #[expect(clippy::cast_precision_loss, reason = "metric; values far below 2^52")]
    let mse = sse as f64 / n as f64;
    10.0 * (255.0 * 255.0 / mse).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, bgr: [u8; 3]) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 4);
        for _ in 0..w * h {
            v.extend_from_slice(&[bgr[0], bgr[1], bgr[2], 255]);
        }
        v
    }

    fn to_i420(data: &[u8], w: usize, h: usize) -> I420 {
        let mut f = I420::default();
        f.fill_from_bgra(&BgraImage {
            data,
            width: w,
            height: h,
            stride: w * 4,
        })
        .expect("convert");
        f
    }

    #[test]
    fn reference_colours_bt709_limited() {
        // (B, G, R) -> (Y, Cb, Cr), reference values from ITU-R BT.709 limited range.
        let cases: [([u8; 3], [u8; 3]); 6] = [
            ([0, 0, 0], [16, 128, 128]),
            ([255, 255, 255], [235, 128, 128]),
            ([0, 0, 255], [63, 102, 240]),
            ([0, 255, 0], [173, 42, 26]),
            ([255, 0, 0], [32, 240, 118]),
            ([128, 128, 128], [126, 128, 128]),
        ];
        for (bgr, yuv) in cases {
            let f = to_i420(&solid(2, 2, bgr), 2, 2);
            let got = [f.y[0], f.u[0], f.v[0]];
            for (g, e) in got.iter().zip(yuv) {
                assert!(g.abs_diff(e) <= 1, "{bgr:?}: got {got:?}, want {yuv:?}");
            }
        }
    }

    #[test]
    fn chroma_is_2x2_average() {
        // Left column red, right column blue: the single chroma sample is the average colour.
        let mut img = Vec::new();
        for _ in 0..2 {
            img.extend_from_slice(&[0, 0, 255, 255, 255, 0, 0, 255]);
        }
        let f = to_i420(&img, 2, 2);
        let avg = to_i420(&solid(2, 2, [128, 0, 128]), 2, 2);
        assert!(f.u[0].abs_diff(avg.u[0]) <= 1 && f.v[0].abs_diff(avg.v[0]) <= 1);
    }

    #[test]
    fn odd_sizes_and_padding_stride() {
        let (w, h, stride) = (5, 3, 32);
        let mut img = vec![0u8; stride * h];
        for row in 0..h {
            for col in 0..w {
                img[row * stride + col * 4..row * stride + col * 4 + 4]
                    .copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let mut f = I420::default();
        f.fill_from_bgra(&BgraImage {
            data: &img,
            width: w,
            height: h,
            stride,
        })
        .expect("convert");
        assert_eq!((f.y.len(), f.u.len()), (15, 6));
        assert!(f.y.iter().all(|&v| v == 235));
        assert!(f.u.iter().chain(&f.v).all(|&v| v == 128));
    }

    #[test]
    fn nv12_matches_i420() {
        let (w, h) = (6, 4);
        let img: Vec<u8> = (0..w * h * 4)
            .map(|i| u8::try_from((i * 37) % 251).expect("byte"))
            .collect();
        let i420 = to_i420(&img, w, h);
        let mut nv = Nv12::default();
        nv.fill_from_bgra(&BgraImage {
            data: &img,
            width: w,
            height: h,
            stride: w * 4,
        })
        .expect("nv12");
        let stride = nv.stride();
        assert_eq!(stride, 6);
        assert_eq!(&nv.data[..w * h], &i420.y[..]);
        let uv = &nv.data[w * h..];
        for i in 0..i420.u.len() {
            assert_eq!(uv[i * 2], i420.u[i]);
            assert_eq!(uv[i * 2 + 1], i420.v[i]);
        }
    }

    #[test]
    fn round_trip_psnr_on_smooth_image() {
        let (w, h) = (64, 48);
        let mut img = Vec::with_capacity(w * h * 4);
        for row in 0..h {
            for col in 0..w {
                img.extend_from_slice(&[
                    u8::try_from(col * 4).expect("b"),
                    u8::try_from(row * 5).expect("g"),
                    u8::try_from((col + row) * 2).expect("r"),
                    255,
                ]);
            }
        }
        let f = to_i420(&img, w, h);
        let mut back = Vec::new();
        i420_to_bgra(&f, &mut back).expect("back");
        let p = psnr(&img, &back);
        assert!(p > 38.0, "round-trip PSNR {p:.1} dB");
    }

    #[test]
    fn rejects_short_buffers() {
        let img = vec![0u8; 10];
        let mut f = I420::default();
        assert!(
            f.fill_from_bgra(&BgraImage {
                data: &img,
                width: 4,
                height: 4,
                stride: 16
            })
            .is_err()
        );
        let mut y = vec![0u8; 1];
        let ok_img = solid(2, 2, [0, 0, 0]);
        let (mut u, mut v) = (vec![0u8; 1], vec![0u8; 1]);
        let src = BgraImage {
            data: &ok_img,
            width: 2,
            height: 2,
            stride: 8,
        };
        assert!(
            bgra_to_yuv420(
                &src,
                &mut y,
                2,
                Chroma::Planar {
                    u: &mut u,
                    v: &mut v
                },
                1
            )
            .is_err()
        );
    }

    #[test]
    fn psnr_basics() {
        assert!(psnr(&[1, 2, 3], &[1, 2, 3]).is_infinite());
        let p = psnr(&[0; 100], &[1; 100]);
        assert!((p - 48.13).abs() < 0.01, "{p}");
    }
}
