//! Single-entry, per-instance raster cache. A new model owns a new cache.
//! Geometry/content depend on viewport, DPI and scroll, not animated colors.
use crate::{round_coverage, shape, template::Content, text, Button};

#[derive(Clone, Copy, PartialEq)]
struct GeometryKey {
    width: u32,
    height: u32,
    scale: f32,
    scroll: [f32; 2],
}
#[derive(Clone, Copy, PartialEq)]
struct PaintKey {
    fill: [u8; 4],
    border: [u8; 4],
    background: bool,
}

pub(crate) struct Cache {
    key: GeometryKey,
    // Four original coverage samples: fill_count * 5 + border_count.
    coverage: Vec<u8>,
    frame: Vec<u8>,
    viewport: Vec<u8>, // 0 clipped; 1 content; 2 scrollbar
    content: Vec<u8>,
    paint: Option<PaintKey>,
    pixels: Vec<u8>,
    native_pixels: Option<Vec<u32>>,
}

/// Straight-alpha source-over, same rounding as the outline renderer.
fn over(dst: &mut [u8], src: &[u8]) {
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
            frame: vec![0; len],
            viewport: vec![1; len],
            content: vec![0; len * 4],
            paint: None,
            pixels: vec![0; len * 4],
            native_pixels: None,
        };
        let mut b = model.scene.button.clone();
        b.x -= model.scroll_x;
        b.y -= model.scroll_y;
        let r = b.radius.min(b.width / 2.).min(b.height / 2.);
        let bw = model.template.border.as_ref().map_or(0., |b| b.width);
        // No geometry evaluation in the empty part of the canvas.
        let x0 = (b.x * scale).floor().clamp(0., width as f32) as u32;
        let y0 = (b.y * scale).floor().clamp(0., height as f32) as u32;
        let x1 = ((b.x + b.width) * scale).ceil().clamp(0., width as f32) as u32;
        let y1 = ((b.y + b.height) * scale).ceil().clamp(0., height as f32) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let mut fill = 0;
                let mut border = 0;
                for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let dx =
                        ((x as f32 + sx) / scale - b.x - b.width / 2.).abs() - (b.width / 2. - r);
                    let dy =
                        ((y as f32 + sy) / scale - b.y - b.height / 2.).abs() - (b.height / 2. - r);
                    let distance = dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - r;
                    if distance <= 0. {
                        fill += 1;
                        if bw > 0. && distance >= -bw {
                            border += 1;
                        }
                    }
                }
                cache.coverage[(y * width + x) as usize] = fill * 5 + border;
            }
        }
        let v = model.viewport();
        let [vx, vy, vw, vh] = [v[0], v[1], v[2], v[3]];
        for y in 0..height {
            for x in 0..width {
                let i = (y * width + x) as usize;
                cache.frame[i] = (round_coverage(
                    x,
                    y,
                    scale,
                    [0., 0., model.width(), model.height()],
                    model.scene.radius,
                ) * 16.) as u8;
                if model.scene.scroll {
                    let px = (x as f32 + 0.5) / scale;
                    let py = (y as f32 + 0.5) / scale;
                    if px < vx || py < vy || px >= vx + vw || py >= vy + vh {
                        cache.viewport[i] = 0;
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
                            cache.viewport[i] = 2;
                        }
                    }
                }
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
                    b.x + r * 0.3,
                    b.y + r * 0.3,
                    b.width - r * 0.6,
                    b.height - r * 0.6,
                ],
                t.color,
            );
        }
        let mut groups = Vec::new();
        for c in &model.template.content {
            match c {
                Content::Text { bounds, text: t } => text::draw_text(
                    &mut cache.content,
                    width,
                    height,
                    scale,
                    &t.text,
                    t.font_size,
                    [b.x + bounds[0], b.y + bounds[1], bounds[2], bounds[3]],
                    t.color,
                ),
                Content::Shape { points, color } => shape::draw(
                    &mut cache.content,
                    width,
                    height,
                    scale,
                    points,
                    [b.x, b.y],
                    *color,
                ),
                Content::Clip { bounds, radius } => groups.push((
                    std::mem::replace(&mut cache.content, vec![0; len * 4]),
                    [bounds[0] + b.x, bounds[1] + b.y, bounds[2], bounds[3]],
                    *radius,
                )),
                Content::ClipEnd => {
                    if let Some((mut parent, bounds, radius)) = groups.pop() {
                        for y in 0..height {
                            for x in 0..width {
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
                        cache.content = parent;
                    }
                }
            }
        }
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
        for (i, dst) in self.pixels.chunks_exact_mut(4).enumerate() {
            dst.copy_from_slice(&palette[self.coverage[i] as usize]);
            over(dst, &self.content[i * 4..i * 4 + 4]);
            match self.viewport[i] {
                0 => dst.fill(0),
                2 => dst.copy_from_slice(&[110, 130, 170, 255]),
                _ => {}
            }
            let coverage = self.frame[i] as f32 / 16.;
            if key.background && dst[3] != 255 {
                let mut bg = model.scene.background;
                if !model.scene.clip {
                    bg[3] = (bg[3] as f32 * coverage).round() as u8;
                }
                over(&mut bg, dst);
                dst.copy_from_slice(&bg);
            }
            if model.scene.clip && self.frame[i] != 16 {
                dst[3] = (dst[3] as f32 * coverage).round() as u8;
                if dst[3] == 0 {
                    dst.fill(0);
                }
            }
        }
        self.paint = Some(key);
        self.native_pixels = None;
    }
}

impl Button {
    fn cached(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
    ) -> Option<std::cell::RefMut<'_, Cache>> {
        if !valid_raster_size(width, height) || !scale.is_finite() || scale <= 0. {
            return None;
        }
        let geometry = GeometryKey {
            width,
            height,
            scale,
            scroll: [self.scroll_x, self.scroll_y],
        };
        let paint = PaintKey {
            fill: self.fill.color(),
            border: self.border.color(),
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
        self.cached(width, height, scale, background)
            .map_or_else(Vec::new, |cache| cache.pixels.clone())
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
            .cached(rw, rh, scale, true)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        if cache.native_pixels.is_none() {
            cache.native_pixels = Some(
                cache
                    .pixels
                    .chunks_exact(4)
                    .map(|p| {
                        let a = p[3] as u32;
                        let c = |k: usize, bg: u32| (p[k] as u32 * a + bg * (255 - a) + 127) / 255;
                        (c(0, 17) << 16) | (c(1, 19) << 8) | c(2, 25)
                    })
                    .collect(),
            );
        }
        let pixels = cache.native_pixels.as_ref().unwrap();
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
