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
pub struct GlyphEdges<'a> {
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
pub struct VectorScratch {
    pub(super) edges: Vec<(Point, Point)>,
    pub(super) ids: Vec<(FontGlyph, f32)>,
}

impl VectorScratch {
    pub fn prewarm() -> Self {
        Self {
            edges: Vec::with_capacity(64),
            ids: Vec::with_capacity(32),
        }
    }
}

#[derive(Clone, Copy)]
struct GlyphItem {
    glyph: FontGlyph,
    ch: char,
    emoji: Option<(bool, Option<char>)>,
}

struct SubpathCollector<'a> {
    inner: &'a mut Contours,
    breaks: Vec<usize>,
}

impl OutlineBuilder for SubpathCollector<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.breaks.push(self.inner.edges.len());
        self.inner.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.inner.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.inner.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.inner.curve_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.inner.close();
    }
}

pub(super) fn origin_x_for(
    rect: [f32; 4],
    width: f32,
    scale: f32,
    text: &str,
    dir: TextDirection,
) -> f32 {
    match dir {
        TextDirection::Ltr => (rect[0] + rect[2] * 0.5 - width * 0.5) * scale,
        TextDirection::Rtl | TextDirection::Auto => {
            if resolve_direction(text, dir) == ResolvedDirection::Rtl {
                (rect[0] + (rect[2] - width).max(0.)) * scale
            } else {
                rect[0] * scale
            }
        }
    }
}

fn glyph_items_with_dir(
    family: FontFamily,
    text: &str,
    dir: TextDirection,
    out: &mut Vec<(GlyphItem, f32)>,
) -> f32 {
    out.clear();
    let mut units = 0.;
    let table = font_table_for(family);
    let has_emoji = text.chars().any(super::color_emoji::is_color_emoji_scalar);
    let chars: Vec<char> = if has_emoji || arabic::has_arabic(text) {
        text.chars().collect()
    } else {
        Vec::new()
    };
    if arabic::has_arabic(text) {
        for (idx, (form, ch)) in arabic::joined(text)
            .into_iter()
            .zip(text.chars())
            .enumerate()
        {
            let resolved = resolve_form_with_table(form, ch, table);
            let emoji = if has_emoji && resolved.0.face == 2 {
                let n1 = chars.get(idx + 1).copied();
                let n2 = chars.get(idx + 2).copied();
                let vs15 = n1 == Some('\u{FE0E}');
                let tone = [n1, n2]
                    .into_iter()
                    .flatten()
                    .find(|&m| super::color_emoji::skin_tone_fill(m).is_some());
                Some((vs15, tone))
            } else {
                None
            };
            units += resolved.1;
            out.push((
                GlyphItem {
                    glyph: resolved.0,
                    ch,
                    emoji,
                },
                resolved.1,
            ));
        }
    } else if has_emoji {
        for (idx, &ch) in chars.iter().enumerate() {
            let resolved = resolve_glyph_with_table(ch, table);
            let emoji = if resolved.0.face == 2 {
                let n1 = chars.get(idx + 1).copied();
                let n2 = chars.get(idx + 2).copied();
                let vs15 = n1 == Some('\u{FE0E}');
                let tone = [n1, n2]
                    .into_iter()
                    .flatten()
                    .find(|&m| super::color_emoji::skin_tone_fill(m).is_some());
                Some((vs15, tone))
            } else {
                None
            };
            units += resolved.1;
            out.push((
                GlyphItem {
                    glyph: resolved.0,
                    ch,
                    emoji,
                },
                resolved.1,
            ));
        }
    } else {
        for ch in text.chars() {
            let resolved = resolve_glyph_with_table(ch, table);
            units += resolved.1;
            out.push((
                GlyphItem {
                    glyph: resolved.0,
                    ch,
                    emoji: None,
                },
                resolved.1,
            ));
        }
    }
    if bidi::has_rtl(text) || resolve_direction(text, dir) == ResolvedDirection::Rtl {
        bidi::reorder_glyphs_with_dir(family, text, dir, out, |ch| GlyphItem {
            glyph: resolve_glyph_with_table(ch, table).0,
            ch,
            emoji: None,
        });
    }
    units
}

/// Resolve every character once, keeping the identifiers and advances while summing exactly the
/// advances `measure_text` sums. Drawing a label needs both, and measuring first then resolving
/// again per outline pays for the same font table lookup twice per character.
pub(super) fn glyph_ids(family: FontFamily, text: &str, ids: &mut Vec<(FontGlyph, f32)>) -> f32 {
    let mut items = Vec::with_capacity(text.chars().count());
    let units = glyph_items_with_dir(family, text, TextDirection::Ltr, &mut items);
    ids.clear();
    ids.extend(items.into_iter().map(|(item, adv)| (item.glyph, adv)));
    units
}

/// Parameters describing a styled, positioned text draw for colored/directional vector emission.
pub struct TextDraw<'a> {
    pub family: FontFamily,
    pub dir: TextDirection,
    pub text: &'a str,
    pub font_size: f32,
    pub rect: [f32; 4],
    pub scale: f32,
    pub color: [u8; 4],
}

