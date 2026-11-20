//! Backend-independent retained display items, transform/clip nodes, diagnostics, and CPU reference rasterizer.

use crate::{blend, geometry, resource_cache::ResourceId};

use super::{DisplayList, SCROLL_CONTENT, STRIDE, scrolled_bounds_with_areas};

/// 2D axis-aligned affine transform (`scale` then `translate`) in logical scene coordinates.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transform2D {
    pub scale: [f32; 2],
    pub translate: [f32; 2],
}

impl Default for Transform2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform2D {
    pub const IDENTITY: Self = Self {
        scale: [1., 1.],
        translate: [0., 0.],
    };

    #[inline]
    pub const fn translate(dx: f32, dy: f32) -> Self {
        Self {
            scale: [1., 1.],
            translate: [dx, dy],
        }
    }

    #[inline]
    pub const fn scale_translate(sx: f32, sy: f32, tx: f32, ty: f32) -> Self {
        Self {
            scale: [sx, sy],
            translate: [tx, ty],
        }
    }

    #[inline]
    pub fn is_identity(self) -> bool {
        self.scale == [1., 1.] && self.translate == [0., 0.]
    }

    #[inline]
    pub fn apply_point(self, p: [f32; 2]) -> [f32; 2] {
        [
            p[0] * self.scale[0] + self.translate[0],
            p[1] * self.scale[1] + self.translate[1],
        ]
    }

    #[inline]
    pub fn apply_bounds(self, b: [f32; 4]) -> [f32; 4] {
        if self.is_identity() {
            return b;
        }
        let x0 = b[0] * self.scale[0] + self.translate[0];
        let y0 = b[1] * self.scale[1] + self.translate[1];
        let w = b[2] * self.scale[0];
        let h = b[3] * self.scale[1];
        [x0.min(x0 + w), y0.min(y0 + h), w.abs(), h.abs()]
    }
}

/// Index into [`DisplayList::clips`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct ClipId(pub u32);

/// Node in a hierarchical clip tree.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipNode {
    pub id: ClipId,
    pub parent: Option<ClipId>,
    pub bounds: [f32; 4],
    pub radius: f32,
    pub crisp: bool,
    pub scroll_mode: f32,
    pub scroll_extra: [f32; 2],
    pub area_slot: Option<u32>,
}

/// Kind of backend-independent display primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub enum DisplayItemKind {
    SolidRect,
    StyledBox,
    RevealBorder,
    VectorPath,
    ScrollbarThumb,
}

/// Single backend-independent vector/shape/text/chrome draw command.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayItem {
    pub kind: DisplayItemKind,
    pub bounds: [f32; 4],
    pub transform: Transform2D,
    pub clip_id: Option<ClipId>,
    pub resource_id: Option<ResourceId>,
    pub color: [u8; 4],
    pub radius: f32,
    pub stroke: f32,
    pub edge_range: [u32; 2],
    pub nonzero: bool,
    pub paint_slots: [u32; 2],
    pub scroll_mode: f32,
    pub scroll_factor: f32,
    pub scroll_extra: [f32; 2],
    pub area_slot: Option<u32>,
    pub control_index: Option<u32>,
}

/// Reference rasterization configuration for [`DisplayList::rasterize_rgba`].
#[derive(Clone, Copy, Debug)]
pub struct RasterizeOpts<'a> {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub scroll: [f32; 2],
    pub paints: &'a [f32],
    pub area_scrolls: &'a [[f32; 4]],
}

impl<'a> RasterizeOpts<'a> {
    pub const fn new(width: u32, height: u32, scale: f32) -> Self {
        Self {
            width,
            height,
            scale,
            scroll: [0., 0.],
            paints: &[],
            area_scrolls: &[],
        }
    }
}

struct ActiveLayer {
    bounds: [f32; 4],
    radius: f32,
    crisp: bool,
    region: [u32; 4],
    pixels: Vec<u8>,
}

struct TargetSurface<'a> {
    pixels: &'a mut [u8],
    region: [u32; 4],
}

fn slot_color(paints: &[f32], slot: u32, fallback: [u8; 4]) -> [u8; 4] {
    let base = slot as usize * 4;
    let Some(slice) = paints.get(base..base + 4) else {
        return fallback;
    };
    [slice[0], slice[1], slice[2], slice[3]].map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
}

