//! Backend-independent vector commands. No raster buffers, DOM or GPU objects.
//! Coordinates remain logical; glyph curves are flattened to 0.15 physical px.
use crate::{template::Content, text, Button};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

pub const TILE: u32 = 32;
// Five vec4 records: kind/rule/edge range, bounds, color, radius/stroke, reserved.
pub const STRIDE: usize = 20;
pub struct DisplayList {
    pub commands: Vec<f32>,
    pub edges: Vec<f32>,
}

/// Reusable CPU storage for tile indexing. One contiguous output allocation and
/// one command-range allocation replace thousands of independently grown bins.
/// Kept separate from DisplayList: window resize does not invalidate geometry.
#[derive(Default)]
pub struct TileScratch {
    data: Vec<u32>,
    ranges: Vec<[u32; 4]>,
    clips: Vec<ClipGroup>,
}

struct ClipGroup {
    // Physical support bounds, including one pixel of conservative AA margin.
    bounds: [f32; 4],
    push_index: usize,
    // Conservative tile union of visible descendants, not the entire viewport.
    descendants: [u32; 4],
}

impl TileScratch {
    /// Retained CPU capacity, not GPU buffer size or allocator overhead.
    pub fn capacity_bytes(&self) -> usize {
        self.data.capacity() * std::mem::size_of::<u32>()
            + self.ranges.capacity() * std::mem::size_of::<[u32; 4]>()
            + self.clips.capacity() * std::mem::size_of::<ClipGroup>()
    }
}

fn intersect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

fn tile_range(bounds: [f32; 4]) -> [u32; 4] {
    if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
        return [0; 4];
    }
    [
        (bounds[0] / TILE as f32).floor() as u32,
        (bounds[1] / TILE as f32).floor() as u32,
        (bounds[2] / TILE as f32).ceil() as u32,
        (bounds[3] / TILE as f32).ceil() as u32,
    ]
}

