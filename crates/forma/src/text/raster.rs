//! CPU rasterization of text into a pixel canvas.

use super::*;

use std::cell::RefCell;

#[derive(Default)]
pub(super) struct CpuRasterScratch {
    pub(super) ids: Vec<(FontGlyph, f32)>,
    pub(super) edges: Vec<(Point, Point)>,
    pub(super) area: Vec<f32>,
    pub(super) full: Vec<f32>,
}

thread_local! {
    static CPU_SCRATCH: RefCell<CpuRasterScratch> = RefCell::new(CpuRasterScratch::default());
}

/// Straight-alpha RGBA raster destination in logical/physical canvas space.
pub struct RasterCanvas<'a> {
    pub pixels: &'a mut [u8],
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub region: [u32; 4],
}

impl<'a> RasterCanvas<'a> {
    #[inline]
    pub fn full(pixels: &'a mut [u8], width: u32, height: u32, scale: f32) -> Self {
        Self {
            pixels,
            width,
            height,
            scale,
            region: [0, 0, width, height],
        }
    }

    #[inline]
    pub fn subregion(
        pixels: &'a mut [u8],
        width: u32,
        height: u32,
        scale: f32,
        region: [u32; 4],
    ) -> Self {
        Self {
            pixels,
            width,
            height,
            scale,
            region,
        }
    }
}

/// Draw a centered, single-line label into a straight-alpha RGBA pixel buffer.
/// `rect = [x, y, width, height]` and font size are logical pixels; `canvas.scale` is
/// device pixels per logical pixel. Coverage is clipped to both rect and canvas.
pub fn draw_text(
    canvas: &mut RasterCanvas<'_>,
    text: &str,
    font_size: f32,
    rect: [f32; 4],
    color: [u8; 4],
) {
    draw_text_in(FontFamily::Sans, canvas, text, font_size, rect, color);
}

/// [`draw_text`] for a chosen family.
pub fn draw_text_in(
    family: FontFamily,
    canvas: &mut RasterCanvas<'_>,
    text: &str,
    font_size: f32,
    rect: [f32; 4],
    color: [u8; 4],
) {
    draw_text_region(
        family,
        &mut RasterCanvas::full(canvas.pixels, canvas.width, canvas.height, canvas.scale),
        text,
        font_size,
        rect,
        color,
    );
}

/// Draw into a physical subregion while retaining global contour coordinates.
pub(crate) fn draw_text_region(
    family: FontFamily,
    canvas: &mut RasterCanvas<'_>,
    text: &str,
    font_size: f32,
    rect: [f32; 4],
    color: [u8; 4],
) {
    let pixels: &mut [u8] = canvas.pixels;
    let width = canvas.width;
    let height = canvas.height;
    let scale = canvas.scale;
    let region = canvas.region;
    let Some(required) = (region[2] as usize)
        .checked_mul(region[3] as usize)
        .and_then(|size| size.checked_mul(4))
    else {
        return;
    };
    if !valid_raster_region(pixels.len(), width, height, region)
        || pixels.len() < required
        || width == 0
        || height == 0
        || color[3] == 0
        || !scale.is_finite()
        || scale <= 0.
        || !font_size.is_finite()
        || font_size <= 0.
        || !rect.iter().all(|n| n.is_finite())
        || rect[2] <= 0.
        || rect[3] <= 0.
    {
        return;
    }
    let clip = [
        (rect[0] * scale).max(0.),
        (rect[1] * scale).max(0.),
        ((rect[0] + rect[2]) * scale).min(width as f32),
        ((rect[1] + rect[3]) * scale).min(height as f32),
    ];
    if !clip.iter().all(|n| n.is_finite()) || clip[0] >= clip[2] || clip[1] >= clip[3] {
        return;
    }
    let face = glyph_face(family.primary());
    let units_to_pixels = font_size * scale / face.units_per_em() as f32;
    let (mut ids, mut edges) = CPU_SCRATCH.with(|cell| {
        let mut s = cell.borrow_mut();
        (std::mem::take(&mut s.ids), std::mem::take(&mut s.edges))
    });
    let text_width = glyph_ids(family, text, &mut ids) * units_to_pixels;
    let origin = Point {
        x: (rect[0] + rect[2] * 0.5) * scale - text_width * 0.5,
        y: (rect[1] + rect[3] * 0.5) * scale
            + (face.ascender() as f32 + face.descender() as f32) * units_to_pixels * 0.5,
    };
    if !units_to_pixels.is_finite() || !origin.x.is_finite() || !origin.y.is_finite() {
        CPU_SCRATCH.with(|cell| {
            let mut s = cell.borrow_mut();
            s.ids = ids;
            s.edges = edges;
        });
        return;
    }
    edges.clear();
    let mut contours = Contours {
        #[cfg(test)]
        tolerance: CURVE_TOLERANCE,
        edges,
        current: origin,
        start: origin,
        origin,
        units_to_pixels,
    };
    for &(glyph, adv) in &ids {
        let resolved_face = glyph_face(glyph.face);
        contours.units_to_pixels = font_size * scale / resolved_face.units_per_em() as f32;
        resolved_face.outline_glyph(glyph.id, &mut contours);
        contours.origin.x += adv * units_to_pixels;
    }
    rasterize_region(pixels, width, height, region, &contours.edges, clip, color);
    CPU_SCRATCH.with(|cell| {
        let mut s = cell.borrow_mut();
        s.ids = ids;
        s.edges = contours.edges;
    });
}

