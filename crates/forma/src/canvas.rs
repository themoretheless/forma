//! Application-drawn geometry inside a reserved rectangle of a scene.
//!
//! A canvas is a control whose template declares `Canvas {}`. The application
//! builds a [`CanvasContent`] with [`CanvasBuilder`] (in its own content
//! coordinates) and hands it to the runtime under the control's key, together
//! with a [`CanvasView`] (pan + uniform zoom). Content never passes through the
//! per-control template, so the template caps do not apply; [`MAX_PRIMITIVES`]
//! and [`MAX_EDGES`] bound it instead.
//!
//! Content is tessellated once. A view change only re-transforms the finished
//! edges and command bounds ([`place`]), which is linear in the edge count and
//! never re-flattens circles or glyphs.
use crate::display_list::{DisplayList, STRIDE};
use crate::geometry::column_area;
use crate::text::{self, Point};
use std::sync::Arc;

/// Largest number of primitives (lines, circles, polygons, rectangles and text
/// runs) one canvas accepts. A graph of 5000 nodes and 20000 edges with labels
/// fits several times over.
pub const MAX_PRIMITIVES: usize = 262_144;
/// Largest number of polygon edges one canvas accepts, summed over every
/// primitive (a line is 4, a circle 8..=64, a glyph its flattened outline).
pub const MAX_EDGES: usize = 4_194_304;
/// Largest polygon a single `polygon` call accepts.
pub const MAX_POLYGON_POINTS: usize = 65_536;
/// Coordinates beyond this magnitude are rejected, as in template content.
pub const MAX_COORDINATE: f32 = 1_000_000.;
/// Glyph outlines are flattened for this many device pixels per content unit,
/// so text stays smooth up to this zoom × DPI product.
const TEXT_FLATTEN_SCALE: f32 = 2.;

/// Why a canvas call refused its input. Nothing is added on error.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasError {
    /// The primitive cap [`MAX_PRIMITIVES`] would be exceeded.
    TooManyPrimitives { limit: usize },
    /// The edge cap [`MAX_EDGES`] would be exceeded.
    TooManyEdges { limit: usize },
    /// A coordinate, size, width or zoom is not finite, negative where it must
    /// be positive, or beyond [`MAX_COORDINATE`].
    InvalidGeometry(&'static str),
}

impl std::fmt::Display for CanvasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyPrimitives { limit } => {
                write!(f, "Canvas primitive limit {limit} exceeded")
            }
            Self::TooManyEdges { limit } => write!(f, "Canvas edge limit {limit} exceeded"),
            Self::InvalidGeometry(what) => write!(f, "Invalid canvas geometry: {what}"),
        }
    }
}

impl std::error::Error for CanvasError {}

/// Finished, immutable canvas geometry in content coordinates. It is a
/// display-list fragment (fill commands with edge ranges) that the runtime
/// shares by `Arc`, so handing the same content to several canvases or keeping
/// it across scene rebuilds copies nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CanvasContent {
    commands: Vec<f32>,
    edges: Vec<f32>,
    primitives: usize,
}

impl CanvasContent {
    /// Primitives added by the builder (a text run counts once).
    pub fn primitive_count(&self) -> usize {
        self.primitives
    }
    /// Fill commands, one per primitive and one per glyph.
    pub fn command_count(&self) -> usize {
        self.commands.len() / STRIDE
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len() / 4
    }
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// Builds [`CanvasContent`]. All coordinates are content units; the view
/// transform maps them into the canvas rectangle. Every method validates its
/// input and returns an error instead of panicking; on error nothing is added.
#[derive(Debug, Default)]
pub struct CanvasBuilder {
    content: CanvasContent,
}

fn finite(values: &[f32]) -> bool {
    values
        .iter()
        .all(|v| v.is_finite() && v.abs() <= MAX_COORDINATE)
}

impl CanvasBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    /// Reserve storage for roughly `primitives` shapes of `edges` edges in all.
    pub fn with_capacity(primitives: usize, edges: usize) -> Self {
        let primitives = primitives.min(MAX_PRIMITIVES);
        Self {
            content: CanvasContent {
                commands: Vec::with_capacity(primitives * STRIDE),
                edges: Vec::with_capacity(edges.min(MAX_EDGES) * 4),
                primitives: 0,
            },
        }
    }
    /// Drop everything added so far, keeping the allocations for the next build.
    pub fn clear(&mut self) {
        self.content.commands.clear();
        self.content.edges.clear();
        self.content.primitives = 0;
    }
    pub fn primitive_count(&self) -> usize {
        self.content.primitives
    }
    pub fn finish(self) -> CanvasContent {
        self.content
    }
    /// Copy of the current content; the builder keeps its storage for reuse.
    pub fn snapshot(&self) -> CanvasContent {
        self.content.clone()
    }

