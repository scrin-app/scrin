//! Mapping a normalised display position to `SendInput` absolute virtual-desktop coordinates.
//!
//! With `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`, `dx`/`dy` span 0..=65535 over the whole
//! virtual desktop. Windows maps a coordinate `c` to pixel `floor(c * size / 65536)`, so the
//! inverse used here targets the pixel centre to avoid off-by-one drift at the edges.

use crate::Rect;

/// Converts a normalised position (0..=1, clamped) on `display` to absolute virtual-desktop
/// coordinates for `SendInput`, given the virtual desktop bounds.
#[must_use]
pub fn normalized_to_virtual_desk(x: f32, y: f32, display: &Rect, desktop: &Rect) -> (i32, i32) {
    (
        axis(
            x,
            display.left,
            display.width(),
            desktop.left,
            desktop.width(),
        ),
        axis(
            y,
            display.top,
            display.height(),
            desktop.top,
            desktop.height(),
        ),
    )
}

fn axis(n: f32, d_origin: i32, d_size: u32, v_origin: i32, v_size: u32) -> i32 {
    if d_size == 0 || v_size == 0 {
        return 0;
    }
    let n = if n.is_finite() {
        f64::from(n).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Pixel on the display (0..size-1), then relative to the virtual desktop origin.
    let px = (n * f64::from(d_size - 1)).round() + f64::from(d_origin) - f64::from(v_origin);
    let coord = ((px + 0.5) * 65_536.0 / f64::from(v_size))
        .floor()
        .clamp(0.0, 65_535.0);
    // Clamped to 0..=65535 just above.
    #[expect(clippy::cast_possible_truncation, reason = "clamped to 0..=65535")]
    let c = coord as i32;
    c
}

/// Inverse of [`normalized_to_virtual_desk`] as Windows performs it: absolute coordinate → pixel.
#[must_use]
pub fn virtual_desk_to_pixel(coord: i32, v_origin: i32, v_size: u32) -> i32 {
    let rel = (i64::from(coord) * i64::from(v_size)) >> 16;
    // rel < v_size <= i32::MAX.
    i32::try_from(rel).unwrap_or(i32::MAX) + v_origin
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESK: Rect = Rect {
        left: -1920,
        top: 0,
        right: 2560,
        bottom: 1440,
    };
    const RIGHT: Rect = Rect {
        left: 0,
        top: 0,
        right: 2560,
        bottom: 1440,
    };
    const LEFT: Rect = Rect {
        left: -1920,
        top: 360,
        right: 0,
        bottom: 1440,
    };

    #[test]
    fn corners_land_on_exact_pixels() {
        for (d, (nx, ny), (px, py)) in [
            (&RIGHT, (0.0, 0.0), (0, 0)),
            (&RIGHT, (1.0, 1.0), (2559, 1439)),
            (&LEFT, (0.0, 0.0), (-1920, 360)),
            (&LEFT, (1.0, 1.0), (-1, 1439)),
            (&RIGHT, (0.5, 0.5), (1280, 720)),
        ] {
            let (cx, cy) = normalized_to_virtual_desk(nx, ny, d, &DESK);
            assert_eq!(
                virtual_desk_to_pixel(cx, DESK.left, DESK.width()),
                px,
                "x for {nx}"
            );
            assert_eq!(
                virtual_desk_to_pixel(cy, DESK.top, DESK.height()),
                py,
                "y for {ny}"
            );
        }
    }

    #[test]
    fn every_pixel_round_trips() {
        let d = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        for px in 0..1920 {
            #[expect(clippy::cast_precision_loss, reason = "test")]
            let n = px as f32 / 1919.0;
            let (cx, _) = normalized_to_virtual_desk(n, 0.0, &d, &d);
            assert_eq!(virtual_desk_to_pixel(cx, 0, 1920), px);
        }
    }

    #[test]
    fn clamps_and_handles_nan() {
        let (cx, cy) = normalized_to_virtual_desk(-3.0, f32::NAN, &RIGHT, &RIGHT);
        assert_eq!(
            (cx, cy),
            normalized_to_virtual_desk(0.0, 0.0, &RIGHT, &RIGHT)
        );
        let (cx, _) = normalized_to_virtual_desk(9.0, 0.0, &RIGHT, &RIGHT);
        assert_eq!(virtual_desk_to_pixel(cx, 0, 2560), 2559);
        assert_eq!(
            normalized_to_virtual_desk(0.5, 0.5, &Rect::default(), &RIGHT),
            (0, 0)
        );
    }
}
