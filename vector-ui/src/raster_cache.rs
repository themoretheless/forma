//! Single-entry, per-instance raster cache. A new model owns a new cache.
//! Geometry/content depend on viewport, DPI and scroll, not animated colors.
use crate::{round_coverage, shape, template::Content, text, Button};

#[derive(Clone, Copy, PartialEq)]
struct GeometryKey {
    width: u32,
    height: u32,
    scale: f32,
    scroll: [f32; 2],
    sparse: bool,
}
#[derive(Clone, Copy, PartialEq)]
struct PaintKey {
    fill: [u8; 4],
    border: [u8; 4],
    reveal: crate::reveal::Paint,
    background: bool,
}

pub(crate) struct Cache {
    key: GeometryKey,
    // Four original coverage samples: fill_count * 5 + border_count.
    coverage: Vec<u8>,
    // Sparse, stable border coverage; pointer motion only changes its paint.
    reveal_pixels: Vec<(usize, f32)>,
    // Low five bits: 0..16 frame coverage. Upper bits: 0 clipped,
    // 1 content, 2 scrollbar. One map replaces two full-canvas byte arrays.
    frame: Vec<u8>,
    content: Vec<u8>,
    // One span per nonempty row; content is compacted to these spans after build.
    content_spans: Vec<std::ops::Range<usize>>,
    paint: Option<PaintKey>,
    pixels: Vec<u8>,
    // Packed pixels retain their original canvas indices, so fractional DPI
    // and overflowing content use exactly the same coverage samples.
    spans: Vec<std::ops::Range<usize>>,
    native_pixels: Vec<u32>,
}

/// Straight-alpha source-over, same rounding as the outline renderer.
pub(crate) fn over(dst: &mut [u8], src: &[u8]) {
    if src[3] == 0 {
        return;
    }
    if src[3] == 255 || dst[3] == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let sa = src[3] as f32 / 255.;
    let retained = dst[3] as f32 / 255. * (1. - sa);
    let a = sa + retained;
    for k in 0..3 {
        dst[k] = ((src[k] as f32 * sa + dst[k] as f32 * retained) / a).round() as u8;
    }
    dst[3] = (a * 255.).round() as u8;
}