/// Exact signed area one edge (physical pixels) contributes to the pixel whose
/// left edge is `px` over the row strip [`y0`, `y1`): the integral of
/// clamp(x_edge(y) - px, 0, 1) signed by direction. Same formula as `edge_area`
/// in `vector.wgsl` (there the strip is one whole pixel), so CPU and GPU
/// coverage agree up to rounding.
pub(super) fn edge_area(a: Point, b: Point, px: f32, y0: f32, y1: f32) -> f32 {
    if a.y == b.y {
        return 0.;
    }
    let ya = a.y.min(b.y).max(y0);
    let yb = a.y.max(b.y).min(y1);
    if ya >= yb {
        return 0.;
    }
    let slope = (b.x - a.x) / (b.y - a.y);
    let xa = a.x + (ya - a.y) * slope;
    let xb = a.x + (yb - a.y) * slope;
    let area = crate::geometry::column_area(xa, xb, px);
    (if b.y > a.y { area } else { -area }) * (yb - ya)
}

/// Fill a closed contour set (physical-pixel edges) into straight-alpha RGBA with
/// exact area coverage under the nonzero rule, clipped to `clip` (physical
/// pixels, fractional edges scale the boundary column/row). Shared by text and
/// composed shapes on native and WASM.
#[cfg(test)]
pub(crate) fn rasterize(
    pixels: &mut [u8],
    width: u32,
    edges: &[(Point, Point)],
    clip: [f32; 4],
    color: [u8; 4],
) {
    if width == 0 {
        return;
    }
    let height = (pixels.len() / 4 / width as usize).min(u32::MAX as usize) as u32;
    rasterize_region(
        pixels,
        width,
        height,
        [0, 0, width, height],
        edges,
        clip,
        color,
    );
}

pub(super) fn valid_raster_region(len: usize, width: u32, height: u32, region: [u32; 4]) -> bool {
    region[2] > 0
        && region[3] > 0
        && region[0]
            .checked_add(region[2])
            .is_some_and(|end| end <= width)
        && region[1]
            .checked_add(region[3])
            .is_some_and(|end| end <= height)
        && (region[2] as usize)
            .checked_mul(region[3] as usize)
            .and_then(|n| n.checked_mul(4))
            .is_some_and(|n| n <= len)
}

