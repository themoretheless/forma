//! Backend-independent vector commands. No raster buffers, DOM or GPU objects.
//! Coordinates remain logical; glyph curves are flattened to 0.15 physical px.
use crate::{markup, resource_cache::ResourceId, template, template::Content, text};
use std::sync::Arc;

pub mod ir;
pub use ir::{ClipId, ClipNode, DisplayItem, DisplayItemKind, RasterizeOpts, Transform2D};

/// Single geometric primitive yielded in painter order for a control.
pub(crate) enum ControlPrimitive<'a> {
    Label {
        bounds: [f32; 4],
        text: &'a template::Text,
    },
    Text {
        bounds: [f32; 4],
        text: &'a template::Text,
    },
    Shape {
        origin: [f32; 2],
        points: &'a [[f32; 2]],
        color: [u8; 4],
    },
    Clip {
        bounds: [f32; 4],
        radius: f32,
    },
    ClipEnd,
}

/// Read-only L2 projection of a control's scene geometry, template and scroll state.
/// Keeps `display_list` and `raster_cache` independent of the L3 `Button` widget.
#[derive(Clone, Copy)]
pub(crate) struct ControlView<'a> {
    pub(crate) scene: &'a markup::Scene,
    pub(crate) template: &'a template::Template,
    pub(crate) scroll_x: f32,
    pub(crate) scroll_y: f32,
}

impl ControlView<'_> {
    pub(crate) fn width(self) -> f32 {
        self.scene.width
    }
    pub(crate) fn height(self) -> f32 {
        self.scene.height
    }
    pub(crate) fn viewport_rect(self) -> [f32; 4] {
        let p = self.scene.padding;
        if self.scene.scroll {
            [
                p[3],
                p[0],
                (self.width() - p[1] - p[3]).max(0.),
                (self.height() - p[0] - p[2]).max(0.),
            ]
        } else {
            [0., 0., self.width(), self.height()]
        }
    }
    pub(crate) fn reveal_bounds(self) -> [f32; 4] {
        let b = &self.scene.button;
        let bounds = [b.x - self.scroll_x, b.y - self.scroll_y, b.width, b.height];
        self.template
            .reveal
            .as_ref()
            .map_or(bounds, |spec| spec.bounds(bounds))
    }
    /// Reveal bounds in unscrolled content coordinates for the vector list.
    pub(crate) fn content_reveal_bounds(self) -> [f32; 4] {
        let b = &self.scene.button;
        let bounds = [b.x, b.y, b.width, b.height];
        self.template
            .reveal
            .as_ref()
            .map_or(bounds, |spec| spec.bounds(bounds))
    }

    /// Visits button label text and template `Content` items in painter order relative to `origin`.
    pub(crate) fn visit_primitives(
        self,
        origin: [f32; 2],
        mut visit: impl FnMut(ControlPrimitive<'_>),
    ) {
        let b = &self.scene.button;
        let r = b.radius.min(b.width / 2.).min(b.height / 2.);
        let [ox, oy] = origin;
        if let Some(t) = &self.template.text {
            visit(ControlPrimitive::Label {
                bounds: [
                    ox + r * 0.3,
                    oy + r * 0.3,
                    b.width - r * 0.6,
                    b.height - r * 0.6,
                ],
                text: t,
            });
        }
        for c in &self.template.content {
            match c {
                Content::Text { bounds, text } => visit(ControlPrimitive::Text {
                    bounds: [ox + bounds[0], oy + bounds[1], bounds[2], bounds[3]],
                    text,
                }),
                Content::Shape { points, color } => visit(ControlPrimitive::Shape {
                    origin,
                    points,
                    color: *color,
                }),
                Content::Clip { bounds, radius } => visit(ControlPrimitive::Clip {
                    bounds: [ox + bounds[0], oy + bounds[1], bounds[2], bounds[3]],
                    radius: *radius,
                }),
                Content::ClipEnd => visit(ControlPrimitive::ClipEnd),
            }
        }
    }
}

