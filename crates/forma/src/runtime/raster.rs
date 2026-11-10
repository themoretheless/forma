//! Multi-control CPU raster cache, scrollbar/backdrop composition and RGBA/XRGB output.

use super::Runtime;
use crate::geometry::round_coverage;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct RasterKey {
    width: u32,
    height: u32,
    scale: f32,
    scroll: [f32; 2],
}

pub(super) struct RasterCache {
    key: RasterKey,
    // Low five bits contain 0..16 frame coverage; high bits contain the
    // scroll viewport/scrollbar classification, shared by all controls.
    frame: Vec<u8>,
    pub(super) pixels: Vec<u8>,
    pub(super) native_pixels: Vec<u32>,
    paint: Option<(u32, bool)>,
}

impl Runtime {
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
        let mut stored = self.raster_cache.borrow_mut();
        let cache = stored.get_or_insert_with(|| RasterCache {
            key,
            frame: Vec::new(),
            pixels: Vec::new(),
            native_pixels: Vec::new(),
            paint: None,
        });
        if cache.key != key || cache.frame.len() != len {
            cache.key = key;
            cache.frame.resize(len, 0);
            cache.pixels.resize(len * 4, 0);
            cache.paint = None;
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
            cache.pixels.fill(0);
            for (i, control) in self.controls.iter().enumerate() {
                if self.modal_of[i].is_none() {
                    control.composite_content(&mut cache.pixels, w, h, s);
                    self.raster_canvas(i, &mut cache.pixels, w, h, s);
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
                    self.controls[index].composite_content(&mut cache.pixels, w, h, s);
                    self.raster_canvas(index, &mut cache.pixels, w, h, s);
                }
            }
            if background || self.scene.clip {
                for (p, &frame) in cache.pixels.chunks_exact_mut(4).zip(&cache.frame) {
                    let cov_int = frame & 31;
                    if background {
                        if cov_int == 16 && p[3] == 255 {
                            // Fully opaque pixel inside unclipped interior: background is completely occluded.
                        } else if cov_int == 16 && p[3] == 0 {
                            p.copy_from_slice(&self.scene.background);
                        } else {
                            let mut bg = self.scene.background;
                            if !self.scene.clip && cov_int < 16 {
                                bg[3] = (bg[3] as f32 * (cov_int as f32 / 16.)).round() as u8;
                            }
                            crate::raster_cache::over(&mut bg, p);
                            p.copy_from_slice(&bg);
                        }
                    }
                    if self.scene.clip && cov_int < 16 {
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
            }
            cache.paint = Some(paint);
            cache.native_pixels.clear();
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