    fn reserve(&self, edges: usize) -> Result<(), CanvasError> {
        if self.content.primitives >= MAX_PRIMITIVES {
            return Err(CanvasError::TooManyPrimitives {
                limit: MAX_PRIMITIVES,
            });
        }
        if self.content.edges.len() / 4 + edges > MAX_EDGES {
            return Err(CanvasError::TooManyEdges { limit: MAX_EDGES });
        }
        Ok(())
    }

    /// One fill command over `edges[first_edge..]`, bounded by their box.
    fn close_path(&mut self, first_edge: usize, color: [u8; 4]) {
        let mut bb = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for e in self.content.edges[first_edge * 4..].chunks_exact(4) {
            bb[0] = bb[0].min(e[0]).min(e[2]);
            bb[1] = bb[1].min(e[1]).min(e[3]);
            bb[2] = bb[2].max(e[0]).max(e[2]);
            bb[3] = bb[3].max(e[1]).max(e[3]);
        }
        let count = self.content.edges.len() / 4 - first_edge;
        if count == 0 {
            return;
        }
        let c = color.map(|v| v as f32 / 255.);
        self.content.commands.extend([
            1.,
            1.,
            first_edge as f32,
            count as f32,
            bb[0],
            bb[1],
            bb[2] - bb[0],
            bb[3] - bb[1],
            c[0],
            c[1],
            c[2],
            c[3],
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
            0.,
        ]);
    }

    fn closed(&mut self, points: impl Iterator<Item = [f32; 2]> + Clone, color: [u8; 4]) {
        let first_edge = self.content.edges.len() / 4;
        let mut iter = points.clone();
        let Some(first) = iter.next() else {
            return;
        };
        let mut previous = first;
        for p in iter {
            self.content
                .edges
                .extend([previous[0], previous[1], p[0], p[1]]);
            previous = p;
        }
        self.content
            .edges
            .extend([previous[0], previous[1], first[0], first[1]]);
        self.close_path(first_edge, color);
    }

    /// A straight segment `width` content units wide, with butt ends.
    pub fn line(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        width: f32,
        color: [u8; 4],
    ) -> Result<(), CanvasError> {
        if !finite(&[from[0], from[1], to[0], to[1], width]) || width < 0. {
            return Err(CanvasError::InvalidGeometry("line"));
        }
        self.reserve(4)?;
        self.content.primitives += 1;
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let length = dx.hypot(dy);
        if color[3] == 0 || width == 0. || length == 0. {
            return Ok(());
        }
        let (nx, ny) = (-dy / length * width * 0.5, dx / length * width * 0.5);
        let quad = [
            [from[0] + nx, from[1] + ny],
            [to[0] + nx, to[1] + ny],
            [to[0] - nx, to[1] - ny],
            [from[0] - nx, from[1] - ny],
        ];
        self.closed(quad.into_iter(), color);
        Ok(())
    }

    /// A filled circle, flattened to 8..=64 segments by radius.
    pub fn circle(
        &mut self,
        center: [f32; 2],
        radius: f32,
        color: [u8; 4],
    ) -> Result<(), CanvasError> {
        if !finite(&[center[0], center[1], radius]) || radius < 0. {
            return Err(CanvasError::InvalidGeometry("circle"));
        }
        let segments = ((radius.sqrt() * 4.).ceil() as usize).clamp(8, 64);
        self.reserve(segments)?;
        self.content.primitives += 1;
        if color[3] == 0 || radius == 0. {
            return Ok(());
        }
        let step = std::f32::consts::TAU / segments as f32;
        self.closed(
            (0..segments).map(|i| {
                let (s, c) = (i as f32 * step).sin_cos();
                [center[0] + c * radius, center[1] + s * radius]
            }),
            color,
        );
        Ok(())
    }

    /// A filled polygon (nonzero rule); at least three points.
    pub fn polygon(&mut self, points: &[[f32; 2]], color: [u8; 4]) -> Result<(), CanvasError> {
        if points.len() < 3
            || points.len() > MAX_POLYGON_POINTS
            || !points.iter().all(|p| finite(p))
        {
            return Err(CanvasError::InvalidGeometry("polygon"));
        }
        self.reserve(points.len())?;
        self.content.primitives += 1;
        if color[3] != 0 {
            self.closed(points.iter().copied(), color);
        }
        Ok(())
    }

