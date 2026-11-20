//! Multi-control CPU raster cache, dirty-tile incremental compositing, scrollbar/backdrop composition and RGBA/XRGB output.

use super::Runtime;
use crate::display_list::TILE;
use crate::geometry::round_coverage;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct RasterKey {
    width: u32,
    height: u32,
    scale: f32,
    scroll: [f32; 2],
}

#[derive(Clone, Copy, PartialEq)]
struct ControlStamp {
    visual_revision: u32,
    support_px: [u32; 4],
    clip_px: Option<[u32; 4]>,
    scroll: [f32; 2],
    canvas_sets: (u32, u32),
}

pub(super) struct RasterCache {
    key: RasterKey,
    // Low five bits contain 0..16 frame coverage; high bits contain the
    // scroll viewport/scrollbar classification, shared by all controls.
    frame: Vec<u8>,
    pub(super) pixels: Vec<u8>,
    pub(super) native_pixels: Vec<u32>,
    paint: Option<(u32, bool)>,
    stamps: Vec<ControlStamp>,
    area_offsets: Vec<[f32; 2]>,
    modal_open: Vec<bool>,
    dirty_tiles: Vec<bool>,
    dirty_rects: Vec<[u32; 4]>,
}

fn composite_frame_bg_clip_pixel(
    p: &mut [u8],
    frame_byte: u8,
    background: bool,
    scene_bg: [u8; 4],
    scene_clip: bool,
) {
    let cov_int = frame_byte & 31;
    if background {
        if cov_int == 16 && p[3] == 255 {
            // Fully opaque pixel inside unclipped interior: background is completely occluded.
        } else if cov_int == 16 && p[3] == 0 {
            p.copy_from_slice(&scene_bg);
        } else {
            let mut bg = scene_bg;
            if !scene_clip && cov_int < 16 {
                bg[3] = (bg[3] as f32 * (cov_int as f32 / 16.)).round() as u8;
            }
            crate::raster_cache::over(&mut bg, p);
            p.copy_from_slice(&bg);
        }
    }
    if scene_clip && cov_int < 16 {
        if cov_int == 0 {
            p.fill(0);
        } else {
            p[3] = (p[3] as f32 * (cov_int as f32 / 16.)).round() as u8;
            if p[3] == 0 {
                p.fill(0);
            }
        }
    }
}

fn px_from_clip(clip: Option<[f32; 4]>, w: u32, h: u32, s: f32) -> Option<[u32; 4]> {
    clip.map(|[x, y, cw, ch]| {
        let edge = |v: f32, limit: u32| (v * s - 0.5).ceil().clamp(0., limit as f32) as u32;
        [edge(x, w), edge(y, h), edge(x + cw, w), edge(y + ch, h)]
    })
}