fn union_tiles(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
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

pub(crate) struct CachedList {
    scale: f32,
    scroll: [f32; 2],
    background: bool,
    list: Arc<DisplayList>,
}
impl DisplayList {
    pub(crate) fn command(&mut self, kind: f32, bounds: [f32; 4], color: [u8; 4], radius: f32, stroke: f32) {
        self.commands.extend([kind, 0., 0., 0.]);
        self.commands.extend(bounds);
        self.commands.extend(color.map(|v| v as f32 / 255.));
        self.commands
            .extend([radius, stroke, 0., 0., 0., 0., 0., 0.]);
    }
    pub(crate) fn push(&mut self, bounds: [f32; 4], radius: f32) {
        self.command(2., bounds, [0; 4], radius, 0.);
    }
    pub(crate) fn pop(&mut self) {
        self.command(3., [0.; 4], [0; 4], 0., 0.);
    }
    fn path(&mut self, edges: impl IntoIterator<Item = [f32; 4]>, color: [u8; 4], nonzero: bool) {
        if color[3] == 0 {
            return;
        }
        let edges = edges.into_iter();
        let first_edge = self.edges.len();
        self.edges.reserve(edges.size_hint().0.saturating_mul(4));
        let mut bb = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for e in edges {
            bb[0] = bb[0].min(e[0]).min(e[2]);
            bb[1] = bb[1].min(e[1]).min(e[3]);
            bb[2] = bb[2].max(e[0]).max(e[2]);
            bb[3] = bb[3].max(e[1]).max(e[3]);
            self.edges.extend(e);
        }
        if self.edges.len() == first_edge {
            return;
        }
        let start = self.commands.len();
        self.command(
            1.,
            [bb[0], bb[1], bb[2] - bb[0], bb[3] - bb[1]],
            color,
            0.,
            0.,
        );
        self.commands[start + 1] = if nonzero { 1. } else { 0. };
        self.commands[start + 2] = (first_edge / 4) as f32;
        self.commands[start + 3] = ((self.edges.len() - first_edge) / 4) as f32;
    }
    fn text(&mut self, t: &crate::template::Text, bounds: [f32; 4], scale: f32, scratch: &mut text::VectorScratch) {
        if t.color[3] == 0 {
            return;
        }
        self.push(bounds, 0.);
        text::vector_glyphs(&t.text, t.font_size, bounds, scale, scratch, |glyph| {
            self.path(glyph, t.color, true);
        });
        self.pop();
    }
    pub fn build(model: &Button, scale: f32, background: bool) -> Self {
        let mut out = Self {
            commands: Vec::new(),
            edges: Vec::new(),
        };
        let mut text_scratch = text::VectorScratch::default();
        let frame = [0., 0., model.width(), model.height()];
        if model.scene.clip {
            out.push(frame, model.scene.radius);
        }
        if background {
            out.command(
                0.,
                frame,
                model.scene.background,
                if model.scene.clip {
                    0.
                } else {
                    model.scene.radius
                },
                0.,
            );
        }
        let v = model.viewport_rect();
        if model.scene.scroll {
            out.command(2., [v[0], v[1], v[2], v[3]], [0; 4], 0., 1.);
        }
        let b = &model.scene.button;
        let x = b.x - model.scroll_x;
        let y = b.y - model.scroll_y;
        let r = b.radius.min(b.width / 2.).min(b.height / 2.);
        out.command(
            4.,
            [x, y, b.width, b.height],
            [0; 4],
            r,
            model.template.border.as_ref().map_or(0., |b| b.width),
        );
        let offset=out.commands.len()-STRIDE;
        out.commands[offset+15]=1.; // Independent fill/border paint slots.
        if let Some(t) = &model.template.text {
            out.text(
                t,
                [
                    x + r * 0.3,
                    y + r * 0.3,
                    b.width - r * 0.6,
                    b.height - r * 0.6,
                ],
                scale,
                &mut text_scratch,
            );
        }
        for c in &model.template.content {
            match c {
                Content::Text { bounds, text } => out.text(
                    text,
                    [x + bounds[0], y + bounds[1], bounds[2], bounds[3]],
                    scale,
                    &mut text_scratch,
                ),
                Content::Shape { points, color } => out.path(
                    (0..points.len())
                        .map(|i| {
                            let a = points[i];
                            let b = points[(i + 1) % points.len()];
                            [x + a[0], y + a[1], x + b[0], y + b[1]]
                        }),
                    *color,
                    false,
                ),
                Content::Clip { bounds, radius } => out.push(
                    [x + bounds[0], y + bounds[1], bounds[2], bounds[3]],
                    *radius,
                ),
                Content::ClipEnd => out.pop(),
            }
        }
        if let Some(reveal) = &model.template.reveal {
            out.command(6., model.reveal_bounds(), [0; 4], reveal.target_radius.unwrap_or(r), reveal.width);
            let offset = out.commands.len() - STRIDE;
            out.commands[offset + 14] = 2.; // Reveal data + color, after fill/border.
        }
        if model.scene.scroll {
            if b.height > v[3] && v[3] > 0. {
                out.command(
                    5.,
                    [
                        (v[0] + v[2] - 5.).max(v[0]),
                        v[1] + model.scroll_y / b.height * v[3],
                        5f32.min(v[2]),
                        v[3] * v[3] / b.height,
                    ],
                    [110, 130, 170, 255],
                    0.,
                    0.,
                );
            }
            if b.width > v[2] && v[2] > 0. {
                out.command(
                    5.,
                    [
                        v[0] + model.scroll_x / b.width * v[2],
                        (v[1] + v[3] - 5.).max(v[1]),
                        v[2] * v[2] / b.width,
                        5f32.min(v[3]),
                    ],
                    [110, 130, 170, 255],
                    0.,
                    0.,
                );
            }
            out.pop();
        }
        if model.scene.clip {
            out.pop();
        }
        out
    }
    /// Stable painter order within each tile, including balanced clip groups.
    pub fn tiles(&self, width: u32, height: u32, scale: f32) -> Vec<u32> {
        let mut scratch = TileScratch::default();
        self.tiles_into(width, height, scale, &mut scratch);
        scratch.data
    }

    /// Fill retained scratch storage using count -> prefix sum -> ordered fill.
    /// Clip pushes/pops share the conservative tile union of their visible
    /// descendants. Fully invisible/empty groups disappear together; descendants
    /// are restricted by every ancestor's physical support bounds. AA margins are
    /// applied BEFORE intersections so even overlapping fringe coverage survives.
    /// At fixed geometry/DPI, only ceil(width/TILE), ceil(height/TILE) matter.
    pub fn tiles_into<'a>(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        scratch: &'a mut TileScratch,
    ) -> &'a [u32] {
        let cols = width.div_ceil(TILE);
        let rows = height.div_ceil(TILE);
        let tile_count = (cols * rows) as usize;
        scratch.data.resize(tile_count * 2, 0);
        scratch.data.fill(0);
        scratch.ranges.clear();
        scratch.clips.clear();
        // Use complete tiles, not the partial last tile's framebuffer edge:
        // renderer caches can safely key this list by tile-grid dimensions.
        let viewport = [0., 0., (cols * TILE) as f32, (rows * TILE) as f32];
        for c in self.commands.chunks_exact(STRIDE) {
            if c[0] == 3. {
                // Display lists built from validated templates have balanced
                // groups. Never insert an unmatched pop into GPU tile storage.
                let Some(group) = scratch.clips.pop() else {
                    scratch.ranges.push([0; 4]);
                    continue;
                };
                scratch.ranges[group.push_index] = group.descendants;
                scratch.ranges.push(group.descendants);
                if let Some(parent) = scratch.clips.last_mut() {
                    parent.descendants = union_tiles(parent.descendants, group.descendants);
                }
                continue;
            }
            let parent = scratch.clips.last().map_or(viewport, |group| group.bounds);
            let support = [
                c[4] * scale - 1.,
                c[5] * scale - 1.,
                (c[4] + c[6]) * scale + 1.,
                (c[5] + c[7]) * scale + 1.,
            ];
            let bounds = intersect(support, parent);
            if c[0] == 2. {
                scratch.clips.push(ClipGroup {
                    bounds: if c[6] <= 0. || c[7] <= 0. {
                        [0.; 4]
                    } else {
                        bounds
                    },
                    push_index: scratch.ranges.len(),
                    descendants: [0; 4],
                });
                scratch.ranges.push([0; 4]);
            } else {
                let range = tile_range(bounds);
                scratch.ranges.push(range);
                if let Some(parent) = scratch.clips.last_mut() {
                    parent.descendants = union_tiles(parent.descendants, range);
                }
            }
        }
        debug_assert!(scratch.clips.is_empty(), "Unclosed display-list clip group");
        for &[x0, y0, x1, y1] in &scratch.ranges {
            for y in y0..y1 {
                for x in x0..x1 {
                    scratch.data[(y * cols + x) as usize * 2 + 1] += 1;
                }
            }
        }
        let mut length = tile_count * 2;
        for pair in scratch.data.chunks_exact_mut(2) {
            let count = pair[1] as usize;
            pair[0] = length as u32;
            // Reuse the final count field as the fill cursor, avoiding another
            // per-tile scratch array. It reaches the same count after filling.
            pair[1] = 0;
            length += count;
        }
        scratch.data.resize(length, 0);
        for (id, &[x0, y0, x1, y1]) in scratch.ranges.iter().enumerate() {
            for y in y0..y1 {
                for x in x0..x1 {
                    let header = (y * cols + x) as usize * 2;
                    let offset = (scratch.data[header] + scratch.data[header + 1]) as usize;
                    scratch.data[offset] = id as u32;
                    scratch.data[header + 1] += 1;
                }
            }
        }
        &scratch.data
    }
}
impl Button {
    fn ensure_vector_list(&self, scale: f32, background: bool) {
        let mut cache = self.display_cache.borrow_mut();
        let scroll = [self.scroll_x, self.scroll_y];
        if cache
            .as_ref()
            .is_none_or(|c| c.scale != scale || c.scroll != scroll || c.background != background)
        {
            *cache = Some(CachedList {
                scale,
                scroll,
                background,
                list: Arc::new(DisplayList::build(self, scale, background)),
            });
        }
    }