fn paint_slot_vec4(paints: &[f32], slot: u32) -> [f32; 4] {
    let base = slot as usize * 4;
    paints
        .get(base..base + 4)
        .map_or([0.; 4], |s| [s[0], s[1], s[2], s[3]])
}

fn clip_pixel_bounds(bounds: [f32; 4], scale: f32, width: u32, height: u32) -> [u32; 4] {
    if bounds[2] <= 0. || bounds[3] <= 0. {
        return [0; 4];
    }
    let x0 = (bounds[0] * scale - 1.).floor().clamp(0., width as f32) as u32;
    let y0 = (bounds[1] * scale - 1.).floor().clamp(0., height as f32) as u32;
    let x1 = ((bounds[0] + bounds[2]) * scale + 1.)
        .ceil()
        .clamp(0., width as f32) as u32;
    let y1 = ((bounds[1] + bounds[3]) * scale + 1.)
        .ceil()
        .clamp(0., height as f32) as u32;
    if x0 >= x1 || y0 >= y1 {
        [0; 4]
    } else {
        [x0, y0, x1 - x0, y1 - y0]
    }
}

fn intersect_region(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    if x0 >= x1 || y0 >= y1 {
        [0; 4]
    } else {
        [x0, y0, x1 - x0, y1 - y0]
    }
}

fn scroll_shift(
    scroll_mode: f32,
    scroll_extra: [f32; 2],
    area_slot: Option<u32>,
    opts: &RasterizeOpts<'_>,
) -> [f32; 2] {
    if scroll_mode == super::SCROLL_AREA_CONTENT {
        let root_mul = scroll_extra[0];
        if let Some(slot) = area_slot {
            let base = slot as usize * 4;
            if let Some(slice) = opts.paints.get(base..base + 2) {
                return [
                    opts.scroll[0] * root_mul + slice[0],
                    opts.scroll[1] * root_mul + slice[1],
                ];
            }
            if let Some(area) = opts.area_scrolls.get(slot as usize) {
                return [
                    opts.scroll[0] * root_mul + area[0],
                    opts.scroll[1] * root_mul + area[1],
                ];
            }
        }
        return [opts.scroll[0] * root_mul, opts.scroll[1] * root_mul];
    }
    if scroll_mode == SCROLL_CONTENT {
        [
            opts.scroll[0] + scroll_extra[0],
            opts.scroll[1] + scroll_extra[1],
        ]
    } else {
        [0., 0.]
    }
}

fn scrolled_item_bounds(
    bounds: [f32; 4],
    scroll_mode: f32,
    scroll_factor: f32,
    scroll_extra: [f32; 2],
    area_slot: Option<u32>,
    opts: &RasterizeOpts<'_>,
) -> [f32; 4] {
    let mut mock = [0f32; STRIDE];
    mock[4..8].copy_from_slice(&bounds);
    mock[super::SCROLL_MODE] = scroll_mode;
    mock[super::SCROLL_FACTOR] = scroll_factor;
    mock[super::SCROLL_EXTRA..super::SCROLL_EXTRA + 2].copy_from_slice(&scroll_extra);
    if !opts.paints.is_empty() {
        return scrolled_bounds_with_areas(&mock, opts.scroll, opts.paints);
    }
    if scroll_mode == super::SCROLL_AREA_CONTENT {
        let root_mul = scroll_extra[0];
        if let Some(slot) = area_slot {
            if let Some(area) = opts.area_scrolls.get(slot as usize) {
                return [
                    bounds[0] - opts.scroll[0] * root_mul - area[0],
                    bounds[1] - opts.scroll[1] * root_mul - area[1],
                    bounds[2],
                    bounds[3],
                ];
            }
        }
    }
    scrolled_bounds_with_areas(&mock, opts.scroll, &[])
}

fn common_clip_prefix(clips: &[ClipNode], a: Option<ClipId>, b: Option<ClipId>) -> usize {
    let mut path_a = [0u32; 32];
    let mut path_b = [0u32; 32];
    let len_a = collect_clip_chain(clips, a, &mut path_a);
    let len_b = collect_clip_chain(clips, b, &mut path_b);
    path_a[..len_a]
        .iter()
        .zip(&path_b[..len_b])
        .take_while(|(x, y)| x == y)
        .count()
}

