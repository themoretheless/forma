//! Glyph outlines as flattened contours.

use super::*;

pub(super) const CURVE_TOLERANCE: f32 = 0.15;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Point {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

impl Point {
    pub(super) fn middle(self, other: Self) -> Self {
        Self {
            x: self.x * 0.5 + other.x * 0.5,
            y: self.y * 0.5 + other.y * 0.5,
        }
    }

    pub(super) fn distance_sq_to_segment(self, a: Self, b: Self) -> f32 {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len_sq = dx * dx + dy * dy;
        let (rx, ry) = if len_sq <= f32::EPSILON * f32::EPSILON {
            (self.x - a.x, self.y - a.y)
        } else {
            let t = (((self.x - a.x) * dx + (self.y - a.y) * dy) / len_sq).clamp(0., 1.);
            (self.x - (a.x + t * dx), self.y - (a.y + t * dy))
        };
        rx * rx + ry * ry
    }
}

pub(super) struct Contours {
    #[cfg(test)]
    pub(super) tolerance: f32,
    pub(super) edges: Vec<(Point, Point)>,
    pub(super) current: Point,
    pub(super) start: Point,
    pub(super) origin: Point,
    pub(super) units_to_pixels: f32,
}

impl Contours {
    pub(super) fn tolerance(&self) -> f32 {
        #[cfg(test)]
        {
            self.tolerance
        }
        #[cfg(not(test))]
        {
            CURVE_TOLERANCE
        }
    }
    pub(super) fn point(&self, x: f32, y: f32) -> Point {
        Point {
            x: self.origin.x + x * self.units_to_pixels,
            y: self.origin.y - y * self.units_to_pixels,
        }
    }

    pub(super) fn edge(&mut self, a: Point, b: Point) {
        // Horizontal segments cannot cross a scanline. The remaining segments
        // retain direction so counters (e.g. the hole in "О") remain empty.
        if a.y != b.y {
            self.edges.push((a, b));
        }
    }

    pub(super) fn quadratic(&mut self, a: Point, b: Point, c: Point, depth: u8) {
        if depth >= 12 || b.distance_sq_to_segment(a, c) <= self.tolerance() * self.tolerance() {
            self.edge(a, c);
            return;
        }
        let ab = a.middle(b);
        let bc = b.middle(c);
        let middle = ab.middle(bc);
        self.quadratic(a, ab, middle, depth + 1);
        self.quadratic(middle, bc, c, depth + 1);
    }

    pub(super) fn cubic(&mut self, a: Point, b: Point, c: Point, d: Point, depth: u8) {
        if depth >= 12
            || b.distance_sq_to_segment(a, d)
                .max(c.distance_sq_to_segment(a, d))
                <= self.tolerance() * self.tolerance()
        {
            self.edge(a, d);
            return;
        }
        let ab = a.middle(b);
        let bc = b.middle(c);
        let cd = c.middle(d);
        let abc = ab.middle(bc);
        let bcd = bc.middle(cd);
        let middle = abc.middle(bcd);
        self.cubic(a, ab, abc, middle, depth + 1);
        self.cubic(middle, bcd, cd, d, depth + 1);
    }
}

impl OutlineBuilder for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        self.current = self.point(x, y);
        self.start = self.current;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let next = self.point(x, y);
        self.edge(self.current, next);
        self.current = next;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let next = self.point(x, y);
        self.quadratic(self.current, self.point(x1, y1), next, 0);
        self.current = next;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let next = self.point(x, y);
        self.cubic(
            self.current,
            self.point(x1, y1),
            self.point(x2, y2),
            next,
            0,
        );
        self.current = next;
    }

    fn close(&mut self) {
        self.edge(self.current, self.start);
        self.current = self.start;
    }
}

#[cfg(test)]
pub(super) fn glyph(face: &Face<'_>, character: char) -> GlyphId {
    let code = character as usize;
    let table = font_table();
    if code < 1104 {
        table.glyphs[code]
    } else {
        face.glyph_index(character)
            .unwrap_or(table.glyphs[b'?' as usize])
    }
}

