//! Single-entry, per-instance raster cache. A new model owns a new cache.
//! Geometry/content depend on viewport, DPI and scroll, not animated colors.
use crate::{
    display_list::ControlView, geometry::round_coverage, markup, shape, template::Content, text,
};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameRun {
    // Exclusive global pixel index; raster size is bounded below u32::MAX.
    end: u32,
    value: u8,
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
    pub(crate) paint: Option<PaintKey>,
    pub(crate) pixels: Vec<u8>,
    // Packed pixels retain their original canvas indices, so fractional DPI
    // and overflowing content use exactly the same coverage samples.
    pub(crate) spans: Vec<std::ops::Range<usize>>,
    pub(crate) native_pixels: Vec<u32>,
}

/// Straight-alpha source-over, same rounding as the outline renderer.
pub(crate) use crate::blend::over;

// Conservative support of geometry plus content, or content alone. Clip
// markers only remove support; overflowing text/shapes must remain visible.
fn raster_region(model: ControlView<'_>, key: GeometryKey, include_geometry: bool) -> [u32; 4] {
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

#[cfg(test)]
thread_local! { static FRAME_SAMPLES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn frame_value(model: ControlView<'_>, key: GeometryKey, v: [f32; 4], x: u32, y: u32) -> u8 {
    #[cfg(test)]
    FRAME_SAMPLES.with(|count| count.set(count.get() + 1));
    let [vx, vy, vw, vh] = v;
    let b = &model.scene.button;
    let mut frame = (round_coverage(
        x,
        y,
        key.scale,
        [0., 0., model.width(), model.height()],
        model.scene.radius,
    ) * 16.) as u8;
    let mut viewport = 1;
    if model.scene.scroll {
        let px = (x as f32 + 0.5) / key.scale;
        let py = (y as f32 + 0.5) / key.scale;
        if px < vx || py < vy || px >= vx + vw || py >= vy + vh {
            viewport = 0;
        }
        if vw > 0. && vh > 0. {
            let ch = b.height;
            let cw = b.width;
            let vertical = ch > vh
                && px >= (vx + vw - 5.).max(vx)
                && px < vx + vw
                && py >= vy + model.scroll_y / ch * vh
                && py < vy + (model.scroll_y + vh) / ch * vh;
            let horizontal = cw > vw
                && py >= (vy + vh - 5.).max(vy)
                && py < vy + vh
                && px >= vx + model.scroll_x / cw * vw
                && px < vx + (model.scroll_x + vw) / cw * vw;
            if vertical || horizontal {
                viewport = 2;
            }
        }
    }
    frame |= viewport << 5;
    frame
}

fn append_frame_run(runs: &mut Vec<FrameRun>, end: u32, value: u8) {
    match runs.last_mut() {
        Some(run) if run.value == value => run.end = end,
        _ => runs.push(FrameRun { end, value }),
    }
}

// All x comparisons use the original pixel-center arithmetic. This avoids
// floor/ceil conversions changing inclusive/exclusive viewport boundaries.
fn frame_x_boundary(width: u32, scale: f32, threshold: f32) -> u32 {
    let (mut lo, mut hi) = (0, width);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if (mid as f32 + 0.5) / scale >= threshold {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

fn frame_runs(model: ControlView<'_>, key: GeometryKey) -> Vec<FrameRun> {
    let mut runs = Vec::new();
    if key.sparse {
        return runs;
    }
    let v = model.viewport_rect();
    let [vx, _, vw, vh] = v;
    let b = &model.scene.button;
    let mut thresholds = vec![model.width() / 2.];
    if model.scene.scroll {
        thresholds.extend([vx, vx + vw]);
        if vw > 0. && vh > 0. {
            thresholds.push((vx + vw - 5.).max(vx));
            if b.width > vw {
                thresholds.extend([
                    vx + model.scroll_x / b.width * vw,
                    vx + (model.scroll_x + vw) / b.width * vw,
                ]);
            }
        }
    }
    let monotone = key.width >= 64
        && model.width().is_finite()
        && model.height().is_finite()
        && model.width() > 0.
        && model.height() > 0.
        && model.scene.radius.is_finite()
        && model.scene.radius >= 0.
        && v.iter().chain(thresholds.iter()).all(|n| n.is_finite());
    let mut cuts = vec![0, key.width];
    if monotone {
        cuts.extend(
            thresholds
                .iter()
                .map(|&x| frame_x_boundary(key.width, key.scale, x)),
        );
        cuts.sort_unstable();
        cuts.dedup();
    }
    for y in 0..key.height {
        if !monotone {
            for x in 0..key.width {
                append_frame_run(
                    &mut runs,
                    y * key.width + x + 1,
                    frame_value(model, key, v, x, y),
                );
            }
            continue;
        }
        // Each interval has constant viewport/scrollbar classification and
        // lies on one side of the rounded frame center. Its coverage is
        // monotone, so an equal-value prefix cannot recur after it ends.
        for interval in cuts.windows(2) {
            let mut x = interval[0];
            while x < interval[1] {
                let value = frame_value(model, key, v, x, y);
                let (mut lo, mut hi) = (x + 1, interval[1]);
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if frame_value(model, key, v, mid, y) == value {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                append_frame_run(&mut runs, y * key.width + lo, value);
                x = lo;
            }
        }
    }
    runs
}

impl Cache {
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
        let content_index = |x: u32, y: u32| ((y - cy) * cw + x - cx) as usize;
        let mut cache = Self {
            key,
            coverage: vec![0; len],
            rect_edges: Vec::new(),
            reveal_pixels: Vec::new(),
            frame: Vec::new(),
            content: vec![0; content_len * 4],
            content_spans: Vec::new(),
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
        cache.frame = frame_runs(model, key);
        if let Some(t) = &model.template.text {
            text::draw_text_region(
                t.family,
                &mut text::RasterCanvas::subregion(
                    &mut cache.content,
                    width,
                    height,
                    scale,
                    content_region,
                ),
                &t.text,
                t.font_size,
                [
                    bx + r * 0.3,
                    by + r * 0.3,
                    b.width - r * 0.6,
                    b.height - r * 0.6,
                ],
                t.color,
            );
        }
        let mut groups = Vec::new();
        // Sibling clip groups reuse the same layer. Only nesting depth, rather
        // than total group count, determines region scratch allocations.
        let mut layers: Vec<Vec<u8>> = Vec::new();
        for c in model.template.content.iter().filter(|_| has_content) {
            match c {
                Content::Text { bounds, text: t } => text::draw_text_region(
                    t.family,
                    &mut text::RasterCanvas::subregion(
                        &mut cache.content,
                        width,
                        height,
                        scale,
                        content_region,
                    ),
                    &t.text,
                    t.font_size,
                    [bx + bounds[0], by + bounds[1], bounds[2], bounds[3]],
                    t.color,
                ),
                Content::Shape { points, color } => shape::draw_region(
                    &mut text::RasterCanvas::subregion(
                        &mut cache.content,
                        width,
                        height,
                        scale,
                        content_region,
                    ),
                    points,
                    [bx, by],
                    *color,
                ),
                Content::Clip { bounds, radius } => {
                    let layer = match layers.pop() {
                        Some(mut layer) => {
                            layer.fill(0);
                            layer
                        }
                        None => vec![0; content_len * 4],
                    };
                    groups.push((
                        std::mem::replace(&mut cache.content, layer),
                        [bounds[0] + bx, bounds[1] + by, bounds[2], bounds[3]],
                        *radius,
                    ));
                }
                Content::ClipEnd => {
                    if let Some((mut parent, bounds, radius)) = groups.pop() {
                        // round_coverage has no support outside these bounds;
                        // one physical pixel conservatively includes every AA sample.
                        let [x0, y0, x1, y1] = clip_pixels(bounds, scale, width, height);
                        let (x0, y0, x1, y1) =
                            (x0.max(cx), y0.max(cy), x1.min(cx + cw), y1.min(cy + ch));
                        for y in y0..y1 {
                            for x in x0..x1 {
                                let i = content_index(x, y) * 4;
                                if cache.content[i + 3] == 0 {
                                    continue;
                                }
                                let mut pixel: [u8; 4] =
                                    cache.content[i..i + 4].try_into().unwrap();
                                pixel[3] = (pixel[3] as f32
                                    * round_coverage(x, y, scale, bounds, radius))
                                .round() as u8;
                                over(&mut parent[i..i + 4], &pixel);
                            }
                        }
                        layers.push(std::mem::replace(&mut cache.content, parent));
                    }
                }
            }
        }
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

pub(crate) fn valid_raster_size(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= 4096
        && height <= 4096
        && width as u64 * height as u64 <= 8_388_608
}

// Physical support of a rounded clip, retaining a full-pixel AA margin before
// clamping. Empty clips remain empty even when the margin would create a box.
fn clip_pixels(bounds: [f32; 4], scale: f32, width: u32, height: u32) -> [u32; 4] {
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

#[cfg(test)]
mod tests;