    /// Borrowed view retained for existing Rust callers and WASM serialization.
    pub fn vector_list(&self, scale: f32, background: bool) -> std::cell::Ref<'_, DisplayList> {
        self.ensure_vector_list(scale, background);
        std::cell::Ref::map(self.display_cache.borrow(), |c| {
            c.as_ref().unwrap().list.as_ref()
        })
    }

    /// Immutable shared geometry identity. Cloning a cached handle does not allocate
    /// or copy commands/edges. Holding the previous handle prevents address reuse,
    /// allowing renderers to detect replacement with `Arc::ptr_eq` without an ABA bug.
    /// Color animations keep the same snapshot; DPI, background, scroll and loading
    /// another source/component replace it. Future geometry setters must invalidate
    /// the display cache instead of mutating a published snapshot.
    pub fn vector_snapshot(&self, scale: f32, background: bool) -> Arc<DisplayList> {
        self.ensure_vector_list(scale, background);
        Arc::clone(&self.display_cache.borrow().as_ref().unwrap().list)
    }
}
#[wasm_bindgen]
impl Button {
    pub fn gpu_commands(&self, scale: f32, background: bool) -> Vec<f32> {
        self.vector_list(scale, background).commands.clone()
    }
    pub fn gpu_edges(&self, scale: f32, background: bool) -> Vec<f32> {
        self.vector_list(scale, background).edges.clone()
    }
    pub fn gpu_tiles(&self, width: u32, height: u32, scale: f32, background: bool) -> Vec<u32> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || !scale.is_finite()
            || scale <= 0.
        {
            return vec![];
        }
        self.vector_list(scale, background)
            .tiles(width, height, scale)
    }
    // viewport/scale/opaque output; animated fill and border. Geometry is static.
    pub fn gpu_params(&self, width: u32, height: u32, scale: f32, opaque: bool) -> Vec<f32> {
        let mut params = Vec::with_capacity(12);
        self.gpu_params_into(width, height, scale, opaque, &mut params);
        params
    }
}