pub const TILE: u32 = 32;
// Five vec4 records: kind/rule/edge range, bounds, color, radius/stroke, scroll.
pub const STRIDE: usize = 20;
/// `commands[SCROLL_MODE]` says how the runtime scroll offset (a uniform, not
/// geometry) applies to a command. Geometry stays in unscrolled content
/// coordinates, so scrolling never rebuilds or re-uploads commands/edges.
pub const SCROLL_MODE: usize = 16;
/// `commands[SCROLL_FACTOR]` scales the scroll offset for scrollbar thumbs.
pub const SCROLL_FACTOR: usize = 17;
/// `commands[SCROLL_EXTRA..SCROLL_EXTRA + 2]`: how far the scroll areas below the root have moved
/// a content command, subtracted on top of the root scroll. Zero outside such areas.
pub const SCROLL_EXTRA: usize = 18;
pub const SCROLL_FIXED: f32 = 0.;
/// Content inside the scroll viewport: drawn at `bounds - scroll`.
pub const SCROLL_CONTENT: f32 = 1.;
/// Vertical thumb: drawn at `bounds.y + scroll.y * factor`.
pub const SCROLL_THUMB_Y: f32 = 2.;
/// Horizontal thumb: drawn at `bounds.x + scroll.x * factor`.
pub const SCROLL_THUMB_X: f32 = 3.;
/// Content inside a nested scroll area: `paints[scroll.w].xy` holds cumulative area offset.
pub const SCROLL_AREA_CONTENT: f32 = 4.;
/// Vertical thumb of a nested scroll area: `scroll.z` = own area slot, `scroll.w` = parent slot or `-1`.
pub const SCROLL_AREA_THUMB_Y: f32 = 5.;
/// Horizontal thumb of a nested scroll area: `scroll.z` = own area slot, `scroll.w` = parent slot or `-1`.
pub const SCROLL_AREA_THUMB_X: f32 = 6.;

/// Logical bounds of one command after applying the scroll uniform, matching
/// the shader. Used by the CPU tile binner and reference rasterizer.
pub fn scrolled_bounds(command: &[f32], scroll: [f32; 2]) -> [f32; 4] {
    scrolled_bounds_with_areas(command, scroll, &[])
}

pub(crate) fn scrolled_bounds_with_areas(
    command: &[f32],
    scroll: [f32; 2],
    area_paints: &[f32],
) -> [f32; 4] {
    let mut b = [command[4], command[5], command[6], command[7]];
    let mode = command[SCROLL_MODE];
    if mode == SCROLL_CONTENT {
        b[0] -= scroll[0] + command[SCROLL_EXTRA];
        b[1] -= scroll[1] + command[SCROLL_EXTRA + 1];
    } else if mode == SCROLL_THUMB_Y {
        b[1] += scroll[1] * command[SCROLL_FACTOR];
    } else if mode == SCROLL_THUMB_X {
        b[0] += scroll[0] * command[SCROLL_FACTOR];
    } else if mode == SCROLL_AREA_CONTENT {
        let root_mul = command[SCROLL_EXTRA];
        let slot = command[SCROLL_EXTRA + 1] as usize * 4;
        let area = area_paints
            .get(slot..slot + 2)
            .map_or([0.; 2], |s| [s[0], s[1]]);
        b[0] -= scroll[0] * root_mul + area[0];
        b[1] -= scroll[1] * root_mul + area[1];
    } else if mode == SCROLL_AREA_THUMB_Y || mode == SCROLL_AREA_THUMB_X {
        let own = command[SCROLL_EXTRA] as usize * 4;
        let parent = if command[SCROLL_EXTRA + 1] >= 0. {
            let ps = command[SCROLL_EXTRA + 1] as usize * 4;
            area_paints
                .get(ps..ps + 2)
                .map_or([0.; 2], |s| [s[0], s[1]])
        } else {
            [0.; 2]
        };
        b[0] -= scroll[0] + parent[0];
        b[1] -= scroll[1] + parent[1];
        if mode == SCROLL_AREA_THUMB_Y {
            let own_y = area_paints.get(own + 3).copied().unwrap_or(0.);
            b[1] += own_y * command[SCROLL_FACTOR];
        } else {
            let own_x = area_paints.get(own + 2).copied().unwrap_or(0.);
            b[0] += own_x * command[SCROLL_FACTOR];
        }
    }
    b
}
#[derive(Default)]
pub struct DisplayList {
    pub commands: Vec<f32>,
    pub edges: Vec<f32>,
    pub items: Vec<DisplayItem>,
    pub clips: Vec<ClipNode>,
}