/// Edges/clip stay in global physical coordinates; only storage indexing is local.
/// Keep the original horizontal accumulation prefix to preserve f32 rounding.
pub(crate) fn rasterize_region(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    region: [u32; 4],
    edges: &[(Point, Point)],
    clip: [f32; 4],
    color: [u8; 4],
) {
    if !valid_raster_region(pixels.len(), width, height, region)
        || !clip.iter().all(|n| n.is_finite())
    {
        return;
    }
    if edges.is_empty() {
        return;
    }
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for &(a, b) in edges {
        for p in [a, b] {
            if !p.x.is_finite() || !p.y.is_finite() {
                return;
            }
            bounds[0] = bounds[0].min(p.x);
            bounds[1] = bounds[1].min(p.y);
            bounds[2] = bounds[2].max(p.x);
            bounds[3] = bounds[3].max(p.y);
        }
    }
    let left = bounds[0].max(clip[0]).floor().max(0.) as usize;
    let top = (bounds[1].max(clip[1]).floor().max(0.) as usize).max(region[1] as usize);
    let right = (bounds[2].min(clip[2]).ceil().max(0.) as usize).min(width as usize);
    let bottom =
        (bounds[3].min(clip[3]).ceil().max(0.) as usize).min((region[1] + region[3]) as usize);
    if left >= right || top >= bottom {
        return;
    }
    let columns = right - left;
    // Per row: exact per-pixel areas where an edge crosses the column, plus a
    // difference array for the full-height contribution of every column to the
    // left of the edge. Scratch is one clipped row, never a full canvas.
    let (mut area, mut full) = CPU_SCRATCH.with(|cell| {
        let mut s = cell.borrow_mut();
        (std::mem::take(&mut s.area), std::mem::take(&mut s.full))
    });
    if area.len() < columns {
        area.resize(columns, 0.);
    }
    if full.len() < columns + 1 {
        full.resize(columns + 1, 0.);
    }
    for y in top..bottom {
        let row_top = (y as f32).max(clip[1]);
        let row_bottom = (y as f32 + 1.).min(clip[3]);
        if row_top >= row_bottom {
            continue;
        }
        area[..columns].fill(0.);
        full[..columns + 1].fill(0.);
        let mut touched = false;
        for &(a, b) in edges {
            let ya = a.y.min(b.y).max(row_top);
            let yb = a.y.max(b.y).min(row_bottom);
            if ya >= yb {
                continue;
            }
            touched = true;
            let slope = (b.x - a.x) / (b.y - a.y);
            let xa = a.x + (ya - a.y) * slope;
            let xb = a.x + (yb - a.y) * slope;
            let (xmin, xmax) = (xa.min(xb), xa.max(xb));
            let signed = if b.y > a.y { yb - ya } else { ya - yb };
            // Columns entirely left of the edge receive the whole row height.
            let first = ((xmin - 1.).floor().max(left as f32) as usize).min(right);
            if first > left {
                full[0] += signed;
                full[first - left] -= signed;
            }
            let last = ((xmax.floor() + 1.).min(right as f32).max(left as f32)) as usize;
            for px in first..last {
                area[px - left] += edge_area(a, b, px as f32, row_top, row_bottom);
            }
        }
        if !touched {
            continue;
        }
        let mut running = 0.;
        for offset in 0..columns {
            running += full[offset];
            let global_x = left + offset;
            if global_x < region[0] as usize || global_x >= (region[0] + region[2]) as usize {
                continue;
            }
            let px = global_x as f32;
            let horizontal = ((px + 1.).min(clip[2]) - px.max(clip[0])).clamp(0., 1.);
            let coverage = (running + area[offset]).abs().min(1.) * horizontal;
            if coverage <= 0. {
                continue;
            }
            let pixel =
                ((y - region[1] as usize) * region[2] as usize + global_x - region[0] as usize) * 4;
            crate::blend::blend(&mut pixels[pixel..pixel + 4], color, coverage);
        }
    }
    CPU_SCRATCH.with(|cell| {
        let mut s = cell.borrow_mut();
        s.area = area;
        s.full = full;
    });
}