fn collect_clip_chain(clips: &[ClipNode], leaf: Option<ClipId>, out: &mut [u32; 32]) -> usize {
    let mut rev = [0u32; 32];
    let mut n = 0usize;
    let mut cur = leaf;
    while let Some(id) = cur {
        if n >= rev.len() {
            break;
        }
        let Some(node) = clips.get(id.0 as usize) else {
            break;
        };
        rev[n] = id.0;
        n += 1;
        cur = node.parent;
    }
    for i in 0..n {
        out[i] = rev[n - 1 - i];
    }
    n
}

fn blend_pixel_at(surface: &mut TargetSurface<'_>, x: u32, y: u32, color: [u8; 4], cov: f32) {
    if cov <= 0. || color[3] == 0 {
        return;
    }
    let [rx, ry, rw, rh] = surface.region;
    if x < rx || y < ry || x >= rx + rw || y >= ry + rh {
        return;
    }
    let idx = ((y - ry) * rw + (x - rx)) as usize * 4;
    if cov >= 1. - 1e-6 && color[3] == 255 {
        surface.pixels[idx..idx + 4].copy_from_slice(&color);
    } else {
        blend::blend(&mut surface.pixels[idx..idx + 4], color, cov.clamp(0., 1.));
    }
}

fn rasterize_item(
    item: &DisplayItem,
    edges_buf: &[f32],
    opts: &RasterizeOpts<'_>,
    surface: &mut TargetSurface<'_>,
) {
    if surface.region[2] == 0 || surface.region[3] == 0 {
        return;
    }
    let b = scrolled_item_bounds(
        item.transform.apply_bounds(item.bounds),
        item.scroll_mode,
        item.scroll_factor,
        item.scroll_extra,
        item.area_slot,
        opts,
    );
    let [rx, ry, rw, rh] = surface.region;
    let x0 = (b[0] * opts.scale - 1.).floor().max(rx as f32) as u32;
    let y0 = (b[1] * opts.scale - 1.).floor().max(ry as f32) as u32;
    let x1 = ((b[0] + b[2]) * opts.scale + 1.)
        .ceil()
        .min((rx + rw) as f32) as u32;
    let y1 = ((b[1] + b[3]) * opts.scale + 1.)
        .ceil()
        .min((ry + rh) as f32) as u32;
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    match item.kind {
        DisplayItemKind::SolidRect | DisplayItemKind::ScrollbarThumb => {
            if item.color[3] == 0 {
                return;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    let cov = if item.kind == DisplayItemKind::ScrollbarThumb {
                        let px = (x as f32 + 0.5) / opts.scale;
                        let py = (y as f32 + 0.5) / opts.scale;
                        if px >= b[0] && py >= b[1] && px < b[0] + b[2] && py < b[1] + b[3] {
                            1.
                        } else {
                            0.
                        }
                    } else if item.radius <= 0. {
                        geometry::rect_area_coverage(x, y, opts.scale, b)
                    } else {
                        let d = geometry::rounded_rect_sdf(
                            (x as f32 + 0.5) / opts.scale,
                            (y as f32 + 0.5) / opts.scale,
                            b,
                            item.radius,
                        );
                        (0.5 - d * opts.scale).clamp(0., 1.)
                    };
                    blend_pixel_at(surface, x, y, item.color, cov);
                }
            }
        }
        DisplayItemKind::StyledBox => {
            let fill = slot_color(opts.paints, item.paint_slots[0], item.color);
            let border = slot_color(opts.paints, item.paint_slots[1], [0; 4]);
            if fill[3] == 0 && (item.stroke <= 0. || border[3] == 0) {
                return;
            }
            let r = item.radius.min(b[2] * 0.5).min(b[3] * 0.5);
            for y in y0..y1 {
                for x in x0..x1 {
                    if r <= 0. {
                        let outer = geometry::rect_area_coverage(x, y, opts.scale, b);
                        if outer <= 0. {
                            continue;
                        }
                        let inner = if item.stroke > 0. {
                            geometry::rect_area_coverage(
                                x,
                                y,
                                opts.scale,
                                [
                                    b[0] + item.stroke,
                                    b[1] + item.stroke,
                                    b[2] - 2. * item.stroke,
                                    b[3] - 2. * item.stroke,
                                ],
                            )
                        } else {
                            outer
                        };
                        let band = (outer - inner).max(0.);
                        let color = geometry::rect_area_paint(fill, border, outer, band);
                        blend_pixel_at(surface, x, y, color, 1.);
                    } else {
                        let d = geometry::rounded_rect_sdf(
                            (x as f32 + 0.5) / opts.scale,
                            (y as f32 + 0.5) / opts.scale,
                            b,
                            r,
                        );
                        let outer = (0.5 - d * opts.scale).clamp(0., 1.);
                        if outer <= 0. {
                            continue;
                        }
                        if item.stroke > 0. && border[3] > 0 {
                            let inner = (0.5 - (d + item.stroke) * opts.scale).clamp(0., 1.);
                            let band = (outer - inner).max(0.);
                            blend_pixel_at(surface, x, y, fill, outer);
                            blend_pixel_at(surface, x, y, border, band);
                        } else {
                            blend_pixel_at(surface, x, y, fill, outer);
                        }
                    }
                }
            }
        }
        DisplayItemKind::RevealBorder => {
            let data = paint_slot_vec4(opts.paints, item.paint_slots[0]);
            let color_f = paint_slot_vec4(opts.paints, item.paint_slots[0] + 1);
            let color = if opts.paints.len() >= (item.paint_slots[0] as usize + 2) * 4 {
                color_f.map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
            } else {
                item.color
            };
            if color[3] == 0 || item.stroke <= 0. {
                return;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = [(x as f32 + 0.5) / opts.scale, (y as f32 + 0.5) / opts.scale];
                    let band =
                        crate::reveal::band_coverage(p, b, item.radius, item.stroke, opts.scale);
                    if band <= 0. {
                        continue;
                    }
                    let radial = if data[3] == 1. {
                        1.
                    } else {
                        let dist = ((p[0] - data[0]).powi(2) + (p[1] - data[1]).powi(2)).sqrt();
                        (1. - dist / data[2].max(0.0001)).clamp(0., 1.)
                    };
                    blend_pixel_at(surface, x, y, color, band * radial);
                }
            }
        }
        DisplayItemKind::VectorPath => {
            if item.color[3] == 0 || item.edge_range[1] == 0 {
                return;
            }
            let first = item.edge_range[0] as usize * 4;
            let end = (item.edge_range[0] as usize + item.edge_range[1] as usize) * 4;
            let Some(raw) = edges_buf.get(first..end) else {
                return;
            };
            let shift = if item.scroll_mode == SCROLL_CONTENT
                || item.scroll_mode == super::SCROLL_AREA_CONTENT
                || item.area_slot.is_some()
            {
                scroll_shift(item.scroll_mode, item.scroll_extra, item.area_slot, opts)
            } else {
                [0., 0.]
            };
            for y in y0..y1 {
                let row_top = y as f32;
                let row_bot = row_top + 1.;
                for x in x0..x1 {
                    let mut area = 0f32;
                    let col = x as f32;
                    for e in raw.chunks_exact(4) {
                        let p0 = item.transform.apply_point([e[0], e[1]]);
                        let p1 = item.transform.apply_point([e[2], e[3]]);
                        let ay = (p0[1] - shift[1]) * opts.scale;
                        let by = (p1[1] - shift[1]) * opts.scale;
                        if ay == by {
                            continue;
                        }
                        let ya = ay.min(by).max(row_top);
                        let yb = ay.max(by).min(row_bot);
                        if ya >= yb {
                            continue;
                        }
                        let ax = (p0[0] - shift[0]) * opts.scale;
                        let bx = (p1[0] - shift[0]) * opts.scale;
                        let slope = (bx - ax) / (by - ay);
                        let fa = ax + (ya - ay) * slope;
                        let fb = ax + (yb - ay) * slope;
                        let contrib = geometry::column_area(fa, fb, col) * (yb - ya);
                        area += if by > ay { contrib } else { -contrib };
                    }
                    let cov = if item.nonzero {
                        area.abs().min(1.)
                    } else {
                        let w = area.abs() % 2.;
                        if w <= 1. { w } else { 2. - w }
                    };
                    blend_pixel_at(surface, x, y, item.color, cov);
                }
            }
        }
    }
}