/// Buffer sizes suggested to a list that is about to be built. A scene runtime asks every
/// control for a list of nearly the same shape, so the previous sibling's exact lengths turn
/// twenty-odd doubling reallocs per control into one allocation. Hints never reach the bytes:
/// a wrong guess only makes the vector grow or keep spare capacity.
#[derive(Clone, Copy)]
pub(crate) struct ListHints {
    pub(crate) commands: usize,
    pub(crate) edges: usize,
    pub(crate) items: usize,
    pub(crate) clips: usize,
}

impl DisplayList {
    pub(crate) fn with_hints(hints: Option<ListHints>) -> Self {
        match hints {
            Some(hints) => Self {
                commands: Vec::with_capacity(hints.commands),
                edges: Vec::with_capacity(hints.edges),
                items: Vec::with_capacity(hints.items),
                clips: if hints.clips == 0 {
                    Vec::new()
                } else {
                    Vec::with_capacity(hints.clips)
                },
            },
            None => Self::default(),
        }
    }

    /// Estimate the command and edge buffers from the control's template so the first
    /// control in a scene does not double its vectors from zero.
    fn estimate_hints(model: ControlView<'_>, background: bool) -> ListHints {
        let mut commands = 1
            + usize::from(model.scene.clip) * 2
            + usize::from(background)
            + usize::from(model.scene.scroll) * 4
            + usize::from(model.template.reveal.is_some());
        let mut clips = usize::from(model.scene.clip) + usize::from(model.scene.scroll);
        let mut edges = 0usize;
        if let Some(t) = &model.template.text {
            let glyphs = t.text.chars().count();
            commands += 2 + glyphs;
            edges += glyphs * 24;
            clips += 1;
        }
        for c in &model.template.content {
            match c {
                Content::Text { text, .. } => {
                    let glyphs = text.text.chars().count();
                    commands += 2 + glyphs;
                    edges += glyphs * 24;
                    clips += 1;
                }
                Content::Shape { points, .. } => {
                    commands += 1;
                    edges += points.len();
                }
                Content::Clip { .. } => {
                    commands += 1;
                    clips += 1;
                }
                Content::ClipEnd => {
                    commands += 1;
                }
            }
        }
        ListHints {
            commands: commands * STRIDE,
            edges: edges * 4,
            items: commands,
            clips,
        }
    }
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
    pub(crate) scale: f32,
    pub(crate) background: bool,
    pub(crate) list: Arc<DisplayList>,
}
impl DisplayList {
    pub(crate) fn command(
        &mut self,
        kind: f32,
        bounds: [f32; 4],
        color: [u8; 4],
        radius: f32,
        stroke: f32,
    ) {
        self.commands.extend([kind, 0., 0., 0.]);
        self.commands.extend(bounds);
        self.commands.extend(color.map(|v| v as f32 / 255.));
        self.commands
            .extend([radius, stroke, 0., 0., 0., 0., 0., 0.]);
    }
    pub(crate) fn push_clip_node(
        &mut self,
        bounds: [f32; 4],
        radius: f32,
        crisp: bool,
        clip_top: &mut Option<ClipId>,
    ) -> ClipId {
        self.command(2., bounds, [0; 4], radius, if crisp { 1. } else { 0. });
        let id = ClipId(self.clips.len() as u32);
        self.clips.push(ClipNode {
            id,
            parent: *clip_top,
            bounds,
            radius,
            crisp,
            scroll_mode: SCROLL_FIXED,
            scroll_extra: [0.; 2],
            area_slot: None,
        });
        *clip_top = Some(id);
        id
    }
    pub(crate) fn pop_clip_node(&mut self, clip_top: &mut Option<ClipId>) {
        self.pop();
        if let Some(id) = *clip_top {
            *clip_top = self.clips.get(id.0 as usize).and_then(|c| c.parent);
        }
    }
    pub(crate) fn pop(&mut self) {
        self.command(3., [0.; 4], [0; 4], 0., 0.);
    }
    /// Mark every root-scrolled command from `start` that is not already bound to a nested scroll area.
    pub(crate) fn mark_scrolled(&mut self, start: usize) {
        for command in self.commands[start..].chunks_exact_mut(STRIDE) {
            if command[SCROLL_MODE] == SCROLL_FIXED {
                command[SCROLL_MODE] = SCROLL_CONTENT;
            } else if command[SCROLL_MODE] == SCROLL_AREA_CONTENT {
                command[SCROLL_EXTRA] = 1.;
            }
        }
    }
    pub(crate) fn mark_ir_scrolled(&mut self, item_start: usize, clip_start: usize) {
        for item in &mut self.items[item_start..] {
            if item.scroll_mode == SCROLL_FIXED {
                item.scroll_mode = SCROLL_CONTENT;
            } else if item.scroll_mode == SCROLL_AREA_CONTENT {
                item.scroll_extra[0] = 1.;
            }
        }
        for clip in &mut self.clips[clip_start..] {
            if clip.scroll_mode == SCROLL_FIXED {
                clip.scroll_mode = SCROLL_CONTENT;
            } else if clip.scroll_mode == SCROLL_AREA_CONTENT {
                clip.scroll_extra[0] = 1.;
            }
        }
    }
    /// Mark commands from `start` as moved by a dynamic nested scroll area paint slot.
    pub(crate) fn mark_area_slot_scrolled(
        &mut self,
        cmd_start: usize,
        item_start: usize,
        clip_start: usize,
        area_slot: u32,
    ) {
        for command in self.commands[cmd_start..].chunks_exact_mut(STRIDE) {
            command[SCROLL_MODE] = SCROLL_AREA_CONTENT;
            command[SCROLL_EXTRA] = 0.;
            command[SCROLL_EXTRA + 1] = area_slot as f32;
        }
        for item in &mut self.items[item_start..] {
            item.scroll_mode = SCROLL_AREA_CONTENT;
            item.scroll_extra = [0., area_slot as f32];
            item.area_slot = Some(area_slot);
        }
        for clip in &mut self.clips[clip_start..] {
            clip.scroll_mode = SCROLL_AREA_CONTENT;
            clip.scroll_extra = [0., area_slot as f32];
            clip.area_slot = Some(area_slot);
        }
    }
    /// Scrollbar thumbs for a viewport `v` and content size. Their base
    /// position is at scroll 0; the renderer offsets them by `scroll * factor`.
    pub(crate) fn scrollbars(&mut self, v: [f32; 4], content: [f32; 2], clip_id: Option<ClipId>) {
        let [cw, ch] = content;
        let thumb_color = [110, 130, 170, 255];
        if ch > v[3] && v[3] > 0. {
            let bounds = [
                (v[0] + v[2] - 5.).max(v[0]),
                v[1],
                5f32.min(v[2]),
                v[3] * v[3] / ch,
            ];
            let factor = v[3] / ch;
            self.command(5., bounds, thumb_color, 0., 0.);
            let offset = self.commands.len() - STRIDE;
            self.commands[offset + SCROLL_MODE] = SCROLL_THUMB_Y;
            self.commands[offset + SCROLL_FACTOR] = factor;
            self.items.push(DisplayItem {
                kind: DisplayItemKind::ScrollbarThumb,
                bounds,
                transform: Transform2D::IDENTITY,
                clip_id,
                resource_id: None,
                color: thumb_color,
                radius: 0.,
                stroke: 0.,
                edge_range: [0, 0],
                nonzero: false,
                paint_slots: [0, 0],
                scroll_mode: SCROLL_THUMB_Y,
                scroll_factor: factor,
                scroll_extra: [0.; 2],
                area_slot: None,
                control_index: None,
            });
        }
        if cw > v[2] && v[2] > 0. {
            let bounds = [
                v[0],
                (v[1] + v[3] - 5.).max(v[1]),
                v[2] * v[2] / cw,
                5f32.min(v[3]),
            ];
            let factor = v[2] / cw;
            self.command(5., bounds, thumb_color, 0., 0.);
            let offset = self.commands.len() - STRIDE;
            self.commands[offset + SCROLL_MODE] = SCROLL_THUMB_X;
            self.commands[offset + SCROLL_FACTOR] = factor;
            self.items.push(DisplayItem {
                kind: DisplayItemKind::ScrollbarThumb,
                bounds,
                transform: Transform2D::IDENTITY,
                clip_id,
                resource_id: None,
                color: thumb_color,
                radius: 0.,
                stroke: 0.,
                edge_range: [0, 0],
                nonzero: false,
                paint_slots: [0, 0],
                scroll_mode: SCROLL_THUMB_X,
                scroll_factor: factor,
                scroll_extra: [0.; 2],
                area_slot: None,
                control_index: None,
            });
        }
    }
    fn path_with_clip(
        &mut self,
        edges: impl IntoIterator<Item = [f32; 4]>,
        color: [u8; 4],
        nonzero: bool,
        clip_id: Option<ClipId>,
        resource_id: Option<ResourceId>,
    ) {
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
        let bounds = [bb[0], bb[1], bb[2] - bb[0], bb[3] - bb[1]];
        let edge_start = (first_edge / 4) as u32;
        let edge_count = ((self.edges.len() - first_edge) / 4) as u32;
        let start = self.commands.len();
        self.command(1., bounds, color, 0., 0.);
        self.commands[start + 1] = if nonzero { 1. } else { 0. };
        self.commands[start + 2] = edge_start as f32;
        self.commands[start + 3] = edge_count as f32;
        self.items.push(DisplayItem {
            kind: DisplayItemKind::VectorPath,
            bounds,
            transform: Transform2D::IDENTITY,
            clip_id,
            resource_id,
            color,
            radius: 0.,
            stroke: 0.,
            edge_range: [edge_start, edge_count],
            nonzero,
            paint_slots: [0, 0],
            scroll_mode: SCROLL_FIXED,
            scroll_factor: 0.,
            scroll_extra: [0.; 2],
            area_slot: None,
            control_index: None,
        });
    }
    fn text(
        &mut self,
        t: &crate::template::Text,
        bounds: [f32; 4],
        scale: f32,
        scratch: &mut text::VectorScratch,
        clip_top: &mut Option<ClipId>,
    ) {
        if t.color[3] == 0 {
            return;
        }
        let text_clip = self.push_clip_node(bounds, 0., false, clip_top);
        let run_resource =
            ResourceId::glyph_run(crate::resource_cache::hash_bytes(t.text.as_bytes()));
        text::vector_colored_glyphs(
            &text::TextDraw {
                family: t.family,
                dir: t.direction,
                text: &t.text,
                font_size: t.font_size,
                rect: bounds,
                scale,
                color: t.color,
            },
            scratch,
            |glyph, color| {
                self.path_with_clip(glyph, color, true, Some(text_clip), Some(run_resource));
            },
        );
        self.pop_clip_node(clip_top);
    }
    /// `scratch` carries the per-glyph flattening buffer. A scene that builds hundreds of
    /// near-identical control lists hands it over once so the buffer grows a single time
    /// instead of starting empty for every control.
    pub(crate) fn build_hinted(
        model: ControlView<'_>,
        scale: f32,
        background: bool,
        hints: Option<ListHints>,
        text_scratch: &mut text::VectorScratch,
    ) -> Self {
        let mut out = Self::with_hints(Some(
            hints.unwrap_or_else(|| Self::estimate_hints(model, background)),
        ));
        let mut clip_top: Option<ClipId> = None;
        let frame = [0., 0., model.width(), model.height()];
        if model.scene.clip {
            out.push_clip_node(frame, model.scene.radius, false, &mut clip_top);
        }
        if background {
            let bg_radius = if model.scene.clip {
                0.
            } else {
                model.scene.radius
            };
            out.command(0., frame, model.scene.background, bg_radius, 0.);
            out.items.push(DisplayItem {
                kind: DisplayItemKind::SolidRect,
                bounds: frame,
                transform: Transform2D::IDENTITY,
                clip_id: clip_top,
                resource_id: None,
                color: model.scene.background,
                radius: bg_radius,
                stroke: 0.,
                edge_range: [0, 0],
                nonzero: false,
                paint_slots: [0, 0],
                scroll_mode: SCROLL_FIXED,
                scroll_factor: 0.,
                scroll_extra: [0.; 2],
                area_slot: None,
                control_index: None,
            });
        }
        let v = model.viewport_rect();
        if model.scene.scroll {
            out.push_clip_node([v[0], v[1], v[2], v[3]], 0., true, &mut clip_top);
        }
        // Content coordinates: scrolling is applied by the renderer uniform.
        let content_start = out.commands.len();
        let content_item_start = out.items.len();
        let content_clip_start = out.clips.len();
        let b = &model.scene.button;
        let x = b.x;
        let y = b.y;
        let r = b.radius.min(b.width / 2.).min(b.height / 2.);
        let stroke = model.template.border.as_ref().map_or(0., |b| b.width);
        let box_bounds = [x, y, b.width, b.height];
        out.command(4., box_bounds, [0; 4], r, stroke);
        let offset = out.commands.len() - STRIDE;
        out.commands[offset + 15] = 1.; // Independent fill/border paint slots.
        out.items.push(DisplayItem {
            kind: DisplayItemKind::StyledBox,
            bounds: box_bounds,
            transform: Transform2D::IDENTITY,
            clip_id: clip_top,
            resource_id: Some(ResourceId::paint_slot(0)),
            color: b.background,
            radius: r,
            stroke,
            edge_range: [0, 0],
            nonzero: false,
            paint_slots: [0, 1],
            scroll_mode: SCROLL_FIXED,
            scroll_factor: 0.,
            scroll_extra: [0.; 2],
            area_slot: None,
            control_index: None,
        });
        model.visit_primitives([x, y], |primitive| match primitive {
            ControlPrimitive::Label { bounds, text } | ControlPrimitive::Text { bounds, text } => {
                out.text(text, bounds, scale, text_scratch, &mut clip_top);
            }
            ControlPrimitive::Shape {
                origin,
                points,
                color,
            } => {
                let mut shape_hash = crate::resource_cache::hash_bytes(&color);
                for p in points {
                    shape_hash ^= u64::from(p[0].to_bits()) | (u64::from(p[1].to_bits()) << 32);
                    shape_hash = shape_hash.wrapping_mul(0x100000001b3);
                }
                out.path_with_clip(
                    (0..points.len()).map(|i| {
                        let a = points[i];
                        let b = points[(i + 1) % points.len()];
                        [
                            origin[0] + a[0],
                            origin[1] + a[1],
                            origin[0] + b[0],
                            origin[1] + b[1],
                        ]
                    }),
                    color,
                    false,
                    clip_top,
                    Some(ResourceId::svg_shape(shape_hash)),
                );
            }
            ControlPrimitive::Clip { bounds, radius } => {
                out.push_clip_node(bounds, radius, false, &mut clip_top);
            }
            ControlPrimitive::ClipEnd => out.pop_clip_node(&mut clip_top),
        });
        if let Some(reveal) = &model.template.reveal {
            let reveal_bounds = model.content_reveal_bounds();
            let reveal_radius = reveal.target_radius.unwrap_or(r);
            out.command(6., reveal_bounds, [0; 4], reveal_radius, reveal.width);
            let offset = out.commands.len() - STRIDE;
            out.commands[offset + 14] = 2.; // Reveal data + color, after fill/border.
            out.items.push(DisplayItem {
                kind: DisplayItemKind::RevealBorder,
                bounds: reveal_bounds,
                transform: Transform2D::IDENTITY,
                clip_id: clip_top,
                resource_id: Some(ResourceId::paint_slot(2)),
                color: reveal.color,
                radius: reveal_radius,
                stroke: reveal.width,
                edge_range: [0, 0],
                nonzero: false,
                paint_slots: [2, 3],
                scroll_mode: SCROLL_FIXED,
                scroll_factor: 0.,
                scroll_extra: [0.; 2],
                area_slot: None,
                control_index: None,
            });
        }
        if model.scene.scroll {
            out.mark_scrolled(content_start);
            out.mark_ir_scrolled(content_item_start, content_clip_start);
            out.pop_clip_node(&mut clip_top);
            out.scrollbars(v, [b.width, b.height], clip_top);
        }
        if model.scene.clip {
            out.pop_clip_node(&mut clip_top);
        }
        out
    }
    /// Stable painter order within each tile, including balanced clip groups.
    pub fn tiles(&self, width: u32, height: u32, scale: f32, scroll: [f32; 2]) -> Vec<u32> {
        let mut scratch = TileScratch::default();
        self.tiles_into(width, height, scale, scroll, &mut scratch);
        scratch.data
    }

