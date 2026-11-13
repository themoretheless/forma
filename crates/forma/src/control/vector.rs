//! The vector display list of a control and painting from it.

use super::*;

impl display_list::DisplayList {
    pub fn build(model: &Button, scale: f32, background: bool) -> Self {
        let mut scratch = text::VectorScratch::prewarm();
        Self::build_hinted(model.control_view(), scale, background, None, &mut scratch)
    }
}

impl Button {
    pub(crate) fn ensure_vector_list(
        &self,
        scale: f32,
        background: bool,
        hints: Option<display_list::ListHints>,
        text_scratch: &mut text::VectorScratch,
    ) {
        let mut cache = self.display_cache.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|c| c.scale != scale || c.background != background)
        {
            *cache = Some(display_list::CachedList {
                scale,
                background,
                list: std::sync::Arc::new(display_list::DisplayList::build_hinted(
                    self.control_view(),
                    scale,
                    background,
                    hints,
                    text_scratch,
                )),
            });
        }
    }

    /// Borrowed view retained for existing Rust callers and WASM serialization.
    pub fn vector_list(
        &self,
        scale: f32,
        background: bool,
    ) -> std::cell::Ref<'_, display_list::DisplayList> {
        let mut scratch = text::VectorScratch::prewarm();
        self.ensure_vector_list(scale, background, None, &mut scratch);
        std::cell::Ref::map(self.display_cache.borrow(), |c| {
            c.as_ref().unwrap().list.as_ref()
        })
    }

    /// Immutable shared geometry identity. Cloning a cached handle does not allocate
    /// or copy commands/edges. Holding the previous handle prevents address reuse,
    /// allowing renderers to detect replacement with `Arc::ptr_eq` without an ABA bug.
    /// Color animations and scroll keep the same snapshot; DPI, background and loading
    /// another source/component replace it. Future geometry setters must invalidate
    /// the display cache instead of mutating a published snapshot.
    pub fn vector_snapshot(
        &self,
        scale: f32,
        background: bool,
    ) -> std::sync::Arc<display_list::DisplayList> {
        let mut scratch = text::VectorScratch::prewarm();
        self.vector_snapshot_hinted(scale, background, None, &mut scratch)
    }

    /// Same snapshot with a capacity head start and a shared flattening buffer. A scene that
    /// asks for hundreds of near-identical control lists uses these to allocate each list at
    /// its final size instead of doubling into it.
    pub(crate) fn vector_snapshot_hinted(
        &self,
        scale: f32,
        background: bool,
        hints: Option<display_list::ListHints>,
        text_scratch: &mut text::VectorScratch,
    ) -> std::sync::Arc<display_list::DisplayList> {
        self.ensure_vector_list(scale, background, hints, text_scratch);
        std::sync::Arc::clone(&self.display_cache.borrow().as_ref().unwrap().list)
    }

    pub(crate) fn cached(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
        sparse: bool,
    ) -> Option<std::cell::RefMut<'_, raster_cache::Cache>> {
        if !raster_cache::valid_raster_size(width, height) || !scale.is_finite() || scale <= 0. {
            return None;
        }
        let geometry = raster_cache::GeometryKey {
            width,
            height,
            scale,
            scroll: [self.scroll_x, self.scroll_y],
            sparse,
        };
        let paint = raster_cache::PaintKey {
            fill: self.fill.color(),
            border: self.border.color(),
            reveal: self.reveal_paint(),
            background,
        };
        let mut stored = self.raster_cache.borrow_mut();
        if stored.as_ref().is_none_or(|cache| cache.key != geometry) {
            *stored = Some(raster_cache::Cache::build(self.control_view(), geometry));
            self.raster_builds
                .set(self.raster_builds.get().wrapping_add(1));
        }
        let cache = stored.as_mut().unwrap();
        if cache.paint != Some(paint) {
            cache.paint(&self.scene, paint);
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

    /// Copy RGBA output into caller-owned storage, retaining its allocation.
    pub fn paint_rgba(
        &self,
        dst: &mut [u8],
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
    ) -> Result<(), &'static str> {
        if (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(dst.len())
        {
            return Err("Invalid RGBA buffer size");
        }
        let cache = self
            .cached(width, height, scale, background, false)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        dst.copy_from_slice(&cache.pixels);
        Ok(())
    }

    /// Composite a leaf without allocating/copying a transparent full canvas.
    /// Root frame clipping and scrollbars are applied once by Runtime.
    pub(crate) fn composite_content(&self, dst: &mut [u8], width: u32, height: u32, scale: f32) {
        debug_assert!(!self.scene.clip && !self.scene.scroll);
        let Some(cache) = self.cached(width, height, scale, false, true) else {
            return;
        };
        let mut pixels = cache.pixels.as_slice();
        for span in &cache.spans {
            let bytes = span.len() * 4;
            for (dst, src) in dst[span.start * 4..span.end * 4]
                .chunks_exact_mut(4)
                .zip(pixels[..bytes].chunks_exact(4))
            {
                raster_cache::over(dst, src);
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
        if !raster_cache::valid_raster_size(rw, rh) {
            rw = rw.min(width);
            rh = rh.min(height);
        }
        let mut cache = self
            .cached(rw, rh, scale, true, false)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        if cache.native_pixels.is_empty() {
            let raster_cache::Cache {
                native_pixels,
                pixels,
                ..
            } = &mut *cache;
            native_pixels.reserve(pixels.len() / 4);
            native_pixels.extend(pixels.chunks_exact(4).map(|p| {
                let a = p[3] as u32;
                if a == 255 {
                    ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32)
                } else if a == 0 {
                    0x111319
                } else {
                    let c = |k: usize, bg: u32| (p[k] as u32 * a + bg * (255 - a) + 127) / 255;
                    (c(0, 17) << 16) | (c(1, 19) << 8) | c(2, 25)
                }
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
    pub(crate) fn paint_extent(&self) -> [f32; 2] {
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
                template::Content::Text { bounds, .. } => {
                    extent[0] = extent[0].max(x + bounds[0] + bounds[2] + 1.);
                    extent[1] = extent[1].max(y + bounds[1] + bounds[3] + 1.);
                }
                template::Content::Shape { points, .. } => {
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