/// The renderer consumes only immutable geometry and paint slots, never controls
/// or interaction state. Both the legacy leaf and the scene runtime implement it.
pub trait RenderScene {
    /// Called after successful GPU submission. External scene types may retain
    /// their existing behavior; Forma drops only reconstructible CPU rasters.
    fn release_cpu_cache(&self) {}
    fn vector_snapshot(&self, scale:f32, background:bool)->Arc<DisplayList>;
    fn gpu_params(&self, width:u32, height:u32, scale:f32, opaque:bool)->Vec<f32>;
    fn gpu_paints(&self)->Vec<f32>;
    /// Replace renderer-owned storage while retaining its allocation. Defaults
    /// preserve compatibility with external implementations of RenderScene.
    fn gpu_params_into(&self, width:u32, height:u32, scale:f32, opaque:bool, out:&mut Vec<f32>) {
        out.clear();
        out.extend(self.gpu_params(width, height, scale, opaque));
    }
    fn gpu_paints_into(&self, out:&mut Vec<f32>) {
        out.clear();
        out.extend(self.gpu_paints());
    }
}
impl RenderScene for Button {
    fn release_cpu_cache(&self) { Button::release_cpu_cache(self); }
    fn vector_snapshot(&self,s:f32,b:bool)->Arc<DisplayList>{Button::vector_snapshot(self,s,b)}
    fn gpu_params(&self,w:u32,h:u32,s:f32,o:bool)->Vec<f32>{Button::gpu_params(self,w,h,s,o)}
    fn gpu_paints(&self)->Vec<f32>{Button::gpu_paints(self)}
    fn gpu_params_into(&self, width:u32, height:u32, scale:f32, opaque:bool, out:&mut Vec<f32>) {
        out.clear();
        out.extend([width as f32, height as f32, scale, if opaque { 1. } else { 0. }]);
        out.extend(self.fill.color().map(|v| v as f32 / 255.));
        out.extend(self.border.color().map(|v| v as f32 / 255.));
    }
    fn gpu_paints_into(&self, out:&mut Vec<f32>) {
        out.clear();
        out.extend(self.fill.color().into_iter().chain(self.border.color()).map(|v| v as f32 / 255.));
        if self.template.reveal.is_some() { out.extend(self.reveal_paint().gpu()); }
    }
}
#[wasm_bindgen]
impl Button {
    pub fn release_cpu_cache(&self) { self.raster_cache.borrow_mut().take(); }
    pub fn gpu_paints(&self)->Vec<f32>{
        let mut paints = Vec::with_capacity(if self.template.reveal.is_some() { 16 } else { 8 });
        self.gpu_paints_into(&mut paints);
        paints
    }
}
