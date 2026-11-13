//! The pixel region a control covers and the sizes a raster may have.

use super::*;

// Conservative support of geometry plus content, or content alone. Clip
// markers only remove support; overflowing text/shapes must remain visible.
pub(super) fn raster_region(
    model: ControlView<'_>,
    key: GeometryKey,
    include_geometry: bool,
) -> [u32; 4] {
    let full = [0, 0, key.width, key.height];
    let b = &model.scene.button;
    let x = b.x - model.scroll_x;
    let y = b.y - model.scroll_y;
    let mut support: Option<[u32; 4]> = None;
    let mut invalid = false;
    let mut add = |bounds: [f32; 4]| {
        if !bounds.iter().all(|n| n.is_finite()) || bounds[2] < 0. || bounds[3] < 0. {
            invalid = true;
            return;
        }
        let physical = [
            bounds[0] * key.scale,
            bounds[1] * key.scale,
            (bounds[0] + bounds[2]) * key.scale,
            (bounds[1] + bounds[3]) * key.scale,
        ];
        if !physical.iter().all(|n| n.is_finite()) {
            invalid = true;
            return;
        }
        let rect = [
            (physical[0] - 1.).floor().clamp(0., key.width as f32) as u32,
            (physical[1] - 1.).floor().clamp(0., key.height as f32) as u32,
            (physical[2] + 1.).ceil().clamp(0., key.width as f32) as u32,
            (physical[3] + 1.).ceil().clamp(0., key.height as f32) as u32,
        ];
        if rect[0] >= rect[2] || rect[1] >= rect[3] {
            return;
        }
        support = Some(match support {
            None => rect,
            Some(old) => [
                old[0].min(rect[0]),
                old[1].min(rect[1]),
                old[2].max(rect[2]),
                old[3].max(rect[3]),
            ],
        });
    };
    if include_geometry {
        add([x, y, b.width, b.height]);
    }
    if model.template.text.is_some() {
        let r = b.radius.min(b.width / 2.).min(b.height / 2.);
        add([
            x + r * 0.3,
            y + r * 0.3,
            b.width - r * 0.6,
            b.height - r * 0.6,
        ]);
    }
    for content in &model.template.content {
        match content {
            Content::Text { bounds, .. } => {
                add([x + bounds[0], y + bounds[1], bounds[2], bounds[3]])
            }
            Content::Shape { points, .. } => {
                if points.is_empty() {
                    continue;
                }
                let mut bounds = [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ];
                for point in points {
                    if !point.iter().all(|n| n.is_finite()) {
                        return full;
                    }
                    bounds[0] = bounds[0].min(point[0]);
                    bounds[1] = bounds[1].min(point[1]);
                    bounds[2] = bounds[2].max(point[0]);
                    bounds[3] = bounds[3].max(point[1]);
                }
                add([
                    x + bounds[0],
                    y + bounds[1],
                    bounds[2] - bounds[0],
                    bounds[3] - bounds[1],
                ]);
            }
            Content::Clip { .. } | Content::ClipEnd => {}
        }
    }
    if invalid {
        return full;
    }
    support.map_or([0; 4], |[x0, y0, x1, y1]| [x0, y0, x1 - x0, y1 - y0])
}

pub(crate) fn valid_raster_size(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= 4096
        && height <= 4096
        && width as u64 * height as u64 <= 8_388_608
}

// Physical support of a rounded clip, retaining a full-pixel AA margin before
// clamping. Empty clips remain empty even when the margin would create a box.
pub(super) fn clip_pixels(bounds: [f32; 4], scale: f32, width: u32, height: u32) -> [u32; 4] {
    if bounds[2] <= 0. || bounds[3] <= 0. {
        return [0; 4];
    }
    [
        (bounds[0] * scale - 1.).floor().clamp(0., width as f32) as u32,
        (bounds[1] * scale - 1.).floor().clamp(0., height as f32) as u32,
        ((bounds[0] + bounds[2]) * scale + 1.)
            .ceil()
            .clamp(0., width as f32) as u32,
        ((bounds[1] + bounds[3]) * scale + 1.)
            .ceil()
            .clamp(0., height as f32) as u32,
    ]
}