pub(super) fn emit_colored_glyph_outlines(
    draw: &TextDraw<'_>,
    edges: &mut Vec<(Point, Point)>,
    mut emit: impl FnMut(&[(Point, Point)], [u8; 4]),
) {
    let TextDraw {
        family,
        dir,
        text,
        font_size,
        rect,
        scale,
        color: fg_color,
    } = *draw;
    if rect[2] <= 0.
        || rect[3] <= 0.
        || !font_size.is_finite()
        || font_size <= 0.
        || fg_color[3] == 0
    {
        return;
    }
    let face = glyph_face(family.primary());
    let units_to_pixels = font_size * scale / face.units_per_em() as f32;
    let mut items = Vec::with_capacity(text.chars().count());
    let units = glyph_items_with_dir(family, text, dir, &mut items);
    let width = pixels_from_units(units, font_size, face.units_per_em());
    let origin = Point {
        x: origin_x_for(rect, width, scale, text, dir),
        y: (rect[1] + rect[3] * 0.5) * scale
            + (face.ascender() as f32 + face.descender() as f32) * units_to_pixels * 0.5,
    };
    if !units_to_pixels.is_finite() || !origin.x.is_finite() || !origin.y.is_finite() {
        return;
    }
    edges.clear();
    let mut contours = Contours {
        #[cfg(test)]
        tolerance: CURVE_TOLERANCE,
        edges: std::mem::take(edges),
        current: origin,
        start: origin,
        origin,
        units_to_pixels,
    };
    let mut skip_prev_emoji_mod = false;
    for &(item, adv) in &items {
        let id = item.glyph;
        let is_skin_mod = super::color_emoji::skin_tone_fill(item.ch).is_some()
            || matches!(item.ch, '\u{FE0E}' | '\u{FE0F}');
        if skip_prev_emoji_mod && is_skin_mod {
            contours.origin.x += adv * units_to_pixels;
            continue;
        }
        skip_prev_emoji_mod = super::color_emoji::is_color_emoji_scalar(item.ch);

        let resolved_face = glyph_face(id.face);
        contours.units_to_pixels = font_size * scale / resolved_face.units_per_em() as f32;

        if let Some(layers) = super::fonts::font_bytes(id.face)
            .and_then(|bytes| super::color_emoji::colr_cpal_layers(bytes, id.id.0, 0, fg_color))
        {
            if !contours.edges.is_empty() {
                emit(&contours.edges, fg_color);
                contours.edges.clear();
            }
            for (layer_gid, rgba) in layers {
                resolved_face.outline_glyph(GlyphId(layer_gid), &mut contours);
                if !contours.edges.is_empty() {
                    emit(&contours.edges, rgba);
                    contours.edges.clear();
                }
            }
        } else if id.face == 2 {
            let palette = item.emoji.and_then(|(vs15, tone)| {
                super::color_emoji::emoji_palette_for_cluster(item.ch, vs15, tone, fg_color[3])
            });
            if let Some((fill_rgba, detail_rgba)) = palette {
                if !contours.edges.is_empty() {
                    emit(&contours.edges, fg_color);
                    contours.edges.clear();
                }
                let mut collector = SubpathCollector {
                    inner: &mut contours,
                    breaks: Vec::with_capacity(8),
                };
                resolved_face.outline_glyph(id.id, &mut collector);
                let breaks = collector.breaks;
                if let Some((start, end)) =
                    super::color_emoji::largest_contour_range(&contours.edges, &breaks)
                {
                    emit(&contours.edges[start..end], fill_rgba);
                    emit(&contours.edges, detail_rgba);
                } else if !contours.edges.is_empty() {
                    emit(&contours.edges, fill_rgba);
                }
                contours.edges.clear();
            } else {
                resolved_face.outline_glyph(id.id, &mut contours);
            }
        } else {
            resolved_face.outline_glyph(id.id, &mut contours);
        }
        contours.origin.x += adv * units_to_pixels;
    }
    if !contours.edges.is_empty() {
        emit(&contours.edges, fg_color);
        contours.edges.clear();
    }
    *edges = contours.edges;
}

/// Color-aware vector contours (`COLR`/`CPAL` v0 and `FormaEmoji` multi-tone layers) for GPU
/// [`crate::display_list::DisplayList::text`]. Keeps monochrome [`vector_glyphs`] unchanged.
pub fn vector_colored_glyphs(
    draw: &TextDraw<'_>,
    scratch: &mut VectorScratch,
    mut emit: impl FnMut(GlyphEdges<'_>, [u8; 4]),
) {
    let mut edges = std::mem::take(&mut scratch.edges);
    let scale = draw.scale;
    emit_colored_glyph_outlines(draw, &mut edges, |slice, color| {
        emit(
            GlyphEdges {
                edges: slice.iter(),
                scale,
            },
            color,
        );
    });
    scratch.edges = edges;
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