fn union_rect_u32(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    if a[0] >= a[2] || a[1] >= a[3] {
        return b;
    }
    if b[0] >= b[2] || b[1] >= b[3] {
        return a;
    }
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn intersect_rect_u32(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = a[2].min(b[2]);
    let y1 = a[3].min(b[3]);
    if x0 >= x1 || y0 >= y1 {
        [0; 4]
    } else {
        [x0, y0, x1, y1]
    }
}

fn encode_xrgb_pixel(src: &[u8]) -> u32 {
    let a = src[3] as u32;
    if a == 255 {
        ((src[0] as u32) << 16) | ((src[1] as u32) << 8) | (src[2] as u32)
    } else if a == 0 {
        0x111319
    } else {
        let rgb = [17u32, 19, 25];
        let c = |k: usize| (src[k] as u32 * a + rgb[k] * (255 - a) + 127) / 255;
        c(0) << 16 | c(1) << 8 | c(2)
    }
}

fn mark_rect_tiles(dirty_tiles: &mut [bool], cols: u32, rows: u32, rect: [u32; 4]) {
    if rect[0] >= rect[2] || rect[1] >= rect[3] {
        return;
    }
    let tx0 = (rect[0] / TILE).min(cols);
    let ty0 = (rect[1] / TILE).min(rows);
    let tx1 = rect[2].div_ceil(TILE).min(cols);
    let ty1 = rect[3].div_ceil(TILE).min(rows);
    for ty in ty0..ty1 {
        let row_base = (ty * cols) as usize;
        for tx in tx0..tx1 {
            dirty_tiles[row_base + tx as usize] = true;
        }
    }
}

fn collect_dirty_rects(
    dirty_tiles: &[bool],
    cols: u32,
    rows: u32,
    w: u32,
    h: u32,
    out: &mut Vec<[u32; 4]>,
) -> u64 {
    out.clear();
    let mut area = 0u64;
    for ty in 0..rows {
        let row_base = (ty * cols) as usize;
        let py0 = ty * TILE;
        let py1 = ((ty + 1) * TILE).min(h);
        let mut tx = 0u32;
        while tx < cols {
            if !dirty_tiles[row_base + tx as usize] {
                tx += 1;
                continue;
            }
            let start = tx;
            while tx < cols && dirty_tiles[row_base + tx as usize] {
                tx += 1;
            }
            let px0 = start * TILE;
            let px1 = (tx * TILE).min(w);
            out.push([px0, py0, px1, py1]);
            area += (px1 - px0) as u64 * (py1 - py0) as u64;
        }
    }
    area
}

impl Runtime {
    pub fn last_dirty_rects(&self) -> Vec<[u32; 4]> {
        self.raster_cache
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |cache| cache.dirty_rects.clone())
    }

    fn control_stamp(&self, index: usize, w: u32, h: u32, s: f32) -> ControlStamp {
        let control = &self.controls[index];
        let mut support_px =
            crate::raster_cache::control_pixel_bounds(control.control_view(), w, h, s);
        if let Some([cx, cy, cw, ch]) = self.canvas_screen_rect(index) {
            let canvas_px = [
                (cx * s - 1.).floor().clamp(0., w as f32) as u32,
                (cy * s - 1.).floor().clamp(0., h as f32) as u32,
                ((cx + cw) * s + 1.).ceil().clamp(0., w as f32) as u32,
                ((cy + ch) * s + 1.).ceil().clamp(0., h as f32) as u32,
            ];
            support_px = union_rect_u32(support_px, canvas_px);
        }
        ControlStamp {
            visual_revision: control.visual_revision(),
            support_px,
            clip_px: px_from_clip(self.screen_clip(index), w, h, s),
            scroll: [control.scroll_x, control.scroll_y],
            canvas_sets: self.canvas_stamp(index),
        }
    }

    /// The scene rectangle control `index` stays inside, when a container below the root clips it.
    pub(super) fn clip(&self, index: usize) -> Option<[f32; 4]> {
        self.clips.get(index).copied().flatten()
    }

    /// Whether the screen point `(x, y)` is inside control `index`'s clip, if it has one.
    pub(super) fn inside_clip(&self, index: usize, x: f32, y: f32) -> bool {
        self.clip(index).is_none_or(|[cx, cy, w, h]| {
            let root = self.root_scroll_of(index);
            let (x, y) = (x + root[0], y + root[1]);
            x >= cx && y >= cy && x < cx + w && y < cy + h
        })
    }

    /// Control `index`'s clip on screen: clips move with the root scroll, not with the areas.
    pub(super) fn screen_clip(&self, index: usize) -> Option<[f32; 4]> {
        let root = self.root_scroll_of(index);
        self.clip(index)
            .map(|[x, y, w, h]| [x - root[0], y - root[1], w, h])
    }

    pub(super) fn raster_cached(
        &self,
        w: u32,
        h: u32,
        s: f32,
        background: bool,
    ) -> Option<std::cell::RefMut<'_, RasterCache>> {
        if w == 0
            || h == 0
            || w > 4096
            || h > 4096
            || w as u64 * h as u64 > 8_388_608
            || !s.is_finite()
            || s <= 0.
        {
            return None;
        }
        let key = RasterKey {
            width: w,
            height: h,
            scale: s,
            scroll: self.scroll,
        };
        let len = (w * h) as usize;
        let cols = w.div_ceil(TILE);
        let rows = h.div_ceil(TILE);
        let max_rects = (rows as usize) * (cols as usize).div_ceil(2) + 1;
        let mut stored = self.raster_cache.borrow_mut();
        let cache = stored.get_or_insert_with(|| RasterCache {
            key,
            frame: Vec::new(),
            pixels: Vec::new(),
            native_pixels: Vec::new(),
            paint: None,
            stamps: Vec::with_capacity(self.controls.len()),
            area_offsets: Vec::with_capacity(self.scroll_areas.len()),
            modal_open: Vec::with_capacity(self.modal_open.len()),
            dirty_tiles: vec![false; (cols * rows) as usize],
            dirty_rects: Vec::with_capacity(max_rects),
        });
        if cache.key != key || cache.frame.len() != len {
            cache.key = key;
            cache.frame.resize(len, 0);
            cache.pixels.resize(len * 4, 0);
            cache.dirty_tiles.resize((cols * rows) as usize, false);
            cache.dirty_rects.clear();
            cache.dirty_rects.reserve(max_rects);
            cache.paint = None;
            cache.native_pixels.clear();
            let v = self.viewport_rect();
            let [cw, ch] = self.content_size();
            let inv_s = 1. / s;
            let sw = self.width();
            let sh = self.height();
            let r = self.scene.radius.min(sw * 0.5).min(sh * 0.5).max(0.);
            let flat_frame = r <= 0. && (sw * s) >= w as f32 && (sh * s) >= h as f32;
            let vert_x0 = (v[0] + v[2] - 5.).max(v[0]);
            let vert_y0 = v[1] + self.scroll[1] / ch * v[3];
            let vert_y1 = v[1] + (self.scroll[1] + v[3]) / ch * v[3];
            let horz_y0 = (v[1] + v[3] - 5.).max(v[1]);
            let horz_x0 = v[0] + self.scroll[0] / cw * v[2];
            let horz_x1 = v[0] + (self.scroll[0] + v[2]) / cw * v[2];
            if flat_frame && !self.scene.scroll {
                cache.frame.fill(16 | (1 << 5));
            } else {
                for y in 0..h {
                    let py = (y as f32 + 0.5) * inv_s;
                    let row = (y * w) as usize;
                    for x in 0..w {
                        let px = (x as f32 + 0.5) * inv_s;
                        let mut viewport = 1;
                        if self.scene.scroll {
                            if px < v[0] || py < v[1] || px >= v[0] + v[2] || py >= v[1] + v[3] {
                                viewport = 0;
                            } else {
                                let vertical =
                                    ch > v[3] && px >= vert_x0 && py >= vert_y0 && py < vert_y1;
                                let horizontal =
                                    cw > v[2] && py >= horz_y0 && px >= horz_x0 && px < horz_x1;
                                if vertical || horizontal {
                                    viewport = 2;
                                }
                            }
                        }
                        let coverage = if flat_frame {
                            1.
                        } else {
                            round_coverage(x, y, s, [0., 0., sw, sh], self.scene.radius)
                        };
                        cache.frame[row + x as usize] = (coverage * 16.) as u8 | viewport << 5;
                    }
                }
            }
        }
        let paint = (self.revision, background);
        if cache.paint != Some(paint) {
            let can_incremental = cache
                .paint
                .is_some_and(|(_, prev_bg)| prev_bg == background)
                && cache.stamps.len() == self.controls.len()
                && cache.area_offsets.len() == self.scroll_areas.len()
                && cache.modal_open == self.modal_open
                && cache.dirty_tiles.len() == (cols * rows) as usize;

            let mut used_partial = false;
            if can_incremental {
                cache.dirty_tiles.fill(false);
                for (i, prev) in cache.stamps.iter_mut().enumerate() {
                    let next = self.control_stamp(i, w, h, s);
                    if next != *prev {
                        let old_clipped = prev
                            .clip_px
                            .map_or(prev.support_px, |c| intersect_rect_u32(prev.support_px, c));
                        let new_clipped = next
                            .clip_px
                            .map_or(next.support_px, |c| intersect_rect_u32(next.support_px, c));
                        mark_rect_tiles(&mut cache.dirty_tiles, cols, rows, old_clipped);
                        mark_rect_tiles(&mut cache.dirty_tiles, cols, rows, new_clipped);
                        *prev = next;
                    }
                }
                for (a, prev_offset) in cache.area_offsets.iter_mut().enumerate() {
                    let offset = self.scroll_areas[a].offset;
                    if offset != *prev_offset {
                        *prev_offset = offset;
                        let [vx, vy, vw, vh] = self.area_screen_viewport(a);
                        let area_px = [
                            (vx * s - 1.).floor().clamp(0., w as f32) as u32,
                            (vy * s - 1.).floor().clamp(0., h as f32) as u32,
                            ((vx + vw) * s + 1.).ceil().clamp(0., w as f32) as u32,
                            ((vy + vh) * s + 1.).ceil().clamp(0., h as f32) as u32,
                        ];
                        mark_rect_tiles(&mut cache.dirty_tiles, cols, rows, area_px);
                    }
                }
                let dirty_area = collect_dirty_rects(
                    &cache.dirty_tiles,
                    cols,
                    rows,
                    w,
                    h,
                    &mut cache.dirty_rects,
                );
                if dirty_area == 0 {
                    cache.paint = Some(paint);
                    used_partial = true;
                } else if dirty_area < (len as u64 * 3) / 4 {
                    used_partial = true;
                    let update_native = cache.native_pixels.len() == len;
                    let edge_px =
                        |v: f32, limit: u32| ((v * s - 0.5).ceil().max(0.) as u32).min(limit);
                    for rect_idx in 0..cache.dirty_rects.len() {
                        let rect = cache.dirty_rects[rect_idx];
                        let [rx0, ry0, rx1, ry1] = rect;
                        for y in ry0..ry1 {
                            let row = (y * w) as usize;
                            cache.pixels[(row + rx0 as usize) * 4..(row + rx1 as usize) * 4]
                                .fill(0);
                        }
                        for i in 0..self.controls.len() {
                            if self.modal_of[i].is_none() {
                                let stamp = cache.stamps[i];
                                let ctrl_rect = stamp.clip_px.map_or(stamp.support_px, |c| {
                                    intersect_rect_u32(stamp.support_px, c)
                                });
                                let inter = intersect_rect_u32(ctrl_rect, rect);
                                if inter[0] < inter[2] && inter[1] < inter[3] {
                                    self.controls[i].composite_content_in_window(
                                        &mut cache.pixels,
                                        w,
                                        h,
                                        s,
                                        Some(inter.map(|v| v as usize)),
                                    );
                                    self.raster_canvas_window(
                                        i,
                                        &mut cache.pixels,
                                        w,
                                        h,
                                        s,
                                        Some(inter),
                                    );
                                }
                            }
                        }
                        if !self.scroll_areas.is_empty() {
                            for [x, y, tw, th] in self.area_thumbs() {
                                let (x, y) = (x - self.scroll[0], y - self.scroll[1]);
                                let py0 = edge_px(y, h).max(ry0);
                                let py1 = edge_px(y + th, h).min(ry1);
                                let px0 = edge_px(x, w).max(rx0);
                                let px1 = edge_px(x + tw, w).min(rx1);
                                for py in py0..py1 {
                                    for px in px0..px1 {
                                        let at = ((py * w + px) * 4) as usize;
                                        cache.pixels[at..at + 4]
                                            .copy_from_slice(&super::scroll_areas::AREA_THUMB);
                                    }
                                }
                            }
                        }
                        if self.scene.scroll {
                            for y in ry0..ry1 {
                                let row = (y * w) as usize;
                                for x in rx0..rx1 {
                                    let idx = row + x as usize;
                                    match cache.frame[idx] >> 5 {
                                        0 => cache.pixels[idx * 4..idx * 4 + 4].fill(0),
                                        2 => cache.pixels[idx * 4..idx * 4 + 4]
                                            .copy_from_slice(&[110, 130, 170, 255]),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        for (m, modal) in self.scene.modals.iter().enumerate() {
                            if !self.modal_open[m] {
                                continue;
                            }
                            if modal.backdrop[3] > 0 {
                                for y in ry0..ry1 {
                                    let row = (y * w) as usize;
                                    for x in rx0..rx1 {
                                        let at = (row + x as usize) * 4;
                                        crate::raster_cache::over(
                                            &mut cache.pixels[at..at + 4],
                                            &modal.backdrop,
                                        );
                                    }
                                }
                            }
                            for index in modal.controls.clone() {
                                let stamp = cache.stamps[index];
                                let ctrl_rect = stamp.clip_px.map_or(stamp.support_px, |c| {
                                    intersect_rect_u32(stamp.support_px, c)
                                });
                                let inter = intersect_rect_u32(ctrl_rect, rect);
                                if inter[0] < inter[2] && inter[1] < inter[3] {
                                    self.controls[index].composite_content_in_window(
                                        &mut cache.pixels,
                                        w,
                                        h,
                                        s,
                                        Some(inter.map(|v| v as usize)),
                                    );
                                    self.raster_canvas_window(
                                        index,
                                        &mut cache.pixels,
                                        w,
                                        h,
                                        s,
                                        Some(inter),
                                    );
                                }
                            }
                        }
                        if background || self.scene.clip {
                            for y in ry0..ry1 {
                                let row = (y * w) as usize;
                                for x in rx0..rx1 {
                                    let idx = row + x as usize;
                                    let fb = cache.frame[idx];
                                    composite_frame_bg_clip_pixel(
                                        &mut cache.pixels[idx * 4..idx * 4 + 4],
                                        fb,
                                        background,
                                        self.scene.background,
                                        self.scene.clip,
                                    );
                                }
                            }
                        }
                        if update_native {
                            for y in ry0..ry1 {
                                let row = (y * w) as usize;
                                for x in rx0..rx1 {
                                    let idx = row + x as usize;
                                    cache.native_pixels[idx] =
                                        encode_xrgb_pixel(&cache.pixels[idx * 4..idx * 4 + 4]);
                                }
                            }
                        }
                    }
                    if !update_native {
                        cache.native_pixels.clear();
                    }
                    cache.paint = Some(paint);
                }
            }

            if !used_partial {
                cache.pixels.fill(0);
                for (i, control) in self.controls.iter().enumerate() {
                    if self.modal_of[i].is_none() {
                        control.composite_content_clipped(
                            &mut cache.pixels,
                            w,
                            h,
                            s,
                            self.screen_clip(i),
                        );
                        self.raster_canvas(i, &mut cache.pixels, w, h, s);
                    }
                }
                for [x, y, tw, th] in self.area_thumbs() {
                    let (x, y) = (x - self.scroll[0], y - self.scroll[1]);
                    let edge =
                        |v: f32, limit: u32| ((v * s - 0.5).ceil().max(0.) as u32).min(limit);
                    for py in edge(y, h)..edge(y + th, h) {
                        for px in edge(x, w)..edge(x + tw, w) {
                            let at = ((py * w + px) * 4) as usize;
                            cache.pixels[at..at + 4]
                                .copy_from_slice(&super::scroll_areas::AREA_THUMB);
                        }
                    }
                }
                if self.scene.scroll {
                    for (p, &frame) in cache.pixels.chunks_exact_mut(4).zip(&cache.frame) {
                        match frame >> 5 {
                            0 => p.fill(0),
                            2 => p.copy_from_slice(&[110, 130, 170, 255]),
                            _ => {}
                        }
                    }
                }
                // Layers sit above the scroll viewport and its bars; the frame's
                // rounded coverage still trims every layer below.
                for (m, modal) in self.scene.modals.iter().enumerate() {
                    if !self.modal_open[m] {
                        continue;
                    }
                    if modal.backdrop[3] > 0 {
                        for p in cache.pixels.chunks_exact_mut(4) {
                            crate::raster_cache::over(p, &modal.backdrop);
                        }
                    }
                    for index in modal.controls.clone() {
                        self.controls[index].composite_content_clipped(
                            &mut cache.pixels,
                            w,
                            h,
                            s,
                            self.screen_clip(index),
                        );
                        self.raster_canvas(index, &mut cache.pixels, w, h, s);
                    }
                }
                if background || self.scene.clip {
                    for (p, &frame) in cache.pixels.chunks_exact_mut(4).zip(&cache.frame) {
                        composite_frame_bg_clip_pixel(
                            p,
                            frame,
                            background,
                            self.scene.background,
                            self.scene.clip,
                        );
                    }
                }
                cache.stamps.clear();
                for i in 0..self.controls.len() {
                    cache.stamps.push(self.control_stamp(i, w, h, s));
                }
                cache.area_offsets.clear();
                cache
                    .area_offsets
                    .extend(self.scroll_areas.iter().map(|a| a.offset));
                cache.modal_open.clear();
                cache.modal_open.extend_from_slice(&self.modal_open);
                cache.dirty_rects.clear();
                cache.dirty_rects.push([0, 0, w, h]);
                cache.paint = Some(paint);
                cache.native_pixels.clear();
            }
        }
        Some(std::cell::RefMut::map(stored, |cache| {
            cache.as_mut().unwrap()
        }))
    }

    pub(super) fn raster(&self, w: u32, h: u32, s: f32, background: bool) -> Vec<u8> {
        if self.single_leaf() {
            return if background {
                self.controls[0].pixels(w, h, s)
            } else {
                self.controls[0].content_pixels(w, h, s)
            };
        }
        self.raster_cached(w, h, s, background)
            .map_or_else(Vec::new, |cache| cache.pixels.clone())
    }

    /// Paint RGBA into caller-owned storage without allocating an output Vec.
    /// Invalid dimensions/DPI leave the destination unchanged.
    pub fn paint_rgba(
        &self,
        out: &mut [u8],
        w: u32,
        h: u32,
        s: f32,
        background: bool,
    ) -> Result<(), &'static str> {
        if self.single_leaf() {
            return self.controls[0].paint_rgba(out, w, h, s, background);
        }
        if (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(out.len())
        {
            return Err("Invalid RGBA buffer size");
        }
        let cache = self
            .raster_cached(w, h, s, background)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        out.copy_from_slice(&cache.pixels);
        Ok(())
    }

    pub fn paint_native(
        &self,
        out: &mut [u32],
        w: u32,
        h: u32,
        s: f32,
    ) -> Result<(), &'static str> {
        if self.single_leaf() {
            return self.controls[0].paint_native(out, w, h, s);
        }
        if (w as usize).checked_mul(h as usize) != Some(out.len()) {
            return Err("Invalid native buffer size");
        }
        let mut cache = self
            .raster_cached(w, h, s, true)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        if cache.native_pixels.is_empty() {
            let RasterCache {
                native_pixels,
                pixels,
                ..
            } = &mut *cache;
            native_pixels.reserve(pixels.len() / 4);
            native_pixels.extend(pixels.chunks_exact(4).map(|src| {
                let a = src[3] as u32;
                if a == 255 {
                    ((src[0] as u32) << 16) | ((src[1] as u32) << 8) | (src[2] as u32)
                } else if a == 0 {
                    0x111319
                } else {
                    let rgb = [17u32, 19, 25];
                    let c = |k: usize| (src[k] as u32 * a + rgb[k] * (255 - a) + 127) / 255;
                    c(0) << 16 | c(1) << 8 | c(2)
                }
            }));
        }
        out.copy_from_slice(&cache.native_pixels);
        Ok(())
    }
}