    /// A filled axis-aligned rectangle `[x, y, width, height]`.
    pub fn rect(&mut self, rect: [f32; 4], color: [u8; 4]) -> Result<(), CanvasError> {
        if !finite(&rect) || rect[2] < 0. || rect[3] < 0. {
            return Err(CanvasError::InvalidGeometry("rect"));
        }
        self.reserve(4)?;
        self.content.primitives += 1;
        if color[3] == 0 || rect[2] == 0. || rect[3] == 0. {
            return Ok(());
        }
        let [x, y, w, h] = rect;
        self.closed(
            [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].into_iter(),
            color,
        );
        Ok(())
    }

    /// One line of text whose top-left corner is `origin`, using the same font
    /// and glyph outlines as scene text. Glyphs scale with the view zoom.
    pub fn text(
        &mut self,
        origin: [f32; 2],
        value: &str,
        font_size: f32,
        color: [u8; 4],
    ) -> Result<(), CanvasError> {
        if !finite(&[origin[0], origin[1], font_size]) || font_size <= 0. || value.contains('\n') {
            return Err(CanvasError::InvalidGeometry("text"));
        }
        self.reserve(0)?;
        let [width, height] = text::measure_line(value, font_size);
        let first_edge = self.content.edges.len();
        let first_command = self.content.commands.len();
        let mut scratch = text::VectorScratch::prewarm();
        let mut overflow = false;
        text::vector_glyphs(
            text::FontFamily::Sans,
            value,
            font_size,
            [origin[0], origin[1], width.max(1.), height.max(1.)],
            TEXT_FLATTEN_SCALE,
            &mut scratch,
            |glyph| {
                if overflow || self.content.edges.len() / 4 + glyph.len() > MAX_EDGES {
                    overflow = true;
                    return;
                }
                let start = self.content.edges.len() / 4;
                for e in glyph {
                    self.content.edges.extend(e);
                }
                if color[3] != 0 {
                    self.close_path(start, color);
                } else {
                    self.content.edges.truncate(start * 4);
                }
            },
        );
        if overflow {
            self.content.edges.truncate(first_edge);
            self.content.commands.truncate(first_command);
            return Err(CanvasError::TooManyEdges { limit: MAX_EDGES });
        }
        self.content.primitives += 1;
        Ok(())
    }
}

/// Pan in canvas-local logical pixels plus a uniform zoom:
/// `local = content * zoom + pan`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasView {
    pub pan: [f32; 2],
    pub zoom: f32,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            pan: [0.; 2],
            zoom: 1.,
        }
    }
}

impl CanvasView {
    pub fn new(pan: [f32; 2], zoom: f32) -> Result<Self, CanvasError> {
        if !finite(&pan) || !zoom.is_finite() || zoom <= 0. {
            return Err(CanvasError::InvalidGeometry("view"));
        }
        Ok(Self { pan, zoom })
    }
    pub fn to_local(&self, content: [f32; 2]) -> [f32; 2] {
        [
            content[0] * self.zoom + self.pan[0],
            content[1] * self.zoom + self.pan[1],
        ]
    }
    pub fn to_content(&self, local: [f32; 2]) -> [f32; 2] {
        [
            (local[0] - self.pan[0]) / self.zoom,
            (local[1] - self.pan[1]) / self.zoom,
        ]
    }
    /// Zoom by `factor` keeping the content point under `local` fixed.
    pub fn zoomed_at(&self, local: [f32; 2], factor: f32) -> Result<Self, CanvasError> {
        let anchor = self.to_content(local);
        let zoom = self.zoom * factor;
        Self::new(
            [local[0] - anchor[0] * zoom, local[1] - anchor[1] * zoom],
            zoom,
        )
    }
}

/// What a pointer did over a canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanvasEventKind {
    Move,
    Press,
    Release,
    /// The pointer left the canvas or the gesture was cancelled.
    Leave,
    /// Wheel or touchpad scroll in logical pixels, same sign as `Runtime::scroll`.
    Wheel {
        dx: f32,
        dy: f32,
    },
}

/// One pointer event delivered to the application, in canvas-local logical
/// pixels (origin at the canvas rectangle's top-left) and in content units
/// (through the view at the time of the event).
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasEvent {
    pub key: String,
    pub kind: CanvasEventKind,
    pub local: [f32; 2],
    pub content: [f32; 2],
}

/// Counters for tests and diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CanvasStats {
    /// Content replacements through `set_canvas_content`.
    pub content_sets: u32,
    /// View changes through `set_canvas_view`.
    pub view_sets: u32,
    /// Times the content was re-transformed into scene coordinates.
    pub placements: u32,
}