impl Cache {
    fn build(model: &Button, key: GeometryKey) -> Self {
        let GeometryKey {
            width,
            height,
            scale,
            ..
        } = key;
        let len = (width * height) as usize;
        let mut cache = Self {
            key,
            coverage: vec![0; len],
            reveal_pixels: Vec::new(),
            frame: if key.sparse { Vec::new() } else { vec![0; len] },
            content: vec![0; len * 4],
            content_spans: Vec::new(),
            paint: None,
            pixels: Vec::new(),
            spans: Vec::new(),
            native_pixels: Vec::new(),
        };
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
        for y in y0..y1 {
            for x in x0..x1 {
                let mut fill = 0;
                let mut border = 0;
                for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let dx =
                        ((x as f32 + sx) / scale - bx - b.width / 2.).abs() - (b.width / 2. - r);
                    let dy =
                        ((y as f32 + sy) / scale - by - b.height / 2.).abs() - (b.height / 2. - r);
                    let distance = dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - r;
                    if distance <= 0. {
                        fill += 1;
                        if bw > 0. && distance >= -bw {
                            border += 1;
                        }
                    }
                }
                cache.coverage[(y * width + x) as usize] = fill * 5 + border;
                if let Some(reveal) = &model.template.reveal {
                    let band = crate::reveal::band_coverage(
                        [(x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale],
                        model.reveal_bounds(), reveal.target_radius.unwrap_or(r), reveal.width, scale,
                    );
                    if band > 0. { cache.reveal_pixels.push(((y * width + x) as usize, band)); }
                }
            }
        }
        let v = model.viewport_rect();
        let [vx, vy, vw, vh] = [v[0], v[1], v[2], v[3]];
        for y in 0..if key.sparse { 0 } else { height } {
            for x in 0..width {
                let i = (y * width + x) as usize;
                cache.frame[i] = (round_coverage(
                    x,
                    y,
                    scale,
                    [0., 0., model.width(), model.height()],
                    model.scene.radius,
                ) * 16.) as u8;
                let mut viewport = 1;
                if model.scene.scroll {
                    let px = (x as f32 + 0.5) / scale;
                    let py = (y as f32 + 0.5) / scale;
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
                cache.frame[i] |= viewport << 5;
            }
        }
        if let Some(t) = &model.template.text {
            text::draw_text(
                &mut cache.content,
                width,
                height,
                scale,
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
        // than total group count, determines full-canvas scratch allocations.
        let mut layers: Vec<Vec<u8>> = Vec::new();
        for c in &model.template.content {
            match c {
                Content::Text { bounds, text: t } => text::draw_text(
                    &mut cache.content,
                    width,
                    height,
                    scale,
                    &t.text,
                    t.font_size,
                    [bx + bounds[0], by + bounds[1], bounds[2], bounds[3]],
                    t.color,
                ),
                Content::Shape { points, color } => shape::draw(
                    &mut cache.content,
                    width,
                    height,
                    scale,
                    points,
                    [bx, by],
                    *color,
                ),
                Content::Clip { bounds, radius } => {
                    let layer = match layers.pop() {
                        Some(mut layer) => { layer.fill(0); layer }
                        None => vec![0; len * 4],
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
                        for y in y0..y1 {
                            for x in x0..x1 {
                                let i = ((y * width + x) * 4) as usize;
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
        for y in 0..height as usize {
            let row_start = y * width as usize;
            let row = &cache.content[row_start * 4..(row_start + width as usize) * 4];
            let mut visible = row.chunks_exact(4).enumerate().filter(|(_, p)| p[3] != 0);
            let Some((first, _)) = visible.next() else { continue };
            let last = visible.next_back().map_or(first, |(x, _)| x);
            let span = row_start + first..row_start + last + 1;
            let bytes = span.len() * 4;
            cache.content.copy_within(span.start * 4..span.end * 4, compacted);
            compacted += bytes;
            cache.content_spans.push(span);
        }
        cache.content.truncate(compacted);
        cache.content.shrink_to_fit();
        if key.sparse {
            // Each row is the union of the button's possible animated paint
            // (including reveal) and actual static content. Content may extend
            // outside the button, including on rows above/below its rectangle.
            let mut content_spans = cache.content_spans.iter().peekable();
            let mut packed = 0;
            for y in 0..height {
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
                    cache.coverage.copy_within(span.clone(), packed);
                    packed += span.len();
                    cache.spans.push(span);
                }
            }
            cache.coverage.truncate(packed);
            cache.coverage.shrink_to_fit();
        } else {
            cache.spans.push(0..len);
        }
        cache.pixels.resize(cache.coverage.len() * 4, 0);
        cache
    }

    fn paint(&mut self, model: &Button, key: PaintKey) {
        // Only 15 combinations are possible; evaluate color mixing once for each,
        // instead of four samples and square roots for every pixel on every frame.
        let mut palette = [[0u8; 4]; 25];
        let fa = key.fill[3] as f32 / 255.;
        let ba = key.border[3] as f32 / 255.;
        for f in 0..=4 {
            for b in 0..=f {
                let a = (fa * (f - b) as f32 + (fa * (1. - ba) + ba) * b as f32) / 4.;
                if a > 0. {
                    for k in 0..3 {
                        palette[f * 5 + b][k] = ((key.fill[k] as f32 * fa * (f - b) as f32
                            + (key.fill[k] as f32 * fa * (1. - ba) + key.border[k] as f32 * ba)
                                * b as f32)
                            / 4.
                            / a)
                            .round() as u8;
                    }
                }
                palette[f * 5 + b][3] = (a * 255.).round() as u8;
            }
        }
        let mut reveal_pixels = self.reveal_pixels.iter().peekable();
        let mut content_spans = self.content_spans.iter();
        let mut content_span = content_spans.next();
        let mut content_pixels = self.content.chunks_exact(4);
        if self.key.sparse {
            let indices = self.spans.iter().flat_map(|span| span.clone());
            for ((i, dst), packed) in indices.zip(self.pixels.chunks_exact_mut(4)).zip(0..) {
                dst.copy_from_slice(&palette[self.coverage[packed] as usize]);
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
                        let mut color = key.reveal.color_at([(x + 0.5) / self.key.scale, (y + 0.5) / self.key.scale]);
                        color[3] = (color[3] as f32 * band).round() as u8;
                        over(dst, &color);
                        reveal_pixels.next();
                    }
                }
            }
        } else {
            // Keep the full-canvas loop dense. Span iteration and the sparse
            // branch must not add work to every pixel of a single control.
            for (i, dst) in self.pixels.chunks_exact_mut(4).enumerate() {
                dst.copy_from_slice(&palette[self.coverage[i] as usize]);
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
                        let mut color = key.reveal.color_at([(x + 0.5) / self.key.scale, (y + 0.5) / self.key.scale]);
                        color[3] = (color[3] as f32 * band).round() as u8;
                        over(dst, &color);
                        reveal_pixels.next();
                    }
                }

                let frame = self.frame[i];
                match frame >> 5 {
                    0 => dst.fill(0),
                    2 => dst.copy_from_slice(&[110, 130, 170, 255]),
                    _ => {}
                }
                let frame_coverage = frame & 31;
                let coverage = frame_coverage as f32 / 16.;
                if key.background && dst[3] != 255 {
                    let mut bg = model.scene.background;
                    if !model.scene.clip {
                        bg[3] = (bg[3] as f32 * coverage).round() as u8;
                    }
                    over(&mut bg, dst);
                    dst.copy_from_slice(&bg);
                }
                if model.scene.clip && frame_coverage != 16 {
                    dst[3] = (dst[3] as f32 * coverage).round() as u8;
                    if dst[3] == 0 {
                        dst.fill(0);
                    }
                }
            }
        }
        self.paint = Some(key);
        self.native_pixels.clear();
    }
}

impl Button {
    fn cached(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
        sparse: bool,
    ) -> Option<std::cell::RefMut<'_, Cache>> {
        if !valid_raster_size(width, height) || !scale.is_finite() || scale <= 0. {
            return None;
        }
        let geometry = GeometryKey {
            width,
            height,
            scale,
            scroll: [self.scroll_x, self.scroll_y],
            sparse,
        };
        let paint = PaintKey {
            fill: self.fill.color(),
            border: self.border.color(),
            reveal: self.reveal_paint(),
            background,
        };
        let mut stored = self.raster_cache.borrow_mut();
        if stored.as_ref().is_none_or(|cache| cache.key != geometry) {
            *stored = Some(Cache::build(self, geometry));
            self.raster_builds
                .set(self.raster_builds.get().wrapping_add(1));
        }
        let cache = stored.as_mut().unwrap();
        if cache.paint != Some(paint) {
            cache.paint(self, paint);
            self.raster_paints
                .set(self.raster_paints.get().wrapping_add(1));
        }
        Some(std::cell::RefMut::map(stored, |cache| {
            cache.as_mut().unwrap()
        }))
    }

    pub(crate) fn raster_cached(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
    ) -> Vec<u8> {
        self.cached(width, height, scale, background, false)
            .map_or_else(Vec::new, |cache| cache.pixels.clone())
    }

    /// Composite a leaf without allocating/copying a transparent full canvas.
    /// Root frame clipping and scrollbars are applied once by Runtime.
    pub(crate) fn composite_content(&self, dst: &mut [u8], width: u32, height: u32, scale: f32) {
        debug_assert!(!self.scene.clip && !self.scene.scroll);
        let Some(cache) = self.cached(width, height, scale, false, true) else { return };
        let mut pixels = cache.pixels.as_slice();
        for span in &cache.spans {
            let bytes = span.len() * 4;
            for (dst, src) in dst[span.start * 4..span.end * 4]
                .chunks_exact_mut(4).zip(pixels[..bytes].chunks_exact(4)) {
                over(dst, src);
            }
            pixels = &pixels[bytes..];
        }
    }

    /// Paint into the host-owned XRGB buffer without a full-window RGBA copy.
    /// The current runtime has fixed logical layout: resizing the host does not
    /// change it. Keep that scene cached at native DPI and crop/copy on resize.
    /// This intentionally does not stretch content or lower quality while dragging.
    pub fn paint_native(
        &self,
        dst: &mut [u32],
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<(), &'static str> {
        let len = (width as usize)
            .checked_mul(height as usize)
            .ok_or("Native buffer size overflow")?;
        if width == 0 || height == 0 || len != dst.len() || !scale.is_finite() || scale <= 0. {
            return Err("Invalid native buffer or DPI");
        }
        let [right, bottom] = self.paint_extent();
        let mut rw = (right * scale).ceil().max(1.) as u32;
        let mut rh = (bottom * scale).ceil().max(1.) as u32;
        // Do not allocate an unbounded offscreen scene. For oversized content,
        // retain only the visible portion (the existing raster safety limit).
        if !valid_raster_size(rw, rh) {
            rw = rw.min(width);
            rh = rh.min(height);
        }
        let mut cache = self
            .cached(rw, rh, scale, true, false)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        if cache.native_pixels.is_empty() {
            let Cache { native_pixels, pixels, .. } = &mut *cache;
            native_pixels.extend(pixels.chunks_exact(4).map(|p| {
                let a = p[3] as u32;
                let c = |k: usize, bg: u32| (p[k] as u32 * a + bg * (255 - a) + 127) / 255;
                (c(0, 17) << 16) | (c(1, 19) << 8) | c(2, 25)
            }));
        }
        let pixels = &cache.native_pixels;
        let copy_width = width.min(rw) as usize;
        let copy_height = height.min(rh) as usize;
        for y in 0..copy_height {
            let row = &mut dst[y * width as usize..(y + 1) * width as usize];
            row[..copy_width]
                .copy_from_slice(&pixels[y * rw as usize..y * rw as usize + copy_width]);
            row[copy_width..].fill(0x111319);
        }
        dst[copy_height * width as usize..].fill(0x111319);
        Ok(())
    }

    // Conservative bounds include overflowing template primitives, not just the
    // Button box. Clip/Scroll restrict them to the root viewport. Text already
    // clips itself to its own bounds; an extra pixel covers fractional edges.
    fn paint_extent(&self) -> [f32; 2] {
        let mut extent = [self.width(), self.height()];
        if self.scene.clip || self.scene.scroll {
            return extent;
        }
        let b = &self.scene.button;
        let x = b.x - self.scroll_x;
        let y = b.y - self.scroll_y;
        extent[0] = extent[0].max(x + b.width);
        extent[1] = extent[1].max(y + b.height);
        for content in &self.template.content {
            match content {
                Content::Text { bounds, .. } => {
                    extent[0] = extent[0].max(x + bounds[0] + bounds[2] + 1.);
                    extent[1] = extent[1].max(y + bounds[1] + bounds[3] + 1.);
                }
                Content::Shape { points, .. } => {
                    for p in points {
                        extent[0] = extent[0].max(x + p[0]);
                        extent[1] = extent[1].max(y + p[1]);
                    }
                }
                _ => {}
            }
        }
        extent
    }
}

fn valid_raster_size(width: u32, height: u32) -> bool {
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
        ((bounds[0] + bounds[2]) * scale + 1.).ceil().clamp(0., width as f32) as u32,
        ((bounds[1] + bounds[3]) * scale + 1.).ceil().clamp(0., height as f32) as u32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_leaf_retains_only_local_rows_and_empty_offscreen_leaf_is_valid() {
        let source = "component Demo { Frame { width:1000; height:600; padding:0; Button { x:700.3; y:400.7; width:40; height:24; } } }";
        let component = "component Button { Rectangle { radius:4; background:#abcdef80; Reveal { color:#ffaa44; } ContentText { x:-6; y:-14; width:60; height:12; text:'Я'; fontSize:11; color:#ffffff; } ContentShape { points:'-8 3 6 3 6 8 -8 8'; color:#334455; } } }";
        let model = Button::from_sources(source, component).unwrap();
        let expected = model.content_pixels(1000, 600, 1.25);
        let mut actual = vec![0; expected.len()];
        model.composite_content(&mut actual, 1000, 600, 1.25);
        assert_eq!(actual, expected);
        {
            let stored = model.raster_cache.borrow();
            let cache = stored.as_ref().unwrap();
            assert!(cache.key.sparse && cache.frame.is_empty());
            assert_eq!(cache.pixels.len(), cache.coverage.len() * 4);
            assert_eq!(cache.coverage.len(), cache.spans.iter().map(|span| span.len()).sum());
            assert!(cache.coverage.capacity() < 3000, "packed geometry must not retain a full canvas");
            assert!(cache.pixels.capacity() < 12_000, "packed RGBA must not retain a full canvas");
            assert!(cache.content.capacity() < 2000);
            assert!(cache.spans.windows(2).all(|pair| pair[0].end <= pair[1].start));
            assert!(cache.spans.iter().all(|span| span.start < span.end && span.start / 1000 == (span.end - 1) / 1000));
        }
        let mut outside = vec![0; 20 * 20 * 4];
        model.composite_content(&mut outside, 20, 20, 1.25);
        assert!(outside.iter().all(|&b| b == 0));
        let stored = model.raster_cache.borrow();
        let cache = stored.as_ref().unwrap();
        assert!(cache.spans.is_empty() && cache.pixels.is_empty() && cache.coverage.is_empty());
    }

    #[test]
    fn sparse_content_retains_only_occupied_rows_without_changing_pixels() {
        let source = "component Demo { Frame { width:160; height:120; padding:0; Button { width:64; height:48; } } }";
        let component = "component Button { Rectangle { background:#00000000; ContentShape { points:'20 10 25 10 25 15 20 15'; color:#abcdef; } } }";
        let model = Button::from_sources(source, component).unwrap();
        let pixels = model.content_pixels(160, 120, 1.);
        for y in 0..120 {
            for x in 0..160 {
                let expected = if (20..25).contains(&x) && (10..15).contains(&y) {
                    [0xab, 0xcd, 0xef, 255]
                } else {
                    [0; 4]
                };
                assert_eq!(&pixels[(y * 160 + x) * 4..(y * 160 + x + 1) * 4], &expected);
            }
        }
        let cache = model.raster_cache.borrow();
        let cache = cache.as_ref().unwrap();
        let retained = cache.content.capacity()
            + cache.content_spans.capacity() * std::mem::size_of::<std::ops::Range<usize>>();
        assert!(retained < 1024, "a 25-pixel shape must not retain a full-canvas RGBA layer: {retained}");
    }

    #[test]
    fn clip_support_contains_all_fractional_coverage_samples() {
        for scale in [0.75, 1., 1.25, 2.] {
            for bounds in [
                [-4.3, -2.1, 13.7, 9.2],
                [7.1, 5.6, 0.2, 0.4],
                [3.3, 4.8, 18.1, 12.5],
                [100., 100., 10., 10.],
                [5., 5., 0., 8.],
            ] {
                let [x0, y0, x1, y1] = clip_pixels(bounds, scale, 48, 40);
                for radius in [0., 2.5, 50.] {
                    for y in 0..40 {
                        for x in 0..48 {
                            if x < x0 || x >= x1 || y < y0 || y >= y1 {
                                assert_eq!(round_coverage(x, y, scale, bounds, radius), 0.);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn pooled_sibling_and_nested_clips_preserve_reference_coverage() {
        let source = "component Demo { Frame { width:48; height:40; padding:0; radius:4; background:#10203080; Button { width:48; height:40; } } }";
        let component = "component Button { Rectangle { background:#12345670; ContentClip { x:1.3; y:2.8; width:16.5; height:22.7; radius:3.2; } ContentShape { points:'-3 -2 45 -2 45 38 -3 38'; color:#ff000080; } ContentClip { x:5.4; y:8.2; width:18; height:11.8; radius:2; } ContentShape { points:'0 0 48 0 48 40 0 40'; color:#00ff0080; } ContentClipEnd {} ContentClipEnd {} ContentClip { x:26.2; y:1.4; width:12.6; height:16.3; radius:4; } ContentShape { points:'0 0 48 0 48 40 0 40'; color:#0000ff80; } ContentClipEnd {} ContentClip { x:0; y:0; width:0; height:30; } ContentShape { points:'0 0 48 0 48 40 0 40'; color:#ffffff; } ContentClipEnd {} } }";
        let model = Button::from_sources(source, component).unwrap();
        for scale in [1., 1.25, 2.] {
            for background in [false, true] {
                let reference = model.raster_reference(96, 80, scale, background);
                let actual = model.raster_cached(96, 80, scale, background);
                for (a, b) in reference.chunks_exact(4).zip(actual.chunks_exact(4)) {
                    for k in 0..4 {
                        let premul = |p: &[u8]| if k == 3 { p[3] as f32 } else { p[k] as f32 * p[3] as f32 / 255. };
                        assert!((premul(a) - premul(b)).abs() <= 2., "{a:?} != {b:?}");
                    }
                }
            }
        }
    }
}