fn pop_clip_layer(layer: ActiveLayer, scale: f32, parent: &mut TargetSurface<'_>) {
    let [lx, ly, lw, lh] = layer.region;
    let [px, py, pw, _ph] = parent.region;
    for y in ly..ly + lh {
        for x in lx..lx + lw {
            let src_idx = ((y - ly) * lw + (x - lx)) as usize * 4;
            let mut src: [u8; 4] = layer.pixels[src_idx..src_idx + 4].try_into().unwrap();
            if src[3] == 0 {
                continue;
            }
            let mask = if layer.crisp {
                let p = [(x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale];
                if p[0] >= layer.bounds[0]
                    && p[1] >= layer.bounds[1]
                    && p[0] < layer.bounds[0] + layer.bounds[2]
                    && p[1] < layer.bounds[1] + layer.bounds[3]
                {
                    1.
                } else {
                    0.
                }
            } else {
                geometry::round_coverage(x, y, scale, layer.bounds, layer.radius)
            };
            src[3] = (src[3] as f32 * mask).round() as u8;
            if src[3] == 0 {
                continue;
            }
            let dst_idx = ((y - py) * pw + (x - px)) as usize * 4;
            blend::over(&mut parent.pixels[dst_idx..dst_idx + 4], &src);
        }
    }
}

impl DisplayList {
    /// Rasterizes [`DisplayList::items`] and [`DisplayList::clips`] directly via the backend-independent IR.
    pub fn rasterize_rgba(
        &self,
        opts: &RasterizeOpts<'_>,
        out: &mut [u8],
    ) -> Result<(), &'static str> {
        let len = (opts.width as usize)
            .checked_mul(opts.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("Invalid raster output buffer size")?;
        if opts.width == 0
            || opts.height == 0
            || opts.width > 4096
            || opts.height > 4096
            || opts.width as u64 * opts.height as u64 > 8_388_608
            || !opts.scale.is_finite()
            || opts.scale <= 0.
            || len != out.len()
        {
            return Err("Invalid raster viewport or output slice");
        }
        out.fill(0);
        let root_region = [0, 0, opts.width, opts.height];
        let mut stack: Vec<ActiveLayer> = Vec::new();
        let mut current_clip: Option<ClipId> = None;

        let unwind_to = |keep: usize, stack: &mut Vec<ActiveLayer>, out_buf: &mut [u8]| {
            while stack.len() > keep {
                let top = stack.pop().unwrap();
                let mut parent = match stack.last_mut() {
                    Some(layer) => TargetSurface {
                        pixels: &mut layer.pixels,
                        region: layer.region,
                    },
                    None => TargetSurface {
                        pixels: out_buf,
                        region: root_region,
                    },
                };
                pop_clip_layer(top, opts.scale, &mut parent);
            }
        };

        for item in &self.items {
            if item.clip_id != current_clip {
                let keep = common_clip_prefix(&self.clips, current_clip, item.clip_id);
                unwind_to(keep, &mut stack, out);
                let mut chain = [0u32; 32];
                let chain_len = collect_clip_chain(&self.clips, item.clip_id, &mut chain);
                for &raw_id in &chain[keep..chain_len] {
                    let clip = &self.clips[raw_id as usize];
                    let scrolled = scrolled_item_bounds(
                        clip.bounds,
                        clip.scroll_mode,
                        0.,
                        clip.scroll_extra,
                        clip.area_slot,
                        opts,
                    );
                    let parent_region = stack.last().map_or(root_region, |l| l.region);
                    let node_region =
                        clip_pixel_bounds(scrolled, opts.scale, opts.width, opts.height);
                    let region = intersect_region(parent_region, node_region);
                    let byte_len = region[2] as usize * region[3] as usize * 4;
                    stack.push(ActiveLayer {
                        bounds: scrolled,
                        radius: clip.radius,
                        crisp: clip.crisp,
                        region,
                        pixels: vec![0; byte_len],
                    });
                }
                current_clip = item.clip_id;
            }
            let mut target = match stack.last_mut() {
                Some(layer) => TargetSurface {
                    pixels: &mut layer.pixels,
                    region: layer.region,
                },
                None => TargetSurface {
                    pixels: out,
                    region: root_region,
                },
            };
            rasterize_item(item, &self.edges, opts, &mut target);
        }
        unwind_to(0, &mut stack, out);
        Ok(())
    }

    /// Structured JSON snapshot of items, clip hierarchy, transforms, and resource IDs for Studio diagnostics.
    pub fn diagnostics_json(&self) -> String {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Diagnostics<'a> {
            item_count: usize,
            clip_count: usize,
            command_floats: usize,
            edge_floats: usize,
            clips: &'a [ClipNode],
            items: &'a [DisplayItem],
        }
        serde_json::to_string(&Diagnostics {
            item_count: self.items.len(),
            clip_count: self.clips.len(),
            command_floats: self.commands.len(),
            edge_floats: self.edges.len(),
            clips: &self.clips,
            items: &self.items,
        })
        .unwrap_or_else(|_| "{}".to_owned())
    }
}