/// Content transformed into scene coordinates for one origin and view.
#[derive(Clone)]
pub(crate) struct Placed {
    pub(crate) origin: [f32; 2],
    pub(crate) view: CanvasView,
    pub(crate) list: Arc<DisplayList>,
}

/// Map content geometry to scene coordinates: `origin + content * zoom + pan`.
/// Only bounds and edges change; commands keep their edge ranges and colors.
pub(crate) fn place(content: &CanvasContent, origin: [f32; 2], view: CanvasView) -> DisplayList {
    let z = view.zoom;
    let tx = origin[0] + view.pan[0];
    let ty = origin[1] + view.pan[1];
    let mut commands = content.commands.clone();
    for c in commands.chunks_exact_mut(STRIDE) {
        c[4] = c[4] * z + tx;
        c[5] = c[5] * z + ty;
        c[6] *= z;
        c[7] *= z;
    }
    let edges = content
        .edges
        .chunks_exact(4)
        .flat_map(|e| [e[0] * z + tx, e[1] * z + ty, e[2] * z + tx, e[3] * z + ty])
        .collect();
    DisplayList { commands, edges }
}

/// Rasterize placed canvas geometry into a straight-alpha RGBA buffer of
/// `width × height` physical pixels. `offset` shifts scene coordinates (the
/// negated scroll offset) and `clip` is the canvas rectangle in shifted scene
/// coordinates. Coverage uses the same exact-area edge integration as scene
/// text and shapes, restricted per row to the columns the edges can touch, so
/// long thin lines cost their length rather than their bounding box.
pub(crate) fn rasterize(
    list: &DisplayList,
    pixels: &mut [u8],
    width: u32,
    height: u32,
    scale: f32,
    offset: [f32; 2],
    clip: [f32; 4],
) {
    if pixels.len() != width as usize * height as usize * 4 {
        return;
    }
    let clip = [
        (clip[0] * scale).max(0.),
        (clip[1] * scale).max(0.),
        ((clip[0] + clip[2]) * scale).min(width as f32),
        ((clip[1] + clip[3]) * scale).min(height as f32),
    ];
    if clip[0] >= clip[2] || clip[1] >= clip[3] {
        return;
    }
    let mut scratch = RowScratch::default();
    let mut edges = Vec::new();
    for c in list.commands.chunks_exact(STRIDE) {
        if c[0] != 1. {
            continue;
        }
        let x0 = (c[4] + offset[0]) * scale;
        let y0 = (c[5] + offset[1]) * scale;
        let x1 = x0 + c[6] * scale;
        let y1 = y0 + c[7] * scale;
        if x1 <= clip[0] || y1 <= clip[1] || x0 >= clip[2] || y0 >= clip[3] {
            continue;
        }
        let first = c[2] as usize;
        let count = c[3] as usize;
        let Some(range) = list.edges.get(first * 4..(first + count) * 4) else {
            continue;
        };
        edges.clear();
        edges.extend(range.chunks_exact(4).map(|e| {
            (
                Point {
                    x: (e[0] + offset[0]) * scale,
                    y: (e[1] + offset[1]) * scale,
                },
                Point {
                    x: (e[2] + offset[0]) * scale,
                    y: (e[3] + offset[1]) * scale,
                },
            )
        }));
        let color = [c[8], c[9], c[10], c[11]].map(|v| (v * 255.).round() as u8);
        fill(
            pixels,
            width,
            [x0, y0, x1, y1],
            &edges,
            clip,
            color,
            &mut scratch,
        );
    }
}

#[derive(Default)]
struct RowScratch {
    area: Vec<f32>,
    full: Vec<f32>,
    edges: Vec<Edge>,
}

/// A non-horizontal edge with its slope resolved once per primitive.
#[derive(Clone, Copy)]
struct Edge {
    x0: f32,
    y0: f32,
    top: f32,
    bottom: f32,
    slope: f32,
    /// +1 for downward edges, -1 for upward ones.
    dir: f32,
}

