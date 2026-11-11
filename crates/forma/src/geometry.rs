//! Coverage and hit tests for rounded rectangles in logical and physical pixels.
//!
//! The CPU rasterizer, hit testing and the GPU shader (`vector.wgsl`) must agree on these; the
//! shader carries its own copy of `rect_area_coverage`.

use crate::markup;

pub(crate) fn inside(x: f32, y: f32, b: &markup::ButtonSpec) -> bool {
    round_inside_raw(x, y, [b.x, b.y, b.width, b.height], b.radius)
}

pub(crate) fn round_inside(x: f32, y: f32, bounds: [f32; 4], radius: f32) -> bool {
    if bounds[2] <= 0. || bounds[3] <= 0. {
        return false;
    }
    round_inside_raw(x, y, bounds, radius)
}

#[inline]
fn round_inside_raw(x: f32, y: f32, [bx, by, w, h]: [f32; 4], radius: f32) -> bool {
    let r = radius.min(w / 2.).min(h / 2.);
    let dx = (x - (bx + w / 2.)).abs() - (w / 2. - r);
    let dy = (y - (by + h / 2.)).abs() - (h / 2. - r);
    if dx <= 0. && dy <= 0. {
        return true;
    }
    let mx = dx.max(0.);
    let my = dy.max(0.);
    mx * mx + my * my <= r * r
}

/// Exact area to the right of a segment inside pixel column `px` over a unit-height
/// row slice where the segment crosses from `xa` (at slice top) to `xb` (at slice bottom).
#[inline]
pub(crate) fn column_area(xa: f32, xb: f32, px: f32) -> f32 {
    let fa = xa - px;
    let fb = xb - px;
    let d = fb - fa;
    if d == 0. {
        return fa.clamp(0., 1.);
    }
    let t0 = (-fa / d).clamp(0., 1.);
    let t1 = ((1. - fa) / d).clamp(0., 1.);
    let lo = t0.min(t1);
    let hi = t0.max(t1);
    let flo = (fa + d * lo).clamp(0., 1.);
    let fhi = (fa + d * hi).clamp(0., 1.);
    (if d > 0. { 1. - hi } else { lo }) + (flo + fhi) * 0.5 * (hi - lo)
}
pub(crate) fn rect_area_coverage(x: u32, y: u32, scale: f32, bounds: [f32; 4]) -> f32 {
    let [bx, by, w, h] = bounds;
    if w <= 0. || h <= 0. {
        return 0.;
    }
    let dx = ((x as f32 + 1.).min((bx + w) * scale) - (x as f32).max(bx * scale)).clamp(0., 1.);
    let dy = ((y as f32 + 1.).min((by + h) * scale) - (y as f32).max(by * scale)).clamp(0., 1.);
    dx * dy
}
pub(crate) fn rect_area_paint(fill: [u8; 4], border: [u8; 4], outer: f32, band: f32) -> [u8; 4] {
    let fa = fill[3] as f32 / 255.;
    let ba = border[3] as f32 / 255.;
    let stroke = ba * band.clamp(0., outer);
    let body = fa * (outer - stroke);
    let alpha = body + stroke;
    let mut color = [0u8; 4];
    if alpha > 0. {
        for k in 0..3 {
            color[k] = ((fill[k] as f32 * body + border[k] as f32 * stroke) / alpha).round() as u8;
        }
    }
    color[3] = (alpha * 255.).round() as u8;
    if color[3] == 0 {
        color.fill(0);
    }
    color
}
pub(crate) fn round_coverage(x: u32, y: u32, scale: f32, bounds: [f32; 4], radius: f32) -> f32 {
    let [bx, by, w, h] = bounds;
    if w <= 0. || h <= 0. {
        return 0.;
    }
    let r = radius.min(w / 2.).min(h / 2.);
    let dx = ((x as f32 + 0.5) / scale - bx - w / 2.).abs() - (w / 2. - r);
    let dy = ((y as f32 + 0.5) / scale - by - h / 2.).abs() - (h / 2. - r);
    let d = if dx <= 0. && dy <= 0. {
        dx.max(dy) - r
    } else {
        dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - r
    };
    if d < -0.71 / scale {
        return 1.;
    }
    if d > 0.71 / scale {
        return 0.;
    }
    let mut count = 0.;
    for sy in [0.125, 0.375, 0.625, 0.875] {
        for sx in [0.125, 0.375, 0.625, 0.875] {
            if round_inside(
                (x as f32 + sx) / scale,
                (y as f32 + sy) / scale,
                bounds,
                radius,
            ) {
                count += 1.;
            }
        }
    }
    count / 16.
}