/// Borrow one glyph's physical contours and convert each edge to logical pixels
/// on demand. The callback consumes these before the scratch buffer is reused.
pub(crate) struct GlyphEdges<'a> {
    pub(super) edges: std::slice::Iter<'a, (Point, Point)>,
    pub(super) scale: f32,
}

impl Iterator for GlyphEdges<'_> {
    type Item = [f32; 4];

    fn next(&mut self) -> Option<Self::Item> {
        self.edges.next().map(|&(a, b)| {
            [
                a.x / self.scale,
                a.y / self.scale,
                b.x / self.scale,
                b.y / self.scale,
            ]
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.edges.size_hint()
    }
}

impl ExactSizeIterator for GlyphEdges<'_> {}

/// Temporary storage for one display-list build. Capacity follows the largest
/// glyph and the longest label, not the combined text, and is released with the build scratch.
#[derive(Default)]
pub(crate) struct VectorScratch {
    pub(super) edges: Vec<(Point, Point)>,
    pub(super) ids: Vec<(FontGlyph, f32)>,
}

impl VectorScratch {
    pub(crate) fn prewarm() -> Self {
        Self {
            edges: Vec::with_capacity(64),
            ids: Vec::with_capacity(32),
        }
    }
}

/// Resolve every character once, keeping the identifiers and advances while summing exactly the
/// advances `measure_text` sums. Drawing a label needs both, and measuring first then resolving
/// again per outline pays for the same font table lookup twice per character.
pub(super) fn glyph_ids(family: FontFamily, text: &str, ids: &mut Vec<(FontGlyph, f32)>) -> f32 {
    ids.clear();
    let mut units = 0.;
    let table = font_table_for(family);
    for ch in text.chars() {
        let resolved = resolve_glyph_with_table(ch, table);
        units += resolved.1;
        ids.push(resolved);
    }
    if bidi::has_rtl(text) {
        bidi::reorder_glyphs(family, text, ids, |ch| {
            resolve_glyph_with_table(ch, table).0
        });
    }
    units
}

/// Vector contours only: the GPU determines winding and pixel coverage.
/// One path per glyph keeps work bounded to that glyph's rectangle. Streaming
/// avoids retaining and allocating a separate temporary path for every glyph.
pub(crate) fn vector_glyphs(
    family: FontFamily,
    text: &str,
    font_size: f32,
    rect: [f32; 4],
    scale: f32,
    scratch: &mut VectorScratch,
    mut emit: impl FnMut(GlyphEdges<'_>),
) {
    if rect[2] <= 0. || rect[3] <= 0. || font_size <= 0. {
        return;
    }
    let face = glyph_face(family.primary());
    let units_to_pixels = font_size * scale / face.units_per_em() as f32;
    let mut ids = std::mem::take(&mut scratch.ids);
    let units = glyph_ids(family, text, &mut ids);
    // The same guard and the same summation order as `measure_text`: a non-finite size still
    // measures as zero width, and the outlines below still draw.
    let width = if font_size.is_finite() {
        pixels_from_units(units, font_size, face.units_per_em())
    } else {
        0.
    };
    let origin = Point {
        x: (rect[0] + rect[2] * 0.5 - width * 0.5) * scale,
        y: (rect[1] + rect[3] * 0.5) * scale
            + (face.ascender() as f32 + face.descender() as f32) * units_to_pixels * 0.5,
    };
    let mut contours = Contours {
        #[cfg(test)]
        tolerance: CURVE_TOLERANCE,
        edges: std::mem::take(&mut scratch.edges),
        current: origin,
        start: origin,
        origin,
        units_to_pixels,
    };
    for &(id, adv) in &ids {
        let resolved_face = glyph_face(id.face);
        contours.units_to_pixels = font_size * scale / resolved_face.units_per_em() as f32;
        resolved_face.outline_glyph(id.id, &mut contours);
        if !contours.edges.is_empty() {
            emit(GlyphEdges {
                edges: contours.edges.iter(),
                scale,
            });
            contours.edges.clear();
        }
        contours.origin.x += adv * units_to_pixels;
    }
    scratch.edges = contours.edges;
    scratch.ids = ids;
}
