//! Small shared outline-text rasterizer, used unchanged by native and Web/WASM.
//!
//! `ttf-parser` only reads the font and supplies vector contours. Curve flattening,
//! winding fill, antialiasing and compositing below are our own. This currently
//! lays out a single line of Latin/Cyrillic glyphs; it is not a complex-script
//! shaping engine (ligatures, bidirectional text and combining marks need shaping).

use ttf_parser::{Face, GlyphId, OutlineBuilder};
use std::sync::OnceLock;

const FONT: &[u8] = include_bytes!("../assets/Ubuntu-Light.ttf");
const CURVE_TOLERANCE: f32 = 0.15;

fn font() -> &'static Face<'static> {
    static FACE: OnceLock<Face<'static>> = OnceLock::new();
    FACE.get_or_init(|| Face::parse(FONT, 0).expect("embedded font"))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Point {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

impl Point {
    fn middle(self, other: Self) -> Self {
        Self {
            x: self.x * 0.5 + other.x * 0.5,
            y: self.y * 0.5 + other.y * 0.5,
        }
    }

    fn distance_to_segment(self, a: Self, b: Self) -> f32 {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let length = dx.hypot(dy);
        if length <= f32::EPSILON {
            (self.x - a.x).hypot(self.y - a.y)
        } else {
            let t = (((self.x - a.x) * dx + (self.y - a.y) * dy) / (length * length)).clamp(0., 1.);
            (self.x - (a.x + t * dx)).hypot(self.y - (a.y + t * dy))
        }
    }
}

struct Contours {
    edges: Vec<(Point, Point)>,
    current: Point,
    start: Point,
    origin: Point,
    units_to_pixels: f32,
}

impl Contours {
    fn point(&self, x: f32, y: f32) -> Point {
        Point {
            x: self.origin.x + x * self.units_to_pixels,
            y: self.origin.y - y * self.units_to_pixels,
        }
    }

    fn edge(&mut self, a: Point, b: Point) {
        // Horizontal segments cannot cross a scanline. The remaining segments
        // retain direction so counters (e.g. the hole in "О") remain empty.
        if a.y != b.y {
            self.edges.push((a, b));
        }
    }

    fn quadratic(&mut self, a: Point, b: Point, c: Point, depth: u8) {
        if depth >= 12 || b.distance_to_segment(a, c) <= CURVE_TOLERANCE {
            self.edge(a, c);
            return;
        }
        let ab = a.middle(b);
        let bc = b.middle(c);
        let middle = ab.middle(bc);
        self.quadratic(a, ab, middle, depth + 1);
        self.quadratic(middle, bc, c, depth + 1);
    }

    fn cubic(&mut self, a: Point, b: Point, c: Point, d: Point, depth: u8) {
        if depth >= 12
            || b.distance_to_segment(a, d).max(c.distance_to_segment(a, d)) <= CURVE_TOLERANCE
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

fn glyph(face: &Face<'_>, character: char) -> GlyphId {
    face.glyph_index(character).unwrap_or(GlyphId(0))
}

pub fn measure_line(value:&str,font_size:f32)->[f32;2] {
    if !font_size.is_finite()||font_size<=0.{return [0.;2];}
    let face=font();
    let height=(face.ascender() as f32-face.descender() as f32+face.line_gap() as f32)*font_size/face.units_per_em() as f32;
    [measure_text(value,font_size),height]
}

fn advance(face: &Face<'_>, glyph: GlyphId) -> f32 {
    face.glyph_hor_advance(glyph).unwrap_or(0) as f32
}

/// Single-line advance width in logical pixels. No complex shaping yet.
pub fn measure_text(text: &str, font_size: f32) -> f32 {
    if !font_size.is_finite() || font_size <= 0. {
        return 0.;
    }
    let face = font();
    let units = text
        .chars()
        .map(|c| advance(face, glyph(face, c)))
        .sum::<f32>();
    pixels_from_units(units, font_size, face.units_per_em())
}

/// Advance units to logical pixels. One definition, because the vector path sums its own
/// advances while resolving outlines and has to land on the same float as `measure_text`.
fn pixels_from_units(units: f32, font_size: f32, units_per_em: u16) -> f32 {
    units * font_size / units_per_em as f32
}

/// Find a caret boundary in one pass. Summing font units before converting to
/// pixels matches measure_text(prefix) without measuring every prefix again.
pub(crate) fn hit_character(text: &str, font_size: f32, x: f32) -> usize {
    let face = font();
    let mut units = 0.;
    let mut previous = 0.;
    for (offset, ch) in text.char_indices() {
        units += advance(face, glyph(face, ch));
        let next = units * font_size / face.units_per_em() as f32;
        if x < (previous + next) * 0.5 { return offset; }
        previous = next;
    }
    text.len()
}

/// Borrow one glyph's physical contours and convert each edge to logical pixels
/// on demand. The callback consumes these before the scratch buffer is reused.
pub(crate) struct GlyphEdges<'a> {
    edges: std::slice::Iter<'a, (Point, Point)>,
    scale: f32,
}

impl Iterator for GlyphEdges<'_> {
    type Item = [f32; 4];

    fn next(&mut self) -> Option<Self::Item> {
        self.edges.next().map(|&(a, b)| {
            [a.x / self.scale, a.y / self.scale, b.x / self.scale, b.y / self.scale]
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
    edges: Vec<(Point, Point)>,
    ids: Vec<GlyphId>,
}

/// Resolve every character once, keeping the identifiers and summing exactly the advances
/// `measure_text` sums. Drawing a label needs both, and measuring first then resolving again per
/// outline pays for the same font table lookup twice per character.
fn glyph_ids(face: &Face<'_>, text: &str, ids: &mut Vec<GlyphId>) -> f32 {
    ids.clear();
    let mut units = 0.;
    for ch in text.chars() {
        let id = glyph(face, ch);
        units += advance(face, id);
        ids.push(id);
    }
    units
}

/// Vector contours only: the GPU determines winding and pixel coverage.
/// One path per glyph keeps work bounded to that glyph's rectangle. Streaming
/// avoids retaining and allocating a separate temporary path for every glyph.
pub(crate) fn vector_glyphs(text:&str,font_size:f32,rect:[f32;4],scale:f32,scratch:&mut VectorScratch,mut emit:impl FnMut(GlyphEdges<'_>)) {
    if rect[2]<=0.||rect[3]<=0.||font_size<=0. {return;}
    let face=font();
    let units_to_pixels=font_size*scale/face.units_per_em()as f32;
    let mut ids=std::mem::take(&mut scratch.ids);
    let units=glyph_ids(&face,text,&mut ids);
    // The same guard and the same summation order as `measure_text`: a non-finite size still
    // measures as zero width, and the outlines below still draw.
    let width=if font_size.is_finite(){pixels_from_units(units,font_size,face.units_per_em())}else{0.};
    let origin=Point{x:(rect[0]+rect[2]*0.5-width*0.5)*scale,
        y:(rect[1]+rect[3]*0.5)*scale+(face.ascender()as f32+face.descender()as f32)*units_to_pixels*0.5};
    let mut contours=Contours{edges:std::mem::take(&mut scratch.edges),current:origin,start:origin,origin,units_to_pixels};
    for &id in &ids{
        face.outline_glyph(id,&mut contours);
        if !contours.edges.is_empty(){
            emit(GlyphEdges { edges: contours.edges.iter(), scale });
            contours.edges.clear();
        }
        contours.origin.x+=advance(&face,id)*units_to_pixels;
    }
    scratch.edges = contours.edges;
    scratch.ids = ids;
}

/// Draw a centered, single-line label into a straight-alpha RGBA pixel buffer.
/// `rect = [x, y, width, height]` and font size are logical pixels; `scale` is
/// device pixels per logical pixel. Coverage is clipped to both rect and canvas.
pub fn draw_text(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    scale: f32,
    text: &str,
    font_size: f32,
    rect: [f32; 4],
    color: [u8; 4],
) {
    let Some(required) = (width as usize)
        .checked_mul(height as usize)
        .and_then(|size| size.checked_mul(4))
    else {
        return;
    };
    if pixels.len() < required
        || width == 0
        || height == 0
        || color[3] == 0
        || !scale.is_finite()
        || scale <= 0.
        || !font_size.is_finite()
        || font_size <= 0.
        || !rect.iter().all(|n| n.is_finite())
        || rect[2] <= 0.
        || rect[3] <= 0.
    {
        return;
    }
    let clip = [
        (rect[0] * scale).max(0.),
        (rect[1] * scale).max(0.),
        ((rect[0] + rect[2]) * scale).min(width as f32),
        ((rect[1] + rect[3]) * scale).min(height as f32),
    ];
    if !clip.iter().all(|n| n.is_finite()) || clip[0] >= clip[2] || clip[1] >= clip[3] {
        return;
    }
    let face = font();
    let units_to_pixels = font_size * scale / face.units_per_em() as f32;
    let glyphs: Vec<_> = text.chars().map(|c| glyph(&face, c)).collect();
    let text_width = glyphs.iter().map(|&g| advance(&face, g)).sum::<f32>() * units_to_pixels;
    let origin = Point {
        x: (rect[0] + rect[2] * 0.5) * scale - text_width * 0.5,
        y: (rect[1] + rect[3] * 0.5) * scale
            + (face.ascender() as f32 + face.descender() as f32) * units_to_pixels * 0.5,
    };
    if !units_to_pixels.is_finite() || !origin.x.is_finite() || !origin.y.is_finite() {
        return;
    }
    let mut contours = Contours {
        edges: Vec::new(),
        current: origin,
        start: origin,
        origin,
        units_to_pixels,
    };
    for glyph in glyphs {
        face.outline_glyph(glyph, &mut contours);
        contours.origin.x += advance(&face, glyph) * units_to_pixels;
    }
    rasterize(pixels, width, &contours.edges, clip, color);
}

/// Exact signed area one edge (physical pixels) contributes to the pixel whose
/// left edge is `px` over the row strip [`y0`, `y1`): the integral of
/// clamp(x_edge(y) - px, 0, 1) signed by direction. Same formula as `edge_area`
/// in `vector.wgsl` (there the strip is one whole pixel), so CPU and GPU
/// coverage agree up to rounding.
fn edge_area(a: Point, b: Point, px: f32, y0: f32, y1: f32) -> f32 {
    if a.y == b.y {
        return 0.;
    }
    let ya = a.y.min(b.y).max(y0);
    let yb = a.y.max(b.y).min(y1);
    if ya >= yb {
        return 0.;
    }
    let slope = (b.x - a.x) / (b.y - a.y);
    let fa = a.x + (ya - a.y) * slope - px;
    let fb = a.x + (yb - a.y) * slope - px;
    let d = fb - fa;
    let mut area = fa.clamp(0., 1.);
    if d != 0. {
        let t0 = (-fa / d).clamp(0., 1.);
        let t1 = ((1. - fa) / d).clamp(0., 1.);
        let lo = t0.min(t1);
        let hi = t0.max(t1);
        let flo = (fa + d * lo).clamp(0., 1.);
        let fhi = (fa + d * hi).clamp(0., 1.);
        area = if d > 0. { 1. - hi } else { lo } + (flo + fhi) * 0.5 * (hi - lo);
    }
    (if b.y > a.y { area } else { -area }) * (yb - ya)
}

/// Fill a closed contour set (physical-pixel edges) into straight-alpha RGBA with
/// exact area coverage under the nonzero rule, clipped to `clip` (physical
/// pixels, fractional edges scale the boundary column/row). Shared by text and
/// composed shapes on native and WASM.
pub(crate) fn rasterize(
    pixels: &mut [u8],
    width: u32,
    edges: &[(Point, Point)],
    clip: [f32; 4],
    color: [u8; 4],
) {
    if edges.is_empty() {
        return;
    }
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for &(a, b) in edges {
        for p in [a, b] {
            if !p.x.is_finite() || !p.y.is_finite() {
                return;
            }
            bounds[0] = bounds[0].min(p.x);
            bounds[1] = bounds[1].min(p.y);
            bounds[2] = bounds[2].max(p.x);
            bounds[3] = bounds[3].max(p.y);
        }
    }
    let left = bounds[0].max(clip[0]).floor().max(0.) as usize;
    let top = bounds[1].max(clip[1]).floor().max(0.) as usize;
    let right = bounds[2].min(clip[2]).ceil().max(0.) as usize;
    let bottom = bounds[3].min(clip[3]).ceil().max(0.) as usize;
    if left >= right || top >= bottom {
        return;
    }
    let columns = right - left;
    // Per row: exact per-pixel areas where an edge crosses the column, plus a
    // difference array for the full-height contribution of every column to the
    // left of the edge. Scratch is one clipped row, never a full canvas.
    let mut area = vec![0f32; columns];
    let mut full = vec![0f32; columns + 1];
    for y in top..bottom {
        let row_top = (y as f32).max(clip[1]);
        let row_bottom = (y as f32 + 1.).min(clip[3]);
        if row_top >= row_bottom {
            continue;
        }
        area.fill(0.);
        full.fill(0.);
        let mut touched = false;
        for &(a, b) in edges {
            let ya = a.y.min(b.y).max(row_top);
            let yb = a.y.max(b.y).min(row_bottom);
            if ya >= yb {
                continue;
            }
            touched = true;
            let slope = (b.x - a.x) / (b.y - a.y);
            let xa = a.x + (ya - a.y) * slope;
            let xb = a.x + (yb - a.y) * slope;
            let (xmin, xmax) = (xa.min(xb), xa.max(xb));
            let signed = if b.y > a.y { yb - ya } else { ya - yb };
            // Columns entirely left of the edge receive the whole row height.
            let first = ((xmin - 1.).floor().max(left as f32) as usize).min(right);
            if first > left {
                full[0] += signed;
                full[first - left] -= signed;
            }
            let last = ((xmax.floor() + 1.).min(right as f32).max(left as f32)) as usize;
            for px in first..last {
                area[px - left] += edge_area(a, b, px as f32, row_top, row_bottom);
            }
        }
        if !touched {
            continue;
        }
        let mut running = 0.;
        for offset in 0..columns {
            running += full[offset];
            let px = (left + offset) as f32;
            let horizontal = ((px + 1.).min(clip[2]) - px.max(clip[0])).clamp(0., 1.);
            let coverage = (running + area[offset]).abs().min(1.) * horizontal;
            if coverage <= 0. {
                continue;
            }
            let pixel = (y * width as usize + left + offset) * 4;
            blend(&mut pixels[pixel..pixel + 4], color, coverage);
        }
    }
}

fn blend(pixel: &mut [u8], color: [u8; 4], coverage: f32) {
    let source_alpha = color[3] as f32 / 255. * coverage;
    let retained_alpha = pixel[3] as f32 / 255. * (1. - source_alpha);
    let alpha = source_alpha + retained_alpha;
    for channel in 0..3 {
        pixel[channel] = ((color[channel] as f32 * source_alpha
            + pixel[channel] as f32 * retained_alpha)
            / alpha)
            .round() as u8;
    }
    pixel[3] = (alpha * 255.).round() as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pre-streaming reference: retain each flattened path before the consumer
    // sees any of them. Keep the original physical-origin arithmetic, including
    // its f32 rounding; translating a cached origin-free path is not equivalent.
    fn collected_vector_glyphs(text: &str, font_size: f32, rect: [f32; 4], scale: f32) -> Vec<Vec<[f32; 4]>> {
        if rect[2] <= 0. || rect[3] <= 0. || font_size <= 0. { return Vec::new(); }
        let face = font();
        let units_to_pixels = font_size * scale / face.units_per_em() as f32;
        let origin = Point {
            x: (rect[0] + rect[2] * 0.5 - measure_text(text, font_size) * 0.5) * scale,
            y: (rect[1] + rect[3] * 0.5) * scale
                + (face.ascender() as f32 + face.descender() as f32) * units_to_pixels * 0.5,
        };
        let mut contours = Contours { edges: Vec::new(), current: origin, start: origin, origin, units_to_pixels };
        let mut paths = Vec::new();
        for ch in text.chars() {
            let id = glyph(face, ch);
            face.outline_glyph(id, &mut contours);
            if !contours.edges.is_empty() {
                paths.push(contours.edges.drain(..).map(|(a, b)| [a.x / scale, a.y / scale, b.x / scale, b.y / scale]).collect());
            }
            contours.origin.x += advance(face, id) * units_to_pixels;
        }
        paths
    }

    #[test]
    fn streamed_glyphs_preserve_exact_geometry_and_coverage() {
        let mut scratch = VectorScratch::default();
        for text in ["", "  ", "Forma ffi", "Привет ОяЖ", "A\u{301}Б🙂"] {
            for font_size in [7., 15.5, 31.25] {
                for scale in [0.75, 1., 1.25, 2., 3.] {
                    for rect in [[0., 0., 120., 50.], [-21.375, 3.125, 160.5, 45.25], [0.125, -7.75, 100., 40.]] {
                        let expected = collected_vector_glyphs(text, font_size, rect, scale);
                        let mut actual = Vec::new();
                        vector_glyphs(text, font_size, rect, scale, &mut scratch, |glyph| actual.push(glyph.collect::<Vec<_>>()));
                        let bits = |paths: &[Vec<[f32; 4]>]| paths.iter().map(|path| path.iter().map(|edge| edge.map(f32::to_bits)).collect::<Vec<_>>()).collect::<Vec<_>>();
                        assert_eq!(bits(&actual), bits(&expected), "{text:?}, {font_size}, {scale}, {rect:?}");

                        let raster = |paths: &[Vec<[f32; 4]>]| {
                            let mut pixels = vec![0; 400 * 180 * 4];
                            for path in paths {
                                let edges: Vec<_> = path.iter().map(|&[x, y, x2, y2]| (Point { x: x * scale, y: y * scale }, Point { x: x2 * scale, y: y2 * scale })).collect();
                                rasterize(&mut pixels, 400, &edges, [0., 0., 400., 180.], [80, 160, 240, 173]);
                            }
                            pixels
                        };
                        assert_eq!(raster(&actual), raster(&expected));
                    }
                }
            }
        }
    }

    #[test]
    fn linear_caret_hit_matches_prefix_measurements_at_unicode_boundaries() {
        for value in ["", "Forma", "Привет🙂 О", "A\u{301}Б"] {
            for size in [9., 15.5, 32.] {
                let reference = |x| {
                    let mut previous = 0.;
                    for (offset, ch) in value.char_indices() {
                        let next = measure_text(&value[..offset + ch.len_utf8()], size);
                        if x < (previous + next) * 0.5 { return offset; }
                        previous = next;
                    }
                    value.len()
                };
                for i in -10..1000 {
                    let x = i as f32 * 0.125;
                    assert_eq!(hit_character(value, size, x), reference(x));
                }
                for (offset, ch) in value.char_indices() {
                    let midpoint = (measure_text(&value[..offset], size)
                        + measure_text(&value[..offset + ch.len_utf8()], size)) * 0.5;
                    for x in [midpoint - 0.0001, midpoint, midpoint + 0.0001] {
                        assert_eq!(hit_character(value, size, x), reference(x));
                    }
                }
            }
        }
    }

    #[test]
    fn draws_cyrillic_font_outlines_with_antialiasing() {
        let face = Face::parse(FONT, 0).unwrap();
        assert!(face.glyph_index('П').is_some());
        assert!(face.glyph_index('я').is_some());
        let mut pixels = vec![0; 180 * 60 * 4];
        draw_text(
            &mut pixels,
            180,
            60,
            1.,
            "Привет",
            28.,
            [0., 0., 180., 60.],
            [120, 180, 240, 255],
        );
        let count = pixels.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(
            count > 150,
            "Cyrillic label must produce real glyph contours"
        );
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
        assert!(measure_text("Привет", 28.) > 60.);
        assert!((measure_text("Привет", 56.) - measure_text("Привет", 28.) * 2.).abs() < 0.01);
    }

    #[test]
    fn clips_oversized_text_to_label_rectangle() {
        let mut pixels = vec![0; 40 * 30 * 4];
        draw_text(
            &mut pixels,
            40,
            30,
            1.,
            "ННННННН",
            35.,
            [10., 10., 20., 10.],
            [255; 4],
        );
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
        for y in 0..30 {
            for x in 0..40 {
                if !(10..30).contains(&x) || !(10..20).contains(&y) {
                    assert_eq!(pixels[(y * 40 + x) * 4 + 3], 0, "escaped clip at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn handles_tiny_canvas_and_invalid_inputs_without_out_of_bounds_writes() {
        let mut pixels = [0; 4];
        draw_text(
            &mut pixels,
            1,
            1,
            2.,
            "Я",
            80.,
            [-10., -10., 30., 30.],
            [255; 4],
        );
        let before = pixels;
        draw_text(
            &mut pixels,
            1,
            1,
            1.,
            "Я",
            80.,
            [10., 10., 30., 30.],
            [255; 4],
        );
        assert_eq!(pixels, before);
        draw_text(
            &mut pixels,
            100,
            100,
            1.,
            "Я",
            80.,
            [0., 0., 30., 30.],
            [255; 4],
        );
        draw_text(
            &mut pixels,
            1,
            1,
            f32::NAN,
            "Я",
            80.,
            [0., 0., 30., 30.],
            [255; 4],
        );
        assert_eq!(pixels, before);
    }

    #[test]
    fn honors_source_alpha_on_transparent_destination() {
        let mut pixels = vec![0; 100 * 50 * 4];
        draw_text(
            &mut pixels,
            100,
            50,
            1.,
            "Ж",
            38.,
            [0., 0., 100., 50.],
            [80, 160, 240, 128],
        );
        let visible: Vec<_> = pixels.chunks_exact(4).filter(|p| p[3] > 0).collect();
        assert!(!visible.is_empty());
        for pixel in visible {
            assert_eq!(&pixel[..3], &[80, 160, 240]);
            assert!(pixel[3] <= 128);
        }
    }

    #[test]
    fn winding_fill_preserves_cyrillic_letter_counter() {
        let mut pixels = vec![0; 100 * 100 * 4];
        draw_text(
            &mut pixels,
            100,
            100,
            1.,
            "О",
            70.,
            [0., 0., 100., 100.],
            [255; 4],
        );
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
        assert_eq!(
            pixels[(50 * 100 + 50) * 4 + 3],
            0,
            "О must retain its empty center"
        );
    }

    #[test]
    fn cubic_contours_are_flattened_into_segments() {
        let origin = Point { x: 0., y: 0. };
        let mut contours = Contours {
            edges: Vec::new(),
            current: origin,
            start: origin,
            origin,
            units_to_pixels: 1.,
        };
        contours.move_to(0., 0.);
        contours.curve_to(0., 20., 20., 20., 20., 0.);
        contours.close();
        assert!(contours.edges.len() > 8);
        assert!(contours.edges.iter().any(|(a, b)| a.y.min(b.y) <= -14.));
    }

    // Scanline reference with exact x spans and 64 sub-rows: independent of the
    // closed-form edge_area, so it validates the CPU rasterizer the same way
    // tests/runtime_gpu.rs validates the shader.
    fn scanline_reference(width: usize, height: usize, edges: &[(Point, Point)]) -> Vec<f32> {
        const ROWS: usize = 64;
        let mut coverage = vec![0f32; width * height];
        let mut hits = Vec::new();
        for row in 0..height * ROWS {
            let y = (row as f32 + 0.5) / ROWS as f32;
            hits.clear();
            for &(a, b) in edges {
                if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
                    hits.push((a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y), if b.y > a.y { 1i32 } else { -1 }));
                }
            }
            hits.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (mut winding, mut previous) = (0, 0f32);
            for &(x, direction) in &hits {
                if winding != 0 {
                    let (x0, x1) = (previous.max(0.), x.min(width as f32));
                    let mut px = x0.floor().max(0.) as usize;
                    while (px as f32) < x1 && px < width {
                        coverage[(row / ROWS) * width + px] += (x1.min(px as f32 + 1.) - x0.max(px as f32)).max(0.) / ROWS as f32;
                        px += 1;
                    }
                }
                winding += direction;
                previous = x;
            }
        }
        coverage
    }

    #[test]
    fn exact_area_rasterizer_matches_scanline_reference() {
        let (width, height) = (160usize, 48usize);
        for (text, size, scale) in [("Forma Жg", 17.5, 1.), ("ilj ОБ 0.5", 11., 1.25), ("Привет", 24., 2.)] {
            let face = font();
            let units_to_pixels = size * scale / face.units_per_em() as f32;
            let origin = Point { x: 3.3, y: 30.7 };
            let mut contours = Contours { edges: Vec::new(), current: origin, start: origin, origin, units_to_pixels };
            for ch in text.chars() {
                let id = glyph(face, ch);
                face.outline_glyph(id, &mut contours);
                contours.origin.x += advance(face, id) * units_to_pixels;
            }
            let mut pixels = vec![0u8; width * height * 4];
            rasterize(&mut pixels, width as u32, &contours.edges, [0., 0., width as f32, height as f32], [255, 255, 255, 255]);
            let expected = scanline_reference(width, height, &contours.edges);
            let (mut max, mut covered) = (0u8, 0usize);
            for (i, cov) in expected.iter().enumerate() {
                let reference = (cov.min(1.) * 255.).round() as u8;
                if reference > 0 { covered += 1; }
                max = max.max(reference.abs_diff(pixels[i * 4 + 3]));
            }
            assert!(covered > 100, "{text}: fixture must rasterize text");
            assert!(max <= 2, "{text}: max coverage error {max}/255 against the scanline reference");
        }
        // Polygons through the shape path: a concave even-odd-compatible shape.
        let mut pixels = vec![0u8; 64 * 64 * 4];
        crate::shape::draw(&mut pixels, 64, 64, 1.25, &[[4., 4.], [40., 6.], [20., 20.], [44., 44.], [6., 40.]], [1.5, 2.25], [255, 255, 255, 255]);
        let points: Vec<Point> = [[4., 4.], [40., 6.], [20., 20.], [44., 44.], [6., 40.]].iter().map(|p| Point { x: (p[0] + 1.5) * 1.25, y: (p[1] + 2.25) * 1.25 }).collect();
        let edges: Vec<_> = (0..points.len()).map(|i| (points[i], points[(i + 1) % points.len()])).collect();
        let expected = scanline_reference(64, 64, &edges);
        let max = expected.iter().enumerate().map(|(i, cov)| ((cov.min(1.) * 255.).round() as u8).abs_diff(pixels[i * 4 + 3])).max().unwrap();
        assert!(max <= 2, "shape: max coverage error {max}/255");
    }
}