fn fill(
    pixels: &mut [u8],
    width: u32,
    bounds: [f32; 4],
    edges: &[(Point, Point)],
    clip: [f32; 4],
    color: [u8; 4],
    scratch: &mut RowScratch,
) {
    scratch.edges.clear();
    scratch.edges.extend(
        edges
            .iter()
            .filter(|(a, b)| a.y != b.y)
            .map(|&(a, b)| Edge {
                x0: a.x,
                y0: a.y,
                top: a.y.min(b.y),
                bottom: a.y.max(b.y),
                slope: (b.x - a.x) / (b.y - a.y),
                dir: if b.y > a.y { 1. } else { -1. },
            }),
    );
    let top = bounds[1].max(clip[1]).floor() as usize;
    let bottom = bounds[3].min(clip[3]).ceil() as usize;
    let clip_left = clip[0].floor() as usize;
    let clip_right = clip[2].ceil() as usize;
    let RowScratch { area, full, edges } = scratch;
    for y in top..bottom {
        let row_top = (y as f32).max(clip[1]);
        let row_bottom = (y as f32 + 1.).min(clip[3]);
        if row_top >= row_bottom {
            continue;
        }
        // Coverage is zero left and right of every edge crossing this row
        // (closed contours), so only that span needs evaluating.
        let mut left = usize::MAX;
        let mut right = 0usize;
        for e in edges.iter() {
            let ya = e.top.max(row_top);
            let yb = e.bottom.min(row_bottom);
            if ya >= yb {
                continue;
            }
            let xa = e.x0 + (ya - e.y0) * e.slope;
            let xb = e.x0 + (yb - e.y0) * e.slope;
            left = left.min((xa.min(xb) - 1.).floor().max(0.) as usize);
            right = right.max((xa.max(xb).floor() + 2.).max(0.) as usize);
        }
        let left = left.max(clip_left);
        let right = right.min(clip_right);
        if left >= right {
            continue;
        }
        let columns = right - left;
        if area.len() < columns {
            area.resize(columns, 0.);
            full.resize(columns + 1, 0.);
        }
        let area = &mut area[..columns];
        let full = &mut full[..columns + 1];
        area.fill(0.);
        full.fill(0.);
        for e in edges.iter() {
            let ya = e.top.max(row_top);
            let yb = e.bottom.min(row_bottom);
            if ya >= yb {
                continue;
            }
            // Crossing points at the slice top and bottom, as `text::edge_area`.
            let xa = e.x0 + (ya - e.y0) * e.slope;
            let xb = e.x0 + (yb - e.y0) * e.slope;
            let (xmin, xmax) = (xa.min(xb), xa.max(xb));
            let signed = e.dir * (yb - ya);
            let first = ((xmin - 1.).floor().max(left as f32) as usize).min(right);
            if first > left {
                full[0] += signed;
                full[first - left] -= signed;
            }
            let last = ((xmax.floor() + 1.).min(right as f32).max(left as f32)) as usize;
            for px in first..last {
                area[px - left] += signed * column_area(xa, xb, px as f32);
            }
        }
        let mut running = 0.;
        let row = y * width as usize;
        for offset in 0..columns {
            running += full[offset];
            let px = (left + offset) as f32;
            let horizontal = ((px + 1.).min(clip[2]) - px.max(clip[0])).clamp(0., 1.);
            let coverage = (running + area[offset]).abs().min(1.) * horizontal;
            if coverage <= 0. {
                continue;
            }
            let i = (row + left + offset) * 4;
            crate::blend::blend(&mut pixels[i..i + 4], color, coverage);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_rejects_invalid_input_without_adding() {
        let mut b = CanvasBuilder::new();
        assert!(b.line([0., 0.], [f32::NAN, 1.], 1., [255; 4]).is_err());
        assert!(b.circle([0., 0.], -1., [255; 4]).is_err());
        assert!(b.polygon(&[[0., 0.], [1., 1.]], [255; 4]).is_err());
        assert!(b.rect([0., 0., 1., f32::INFINITY], [255; 4]).is_err());
        assert!(b.text([0., 0.], "a\nb", 12., [255; 4]).is_err());
        assert!(CanvasView::new([0., 0.], 0.).is_err());
        assert_eq!(b.primitive_count(), 0);
        assert!(b.finish().is_empty());
    }

    #[test]
    fn view_round_trips_and_zooms_about_anchor() {
        let v = CanvasView::new([10., -4.], 2.5).unwrap();
        let p = v.to_content(v.to_local([3., 7.]));
        assert!((p[0] - 3.).abs() < 1e-5 && (p[1] - 7.).abs() < 1e-5);
        let z = v.zoomed_at([50., 60.], 2.).unwrap();
        let a = v.to_content([50., 60.]);
        let b = z.to_content([50., 60.]);
        assert!((a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4);
    }

    #[test]
    fn placement_transforms_bounds_and_edges() {
        let mut b = CanvasBuilder::new();
        b.rect([1., 2., 3., 4.], [255; 4]).unwrap();
        let content = b.finish();
        let placed = place(
            &content,
            [100., 50.],
            CanvasView::new([5., 6.], 2.).unwrap(),
        );
        assert_eq!(&placed.commands[4..8], &[107., 60., 6., 8.]);
        assert_eq!(&placed.edges[..2], &[107., 60.]);
        assert_eq!(placed.commands[2..4], content.commands[2..4]);
    }
}