    /// Fill retained scratch storage using count -> prefix sum -> ordered fill.
    /// Clip pushes/pops share the conservative tile union of their visible
    /// descendants. Fully invisible/empty groups disappear together; descendants
    /// are restricted by every ancestor's physical support bounds. AA margins are
    /// applied BEFORE intersections so even overlapping fringe coverage survives.
    /// At fixed geometry/DPI/scroll, only ceil(width/TILE), ceil(height/TILE) matter.
    /// `scroll` is the same uniform the shader receives; geometry is unchanged.
    pub fn tiles_into<'a>(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        scroll: [f32; 2],
        scratch: &'a mut TileScratch,
    ) -> &'a [u32] {
        self.tiles_with_areas_into(width, height, scale, scroll, &[], scratch)
    }

    pub fn tiles_with_areas_into<'a>(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        scroll: [f32; 2],
        area_paints: &[f32],
        scratch: &'a mut TileScratch,
    ) -> &'a [u32] {
        let cols = width.div_ceil(TILE);
        let rows = height.div_ceil(TILE);
        let tile_count = (cols * rows) as usize;
        let headers = tile_count * 2;
        if scratch.data.len() < headers {
            scratch.data.resize(headers, 0);
        }
        scratch.data[..headers].fill(0);
        scratch.ranges.clear();
        scratch.clips.clear();
        // The tiler emits exactly one range record per command; reserving that depth keeps the
        // pass from copying the whole list several times over.
        scratch.ranges.reserve(self.commands.len() / STRIDE);
        scratch.clips.reserve(16);
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
            let b = scrolled_bounds_with_areas(c, scroll, area_paints);
            let support = [
                b[0] * scale - 1.,
                b[1] * scale - 1.,
                (b[0] + b[2]) * scale + 1.,
                (b[1] + b[3]) * scale + 1.,
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
        // The counts are already in the header, so the tail is sized once here rather than
        // doubling while the fill pass writes into it.
        let mut length = headers;
        for pair in scratch.data[..headers].chunks_exact_mut(2) {
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
/// The renderer consumes only immutable geometry and paint slots, never controls
/// or interaction state. Both the legacy leaf and the scene runtime implement it.
pub trait RenderScene {
    /// Called after successful GPU submission. External scene types may retain
    /// their existing behavior; Forma drops only reconstructible CPU rasters.
    fn release_cpu_cache(&self) {}
    /// Logical scroll offset applied by the renderer to `SCROLL_CONTENT`
    /// commands and scrollbar thumbs. Unscrollable scenes return zero.
    fn gpu_scroll(&self) -> [f32; 2] {
        [0.; 2]
    }
    fn vector_snapshot(&self, scale: f32, background: bool) -> Arc<DisplayList>;
    fn gpu_params(&self, width: u32, height: u32, scale: f32, opaque: bool) -> Vec<f32>;
    fn gpu_paints(&self) -> Vec<f32>;
    /// Replace renderer-owned storage while retaining its allocation. Defaults
    /// preserve compatibility with external implementations of RenderScene.
    fn gpu_params_into(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        opaque: bool,
        out: &mut Vec<f32>,
    ) {
        out.clear();
        out.extend(self.gpu_params(width, height, scale, opaque));
    }
    fn gpu_paints_into(&self, out: &mut Vec<f32>) {
        out.clear();
        out.extend(self.gpu_paints());
    }
    /// Float index inside `gpu_paints_into` where dynamic nested scroll-area `vec4`s start, if any.
    fn gpu_area_paints_start(&self) -> Option<usize> {
        None
    }
}
/// Uniform layout: viewport(w, h, scale, opaque), fill, border, scroll(x, y, 0, 0).
pub const PARAMS_LEN: usize = 16;
