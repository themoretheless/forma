//! Single-entry, per-instance raster cache. A new model owns a new cache.
//! Geometry/content depend on viewport, DPI and scroll, not animated colors.

use crate::{
    display_list::{ControlPrimitive, ControlView},
    geometry::round_coverage,
    markup, shape, text,
};

mod frame;
mod region;

use frame::*;
pub(crate) use region::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct GeometryKey {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale: f32,
    pub(crate) scroll: [f32; 2],
    pub(crate) sparse: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct PaintKey {
    pub(crate) fill: [u8; 4],
    pub(crate) border: [u8; 4],
    pub(crate) reveal: crate::reveal::Paint,
    pub(crate) background: bool,
}

pub(crate) struct Cache {
    pub(crate) key: GeometryKey,
    // Four original coverage samples: fill_count * 5 + border_count.
    coverage: Vec<u8>,
    // Precise outer/border boundary coverage for square rectangles. Canvas
    // indices survive sparse packing; solid interiors use the usual palette.
    rect_edges: Vec<(usize, f32, f32)>,
    // Sparse, stable border coverage; pointer motion only changes its paint.
    reveal_pixels: Vec<(usize, f32)>,
    // Low five bits: 0..16 frame coverage. Upper bits: 0 clipped,
    // 1 content, 2 scrollbar. Equal consecutive samples share a run.
    frame: Vec<FrameRun>,
    content: Vec<u8>,
    // One span per nonempty row; content is compacted to these spans after build.
    content_spans: Vec<std::ops::Range<usize>>,
    unclipped_support: Option<[i32; 4]>,
    pub(crate) paint: Option<PaintKey>,
    pub(crate) pixels: Vec<u8>,
    // Packed pixels retain their original canvas indices, so fractional DPI
    // and overflowing content use exactly the same coverage samples.
    pub(crate) spans: Vec<std::ops::Range<usize>>,
    pub(crate) native_pixels: Vec<u32>,
}

/// Straight-alpha source-over, same rounding as the outline renderer.
pub(crate) use crate::blend::over;

impl Cache {
    pub(crate) fn try_translate(&mut self, model: ControlView<'_>, next: GeometryKey) -> bool {
        if !self.key.sparse
            || !next.sparse
            || self.key.scale != next.scale
            || model.scene.clip
            || model.scene.scroll
        {
            return false;
        }
        let Some(support) = self.unclipped_support else {
            return false;
        };
        let dx_f = (self.key.scroll[0] - next.scroll[0]) * next.scale;
        let dy_f = (self.key.scroll[1] - next.scroll[1]) * next.scale;
        if !dx_f.is_finite() || !dy_f.is_finite() {
            return false;
        }
        let dx = dx_f.round() as i32;
        let dy = dy_f.round() as i32;
        if (dx_f - dx as f32).abs() != 0. || (dy_f - dy as f32).abs() != 0. {
            return false;
        }
        let b = &model.scene.button;
        let old_px = (b.x - self.key.scroll[0]) * next.scale;
        let new_px = (b.x - next.scroll[0]) * next.scale;
        let old_py = (b.y - self.key.scroll[1]) * next.scale;
        let new_py = (b.y - next.scroll[1]) * next.scale;
        if new_px != old_px + dx as f32 || new_py != old_py + dy as f32 {
            return false;
        }
        let shifted = [
            support[0] + dx,
            support[1] + dy,
            support[2] + dx,
            support[3] + dy,
        ];
        if shifted[0] < 0
            || shifted[1] < 0
            || shifted[2] > next.width as i32
            || shifted[3] > next.height as i32
        {
            return false;
        }
        if dx == 0 && dy == 0 && self.key.width == next.width && self.key.height == next.height {
            self.key = next;
            return true;
        }
        let old_w = self.key.width as usize;
        let new_w = next.width as usize;
        let remap = |idx: usize| -> usize {
            let x = (idx % old_w) as i32 + dx;
            let y = (idx / old_w) as i32 + dy;
            y as usize * new_w + x as usize
        };
        for span in &mut self.spans {
            let len = span.len();
            let start = remap(span.start);
            *span = start..start + len;
        }
        for span in &mut self.content_spans {
            let len = span.len();
            let start = remap(span.start);
            *span = start..start + len;
        }
        for (idx, _, _) in &mut self.rect_edges {
            *idx = remap(*idx);
        }
        for (idx, _) in &mut self.reveal_pixels {
            *idx = remap(*idx);
        }
        self.unclipped_support = Some(shifted);
        self.key = next;
        if !self.reveal_pixels.is_empty() {
            self.paint = None;
        }
        self.native_pixels.clear();
        true
    }

    pub(crate) fn build(model: ControlView<'_>, key: GeometryKey) -> Self {
        Self::build_with_content_region(model, key, raster_region(model, key, false))
    }

    fn build_with_content_region(
        model: ControlView<'_>,
        key: GeometryKey,
        content_region: [u32; 4],
    ) -> Self {
        let GeometryKey {
            width,
            height,
            scale,
            ..
        } = key;
        let region = raster_region(model, key, true);
        let [rx, ry, rw, rh] = region;
        let offscreen = rw == 0 || rh == 0;
        let len = (rw * rh) as usize;
        let storage_index = |x: u32, y: u32| ((y - ry) * rw + x - rx) as usize;
        // Clip markers cannot produce pixels by themselves. A geometry-only
        // control needs no temporary RGBA content layer, even on the dense path.
        let [cx, cy, cw, ch] = content_region;
        let content_len = (cw * ch) as usize;
        let has_content = content_len > 0;
        let unclipped_support = if key.sparse && !model.scene.clip && !model.scene.scroll {
            control_support_unclamped(model, scale, true).filter(|[sx0, sy0, sx1, sy1]| {
                *sx0 >= 0 && *sy0 >= 0 && *sx1 <= width as i32 && *sy1 <= height as i32
            })
        } else {
            None
        };
        let mut cache = Self {
            key,
            coverage: vec![0; len],
            rect_edges: Vec::new(),
            reveal_pixels: Vec::new(),
            frame: Vec::new(),
            content: vec![0; content_len * 4],
            content_spans: Vec::new(),
            unclipped_support,
            paint: None,
            pixels: Vec::new(),
            spans: Vec::new(),
            native_pixels: Vec::new(),
        };
        if offscreen && key.sparse {
            return cache;
        }
        let b = &model.scene.button;
        let bx = b.x - model.scroll_x;
        let by = b.y - model.scroll_y;
        let r = b.radius.min(b.width / 2.).min(b.height / 2.);
        let bw = model.template.border.as_ref().map_or(0., |b| b.width);
        // No geometry evaluation in the empty part of the canvas.
        let x0 = (bx * scale).floor().clamp(0., width as f32) as u32;
        let y0 = (by * scale).floor().clamp(0., height as f32) as u32;
        let x1 = ((bx + b.width) * scale).ceil().clamp(0., width as f32) as u32;
        let y1 = ((by + b.height) * scale).ceil().clamp(0., height as f32) as u32;
        let half_w = b.width * 0.5;
        let half_h = b.height * 0.5;
        let center_x = bx + half_w;
        let center_y = by + half_h;
        let corner_w = half_w - r;
        let corner_h = half_h - r;
        let reveal_bounds = model
            .template
            .reveal
            .as_ref()
            .map(|_| model.reveal_bounds());
        let reveal_r = model
            .template
            .reveal
            .as_ref()
            .map_or(r, |rev| rev.target_radius.unwrap_or(r));
        let reveal_w = model.template.reveal.as_ref().map_or(0., |rev| rev.width);
        let default_reveal_bounds = model.template.reveal.as_ref().is_none_or(|rev| {
            rev.target_x.is_none()
                && rev.target_y.is_none()
                && rev.target_width.is_none()
                && rev.target_height.is_none()
                && rev.target_radius.is_none()
        });
        let safe_inset = r.max(bw).max(if default_reveal_bounds {
            reveal_w + 0.5 / scale
        } else {
            f32::MAX
        }) + 1.0 / scale;
        let ix0 = ((bx + safe_inset) * scale)
            .ceil()
            .clamp(x0 as f32, x1 as f32) as u32;
        let iy0 = ((by + safe_inset) * scale)
            .ceil()
            .clamp(y0 as f32, y1 as f32) as u32;
        let ix1 = ((bx + b.width - safe_inset) * scale)
            .floor()
            .clamp(x0 as f32, x1 as f32) as u32;
        let iy1 = ((by + b.height - safe_inset) * scale)
            .floor()
            .clamp(y0 as f32, y1 as f32) as u32;
        for y in y0..y1 {
            let row_inside = y >= iy0 && y < iy1 && ix1 > ix0;
            for x in x0..x1 {
                if row_inside && x >= ix0 && x < ix1 {
                    cache.coverage[storage_index(x, y)] = 20;
                    continue;
                }
                if r == 0. {
                    let area = crate::geometry::rect_area_coverage(
                        x,
                        y,
                        scale,
                        [bx, by, b.width, b.height],
                    );
                    let inner = crate::geometry::rect_area_coverage(
                        x,
                        y,
                        scale,
                        [bx + bw, by + bw, b.width - 2. * bw, b.height - 2. * bw],
                    );
                    let band = (area - inner).max(0.);
                    let index = (y * width + x) as usize;
                    cache.coverage[storage_index(x, y)] = if area == 0. {
                        0
                    } else if area == 1. && band == 1. {
                        24
                    } else {
                        20
                    };
                    if area > 0. && (area < 1. || (band > 0. && band < 1.)) {
                        cache.rect_edges.push((index, area, band));
                    }
                } else {
                    let mut fill = 0;
                    let mut border = 0;
                    for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                        let dx = ((x as f32 + sx) / scale - center_x).abs() - corner_w;
                        let dy = ((y as f32 + sy) / scale - center_y).abs() - corner_h;
                        let distance = if dx <= 0. && dy <= 0. {
                            dx.max(dy) - r
                        } else {
                            dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - r
                        };
                        if distance <= 0. {
                            fill += 1;
                            if bw > 0. && distance >= -bw {
                                border += 1;
                            }
                        }
                    }
                    cache.coverage[storage_index(x, y)] = fill * 5 + border;
                }
                if let Some(r_bounds) = reveal_bounds {
                    let band = crate::reveal::band_coverage(
                        [(x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale],
                        r_bounds,
                        reveal_r,
                        reveal_w,
                        scale,
                    );
                    if band > 0. {
                        cache.reveal_pixels.push(((y * width + x) as usize, band));
                    }
                }
            }
        }
        struct ClipLayer {
            parent: Vec<u8>,
            region: [u32; 4],
            bounds: [f32; 4],
            radius: f32,
        }
        cache.frame = frame_runs(model, key);
        let mut groups: Vec<ClipLayer> = Vec::new();
        // Sibling clip groups reuse the same layer, sized to the intersected node clip bounds.
        let mut layers: Vec<Vec<u8>> = Vec::new();
        let mut active_region = content_region;
        model.visit_primitives([bx, by], |primitive| match primitive {
            ControlPrimitive::Label { bounds, text: t } => {
                text::draw_text_dir_region(
                    t.family,
                    t.direction,
                    &mut text::RasterCanvas::subregion(
                        &mut cache.content,
                        width,
                        height,
                        scale,
                        active_region,
                    ),
                    &t.text,
                    t.font_size,
                    bounds,
                    t.color,
                );
            }
            ControlPrimitive::Text { bounds, text: t } if has_content => {
                text::draw_text_dir_region(
                    t.family,
                    t.direction,
                    &mut text::RasterCanvas::subregion(
                        &mut cache.content,
                        width,
                        height,
                        scale,
                        active_region,
                    ),
                    &t.text,
                    t.font_size,
                    bounds,
                    t.color,
                );
            }
            ControlPrimitive::Shape {
                origin,
                points,
                color,
            } if has_content => shape::draw_region(
                &mut text::RasterCanvas::subregion(
                    &mut cache.content,
                    width,
                    height,
                    scale,
                    active_region,
                ),
                points,
                origin,
                color,
            ),
            ControlPrimitive::Clip { bounds, radius } if has_content => {
                let [cx0, cy0, cx1, cy1] = clip_pixels(bounds, scale, width, height);
                let [px, py, pw, ph] = active_region;
                let lx0 = cx0.max(px);
                let ly0 = cy0.max(py);
                let lx1 = cx1.min(px + pw);
                let ly1 = cy1.min(py + ph);
                let next_region = if lx0 < lx1 && ly0 < ly1 {
                    [lx0, ly0, lx1 - lx0, ly1 - ly0]
                } else {
                    [0; 4]
                };
                let next_len = (next_region[2] as usize) * (next_region[3] as usize) * 4;
                let mut layer = layers.pop().unwrap_or_default();
                layer.clear();
                layer.resize(next_len, 0);
                groups.push(ClipLayer {
                    parent: std::mem::replace(&mut cache.content, layer),
                    region: active_region,
                    bounds,
                    radius,
                });
                active_region = next_region;
            }
            ControlPrimitive::ClipEnd if has_content => {
                if let Some(ClipLayer {
                    mut parent,
                    region: parent_region,
                    bounds,
                    radius,
                }) = groups.pop()
                {
                    let [lx, ly, lw, lh] = active_region;
                    let [px, py, pw, _ph] = parent_region;
                    for y in ly..ly + lh {
                        for x in lx..lx + lw {
                            let i = ((y - ly) * lw + (x - lx)) as usize * 4;
                            if cache.content[i + 3] == 0 {
                                continue;
                            }
                            let mut pixel: [u8; 4] = cache.content[i..i + 4].try_into().unwrap();
                            pixel[3] = (pixel[3] as f32
                                * round_coverage(x, y, scale, bounds, radius))
                            .round() as u8;
                            let dst_i = ((y - py) * pw + (x - px)) as usize * 4;
                            over(&mut parent[dst_i..dst_i + 4], &pixel);
                        }
                    }
                    let finished = std::mem::replace(&mut cache.content, parent);
                    if finished.capacity() > 0 {
                        layers.push(finished);
                    }
                    active_region = parent_region;
                }
            }
            _ => {}
        });
        drop(layers);
        drop(groups);
        // Most canvases contain only a few rows of text/shapes. Keep their exact
        // RGBA samples, without retaining four bytes for every transparent pixel.
        let mut compacted = 0;
        for y in 0..ch as usize {
            let row_start = y * cw as usize;
            let row = &cache.content[row_start * 4..(row_start + cw as usize) * 4];
            if let Some(first_alpha_byte) = row.iter().skip(3).step_by(4).position(|&a| a != 0) {
                let first = first_alpha_byte;
                let last = row
                    .iter()
                    .skip(3)
                    .step_by(4)
                    .rposition(|&a| a != 0)
                    .unwrap_or(first);
                let span = row_start + first..row_start + last + 1;
                let bytes = span.len() * 4;
                cache
                    .content
                    .copy_within(span.start * 4..span.end * 4, compacted);
                compacted += bytes;
                let global_row = (y + cy as usize) * width as usize + cx as usize;
                cache
                    .content_spans
                    .push(global_row + first..global_row + last + 1);
            }
        }
        cache.content.truncate(compacted);
        cache.content.shrink_to_fit();
        {
            // Each row is the union of the button's possible animated paint
            // (including reveal) and actual static content. Content may extend
            // outside the button, including on rows above/below its rectangle.
            let mut content_spans = cache.content_spans.iter().peekable();
            let mut packed = 0;
            for y in ry..ry + rh {
                let row = (y * width) as usize;
                let mut start = width as usize;
                let mut end = 0;
                if y >= y0 && y < y1 && x0 < x1 {
                    start = x0 as usize;
                    end = x1 as usize;
                }
                if let Some(span) = content_spans.peek() {
                    if span.start / width as usize == y as usize {
                        start = start.min(span.start - row);
                        end = end.max(span.end - row);
                        content_spans.next();
                    }
                }
                if start < end {
                    let span = row + start..row + end;
                    let local_start = storage_index(start as u32, y);
                    cache
                        .coverage
                        .copy_within(local_start..local_start + span.len(), packed);
                    packed += span.len();
                    cache.spans.push(span);
                }
            }
            cache.coverage.truncate(packed);
            cache.coverage.shrink_to_fit();
        }
        let pixel_len = if key.sparse {
            cache.coverage.len()
        } else {
            width as usize * height as usize
        };
        cache.pixels.resize(pixel_len * 4, 0);
        cache
    }

    pub(crate) fn paint(&mut self, scene: &markup::Scene, key: PaintKey) {
        // Only 15 combinations are possible; evaluate color mixing once for each,
        // instead of four samples and square roots for every pixel on every frame.
        let mut palette = [[0u8; 4]; 25];
        let fa = key.fill[3] as f32 / 255.;
        let ba = key.border[3] as f32 / 255.;
        for f in 0..=4 {
            for b in 0..=f {
                let a = (fa * (f - b) as f32 + (fa * (1. - ba) + ba) * b as f32) / 4.;
                if a > 0. {
                    for (chan, (&fill_c, &border_c)) in palette[f * 5 + b][..3]
                        .iter_mut()
                        .zip(key.fill[..3].iter().zip(&key.border[..3]))
                    {
                        *chan = ((fill_c as f32 * fa * (f - b) as f32
                            + (fill_c as f32 * fa * (1. - ba) + border_c as f32 * ba) * b as f32)
                            / 4.
                            / a)
                            .round() as u8;
                    }
                }
                palette[f * 5 + b][3] = (a * 255.).round() as u8;
            }
        }
        let mut reveal_pixels = self.reveal_pixels.iter().peekable();
        let mut rect_edges = self.rect_edges.iter().peekable();
        let mut content_spans = self.content_spans.iter();
        let mut content_span = content_spans.next();
        let mut content_pixels = self.content.chunks_exact(4);
        if self.key.sparse {
            let indices = self.spans.iter().flat_map(|span| span.clone());
            for ((i, dst), packed) in indices.zip(self.pixels.chunks_exact_mut(4)).zip(0..) {
                dst.copy_from_slice(&palette[self.coverage[packed] as usize]);
                if let Some(&&(index, area, band)) = rect_edges.peek() {
                    if index == i {
                        dst.copy_from_slice(&crate::geometry::rect_area_paint(
                            key.fill, key.border, area, band,
                        ));
                        rect_edges.next();
                    }
                }
                if let Some(span) = content_span {
                    if i >= span.start {
                        over(dst, content_pixels.next().unwrap());
                        if i + 1 == span.end {
                            content_span = content_spans.next();
                        }
                    }
                }
                if let Some(&&(index, band)) = reveal_pixels.peek() {
                    if index == i {
                        let x = (i % self.key.width as usize) as f32;
                        let y = (i / self.key.width as usize) as f32;
                        let mut color = key
                            .reveal
                            .color_at([(x + 0.5) / self.key.scale, (y + 0.5) / self.key.scale]);
                        color[3] = (color[3] as f32 * band).round() as u8;
                        over(dst, &color);
                        reveal_pixels.next();
                    }
                }
            }
        } else {
            // Base geometry and static content occupy packed global spans;
            // only the final frame/background composition needs a dense pass.
            self.pixels.fill(0);
            let mut packed = 0;
            for span in &self.spans {
                for (dst, &coverage) in self.pixels[span.start * 4..span.end * 4]
                    .chunks_exact_mut(4)
                    .zip(&self.coverage[packed..packed + span.len()])
                {
                    dst.copy_from_slice(&palette[coverage as usize]);
                }
                packed += span.len();
            }
            for &(i, area, band) in &self.rect_edges {
                self.pixels[i * 4..i * 4 + 4].copy_from_slice(&crate::geometry::rect_area_paint(
                    key.fill, key.border, area, band,
                ));
            }
            let mut packed = 0;
            for span in &self.content_spans {
                let bytes = span.len() * 4;
                for (dst, src) in self.pixels[span.start * 4..span.end * 4]
                    .chunks_exact_mut(4)
                    .zip(self.content[packed..packed + bytes].chunks_exact(4))
                {
                    over(dst, src);
                }
                packed += bytes;
            }
            for &(i, band) in &self.reveal_pixels {
                let x = (i % self.key.width as usize) as f32;
                let y = (i / self.key.width as usize) as f32;
                let mut color = key
                    .reveal
                    .color_at([(x + 0.5) / self.key.scale, (y + 0.5) / self.key.scale]);
                color[3] = (color[3] as f32 * band).round() as u8;
                over(&mut self.pixels[i * 4..i * 4 + 4], &color);
            }
            let mut start = 0;
            for &FrameRun { end, value: frame } in &self.frame {
                let pixels = &mut self.pixels[start * 4..end as usize * 4];
                start = end as usize;
                match frame >> 5 {
                    0 => pixels.fill(0),
                    2 => {
                        for dst in pixels.chunks_exact_mut(4) {
                            dst.copy_from_slice(&[110, 130, 170, 255]);
                        }
                    }
                    _ => {}
                }
                let frame_coverage = frame & 31;
                let coverage = frame_coverage as f32 / 16.;
                if key.background {
                    let mut background = scene.background;
                    if !scene.clip {
                        background[3] = (background[3] as f32 * coverage).round() as u8;
                    }
                    for dst in pixels.chunks_exact_mut(4) {
                        if dst[3] == 0 {
                            dst.copy_from_slice(&background);
                        } else if dst[3] != 255 {
                            let mut bg = background;
                            over(&mut bg, dst);
                            dst.copy_from_slice(&bg);
                        }
                    }
                }
                if scene.clip && frame_coverage != 16 {
                    for dst in pixels.chunks_exact_mut(4) {
                        dst[3] = (dst[3] as f32 * coverage).round() as u8;
                        if dst[3] == 0 {
                            dst.fill(0);
                        }
                    }
                }
            }
        }
        self.paint = Some(key);
        self.native_pixels.clear();
    }
}

#[cfg(test)]
mod tests;
