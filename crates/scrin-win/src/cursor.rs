//! DXGI pointer shapes → straight RGBA.
//!
//! Desktop Duplication reports three shape types:
//! - **Colour**: BGRA with alpha.
//! - **Monochrome**: 1 bpp AND mask followed by 1 bpp XOR mask (height = 2× the cursor height).
//! - **Masked colour**: BGRA where alpha 0xFF means "XOR with the screen" and 0 means "replace".
//!
//! XOR-with-screen pixels cannot be reproduced without the screen; like other remote-desktop
//! clients they become black (or white for the inverted case) with full opacity so the cursor
//! stays visible on any background.

// Pixel loops read best with the conventional x/y/w/r/g/b names.
#![allow(clippy::many_single_char_names)]

use crate::{CursorShape, Error, Result};

/// DXGI shape type ids (`DXGI_OUTDUPL_POINTER_SHAPE_TYPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeType {
    /// `DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MONOCHROME` (1).
    Monochrome,
    /// `DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR` (2).
    Color,
    /// `DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MASKED_COLOR` (4).
    MaskedColor,
}

impl ShapeType {
    /// From the raw DXGI value.
    #[must_use]
    pub fn from_raw(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Monochrome),
            2 => Some(Self::Color),
            4 => Some(Self::MaskedColor),
            _ => None,
        }
    }
}

/// Converts a raw shape buffer into RGBA. `height` is the *reported* height (for monochrome it
/// covers both masks).
pub fn shape_to_rgba(
    kind: ShapeType,
    width: u32,
    height: u32,
    pitch: u32,
    hotspot: (i32, i32),
    buf: &[u8],
) -> Result<CursorShape> {
    let (w, pitch) = (width as usize, pitch as usize);
    let out_h = if kind == ShapeType::Monochrome {
        height / 2
    } else {
        height
    } as usize;
    let needed = pitch * height as usize;
    if w == 0 || out_h == 0 || buf.len() < needed {
        return Err(Error::InvalidInput(format!(
            "cursor shape {width}x{height} pitch {pitch}, {} bytes",
            buf.len()
        )));
    }
    let mut rgba = vec![0u8; w * out_h * 4];
    for y in 0..out_h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            let px = match kind {
                ShapeType::Color => {
                    let s = y * pitch + x * 4;
                    [buf[s + 2], buf[s + 1], buf[s], buf[s + 3]]
                }
                ShapeType::MaskedColor => {
                    let s = y * pitch + x * 4;
                    let (b, g, r, mask) = (buf[s], buf[s + 1], buf[s + 2], buf[s + 3]);
                    if mask == 0 {
                        [r, g, b, 255]
                    } else if (r, g, b) == (0, 0, 0) {
                        // XOR with black = screen unchanged → transparent.
                        [0, 0, 0, 0]
                    } else {
                        // XOR inverts the screen; approximate with an opaque black pixel.
                        [0, 0, 0, 255]
                    }
                }
                ShapeType::Monochrome => {
                    let bit = 0x80u8 >> (x % 8);
                    let and = buf[y * pitch + x / 8] & bit != 0;
                    let xor = buf[(y + out_h) * pitch + x / 8] & bit != 0;
                    match (and, xor) {
                        // Black, or invert-the-screen which we approximate with black.
                        (false, false) | (true, true) => [0, 0, 0, 255],
                        (false, true) => [255, 255, 255, 255],
                        (true, false) => [0, 0, 0, 0],
                    }
                }
            };
            rgba[o..o + 4].copy_from_slice(&px);
        }
    }
    Ok(CursorShape {
        width,
        height: u32::try_from(out_h).unwrap_or(u32::MAX),
        hotspot,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_swaps_bgr() {
        let buf = [10, 20, 30, 40, 1, 2, 3, 255];
        let s = shape_to_rgba(ShapeType::Color, 2, 1, 8, (1, 0), &buf).expect("shape");
        assert_eq!(s.rgba, vec![30, 20, 10, 40, 3, 2, 1, 255]);
        assert_eq!((s.width, s.height, s.hotspot), (2, 1, (1, 0)));
    }

    #[test]
    fn monochrome_truth_table() {
        // 4x1 cursor, pitch 1 byte; AND row then XOR row.
        let and = 0b0011_0000u8; // pixels 2,3 AND=1
        let xor = 0b0101_0000u8; // pixels 1,3 XOR=1
        let s = shape_to_rgba(ShapeType::Monochrome, 4, 2, 1, (0, 0), &[and, xor]).expect("shape");
        assert_eq!(s.height, 1);
        let px: Vec<&[u8]> = s.rgba.chunks(4).collect();
        assert_eq!(px[0], [0, 0, 0, 255]);
        assert_eq!(px[1], [255, 255, 255, 255]);
        assert_eq!(px[2], [0, 0, 0, 0]);
        assert_eq!(px[3], [0, 0, 0, 255]);
    }

    #[test]
    fn masked_color_modes() {
        let buf = [5, 6, 7, 0, 0, 0, 0, 255, 9, 9, 9, 255];
        let s = shape_to_rgba(ShapeType::MaskedColor, 3, 1, 12, (0, 0), &buf).expect("shape");
        assert_eq!(&s.rgba[0..4], &[7, 6, 5, 255]);
        assert_eq!(&s.rgba[4..8], &[0, 0, 0, 0]);
        assert_eq!(&s.rgba[8..12], &[0, 0, 0, 255]);
    }

    #[test]
    fn rejects_short_and_unknown() {
        assert!(shape_to_rgba(ShapeType::Color, 4, 4, 16, (0, 0), &[0; 10]).is_err());
        assert_eq!(ShapeType::from_raw(3), None);
        assert_eq!(ShapeType::from_raw(4), Some(ShapeType::MaskedColor));
    }
}
