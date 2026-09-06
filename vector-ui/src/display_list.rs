//! Backend-independent vector commands. No raster buffers, DOM or GPU objects.
//! Coordinates remain logical; glyph curves are flattened to 0.15 physical px.
use crate::{template::Content, text, Button};
use wasm_bindgen::prelude::*;

pub const TILE: u32 = 32;
// Five vec4 records: kind/rule/edge range, bounds, color, radius/stroke, reserved.
pub const STRIDE: usize = 20;
pub struct DisplayList {
    pub commands: Vec<f32>,
    pub edges: Vec<f32>,
}
pub(crate) struct CachedList {
    scale: f32,
    scroll: [f32; 2],
    background: bool,
    list: DisplayList,
}
impl DisplayList {
    fn command(&mut self, kind: f32, bounds: [f32; 4], color: [u8; 4], radius: f32, stroke: f32) {
        self.commands.extend([kind, 0., 0., 0.]);
        self.commands.extend(bounds);
        self.commands.extend(color.map(|v| v as f32 / 255.));
        self.commands
            .extend([radius, stroke, 0., 0., 0., 0., 0., 0.]);
    }
    fn push(&mut self, bounds: [f32; 4], radius: f32) {
        self.command(2., bounds, [0; 4], radius, 0.);
    }
    fn pop(&mut self) {
        self.command(3., [0.; 4], [0; 4], 0., 0.);
    }
    fn path(&mut self, edges: Vec<[f32; 4]>, color: [u8; 4], nonzero: bool) {
        if edges.is_empty() || color[3] == 0 {
            return;
        }
        let mut bb = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for e in &edges {
            bb[0] = bb[0].min(e[0]).min(e[2]);
            bb[1] = bb[1].min(e[1]).min(e[3]);
            bb[2] = bb[2].max(e[0]).max(e[2]);
            bb[3] = bb[3].max(e[1]).max(e[3]);
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
        self.commands[start + 2] = (self.edges.len() / 4) as f32;
        self.commands[start + 3] = edges.len() as f32;
        self.edges.extend(edges.into_iter().flatten());
    }
    fn text(&mut self, t: &crate::template::Text, bounds: [f32; 4], scale: f32) {
        if t.color[3] == 0 {
            return;
        }
        self.push(bounds, 0.);
        for glyph in text::vector_glyphs(&t.text, t.font_size, bounds, scale) {
            self.path(glyph, t.color, true);
        }
        self.pop();
    }
    pub fn build(model: &Button, scale: f32, background: bool) -> Self {
        let mut out = Self {
            commands: Vec::new(),
            edges: Vec::new(),
        };
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
        let v = model.viewport();
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
            );
        }
        for c in &model.template.content {
            match c {
                Content::Text { bounds, text } => out.text(
                    text,
                    [x + bounds[0], y + bounds[1], bounds[2], bounds[3]],
                    scale,
                ),
                Content::Shape { points, color } => out.path(
                    (0..points.len())
                        .map(|i| {
                            let a = points[i];
                            let b = points[(i + 1) % points.len()];
                            [x + a[0], y + a[1], x + b[0], y + b[1]]
                        })
                        .collect(),
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
        let cols = width.div_ceil(TILE);
        let rows = height.div_ceil(TILE);
        let mut bins = vec![Vec::new(); (cols * rows) as usize];
        for (id, c) in self.commands.chunks_exact(STRIDE).enumerate() {
            let group = c[0] == 2. || c[0] == 3.;
            let (x0, y0, x1, y1) = if group {
                (0, 0, cols, rows)
            } else {
                (
                    ((c[4] * scale - 1.).max(0.) / TILE as f32)
                        .floor()
                        .min(cols as f32) as u32,
                    ((c[5] * scale - 1.).max(0.) / TILE as f32)
                        .floor()
                        .min(rows as f32) as u32,
                    (((c[4] + c[6]) * scale + 1.).max(0.) / TILE as f32)
                        .ceil()
                        .min(cols as f32) as u32,
                    (((c[5] + c[7]) * scale + 1.).max(0.) / TILE as f32)
                        .ceil()
                        .min(rows as f32) as u32,
                )
            };
            for y in y0..y1 {
                for x in x0..x1 {
                    bins[(y * cols + x) as usize].push(id as u32);
                }
            }
        }
        let mut data = vec![0; bins.len() * 2];
        for (i, bin) in bins.into_iter().enumerate() {
            data[i * 2] = data.len() as u32;
            data[i * 2 + 1] = bin.len() as u32;
            data.extend(bin);
        }
        data
    }
}
impl Button {
    pub fn vector_list(&self, scale: f32, background: bool) -> std::cell::Ref<'_, DisplayList> {
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
                list: DisplayList::build(self, scale, background),
            });
        }
        drop(cache);
        std::cell::Ref::map(self.display_cache.borrow(), |c| &c.as_ref().unwrap().list)
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
        let mut p = vec![
            width as f32,
            height as f32,
            scale,
            if opaque { 1. } else { 0. },
        ];
        p.extend(self.fill.color().map(|v| v as f32 / 255.));
        p.extend(self.border.color().map(|v| v as f32 / 255.));
        p
    }
}
