//! Small shared outline-text rasterizer and complex-script shaper, used unchanged by native and Web/WASM.
//!
//! `ttf-parser` reads the font and supplies vector contours. Curve flattening,
//! winding fill, antialiasing, complex-script shaping (`shape_text`: standard ligatures,
//! OpenType kerning, combining diacritics, BiDi/RTL run reordering, and fallback glyph
//! chains) and compositing below are our own.

use std::sync::OnceLock;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

const FONT: &[u8] = include_bytes!("../assets/Ubuntu-Light.ttf");
const MONO: &[u8] = include_bytes!("../assets/mono/IBMPlexMono-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../assets/bold/Ubuntu-Bold.ttf");

/// Primary face of a passive text run. Fallback faces are shared by every family.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontFamily {
    /// Ubuntu Light, the default interface face.
    #[default]
    Sans,
    /// IBM Plex Mono Regular, for code and aligned columns.
    Mono,
    /// Ubuntu Bold: `Sans` with `fontWeight: bold`. Markup selects it through the weight,
    /// not through `fontFamily`.
    SansBold,
}

/// Weight requested by markup (`fontWeight`). Only the sans family has a bold face; a bold
/// monospace run keeps IBM Plex Mono Regular.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontWeight {
    #[default]
    Regular,
    Bold,
}

impl FontWeight {
    /// Markup name: `regular` (also `normal`) or `bold`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "regular" | "normal" => Some(Self::Regular),
            "bold" => Some(Self::Bold),
            _ => None,
        }
    }
}

impl FontFamily {
    /// Markup name: `sans` or `mono`. The bold face is chosen by [`FontFamily::with_weight`].
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "sans" => Some(Self::Sans),
            "mono" => Some(Self::Mono),
            _ => None,
        }
    }
    /// Name in compiled scene text (`ContentText { fontFamily: ... }`): the markup names plus
    /// `sans-bold`.
    pub fn from_scene_name(name: &str) -> Option<Self> {
        match name {
            "sans-bold" => Some(Self::SansBold),
            other => Self::from_name(other),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sans => "sans",
            Self::Mono => "mono",
            Self::SansBold => "sans-bold",
        }
    }
    /// The face for `weight`: `Sans` and `SansBold` switch, `Mono` has one weight.
    pub fn with_weight(self, weight: FontWeight) -> Self {
        match (self, weight) {
            (Self::Sans | Self::SansBold, FontWeight::Bold) => Self::SansBold,
            (Self::Sans | Self::SansBold, FontWeight::Regular) => Self::Sans,
            (Self::Mono, _) => Self::Mono,
        }
    }
    /// Index of the family's primary face, as used by [`font_face`].
    fn primary(self) -> u8 {
        match self {
            Self::Sans => 0,
            Self::Mono => 3,
            Self::SansBold => 4,
        }
    }
}
const CURVE_TOLERANCE: f32 = 0.15;

mod runs;
pub use runs::{RunFragment, RunLayout, layout_runs};

/// Shaped glyph placement produced by [`shape_text`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Resolved font glyph identifier (`GlyphId(0)` when synthesized by the fallback chain).
    pub glyph_id: u16,
    /// Bundled face: 0=Ubuntu, 1=NotoSansSC, 2=NotoEmoji, 3=IBM Plex Mono, 4=Ubuntu Bold;
    /// see [`font_face`].
    /// Shaping itself only produces 0..=2: the monospace face is not shaped.
    pub font_index: u8,
    /// Logical UTF-8 byte offset of the cluster start in the source string.
    pub cluster: usize,
    /// Horizontal advance in logical pixels.
    pub x_advance: f32,
    /// Horizontal placement offset in logical pixels (used by combining marks and kerning).
    pub x_offset: f32,
    /// Vertical placement offset in logical pixels (positive moves upward).
    pub y_offset: f32,
    /// True when the cluster belongs to a Right-to-Left (BiDi RTL) run.
    pub rtl: bool,
    /// True when the glyph is a non-spacing combining diacritical mark (`U+0300..=U+036F`, etc.).
    pub combining: bool,
    /// True when the primary font lacked this scalar and a fallback glyph was used.
    pub fallback: bool,
}

/// Returns `true` if `ch` is a non-spacing combining mark (Unicode Combining Diacritical Marks,
/// Hebrew/Arabic vowel points, or enclosing marks).
pub fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F
            | 0x0483..=0x0489
            | 0x0591..=0x05BD
            | 0x05BF
            | 0x05C1..=0x05C2
            | 0x05C4..=0x05C5
            | 0x05C7
            | 0x0610..=0x061A
            | 0x064B..=0x065F
            | 0x0670
            | 0x06D6..=0x06ED
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F
    )
}

/// Returns `true` if `ch` is a zero-advance joiner or variation selector (`ZWJ`, `ZWNJ`, `VS1..=VS16`, `VS17..=VS256`).
pub fn is_zero_width_modifier(ch: char) -> bool {
    matches!(
        ch as u32,
        0x200C..=0x200D | 0xFE00..=0xFE0F | 0xE0100..=0xE01EF
    )
}

/// Returns `true` if `ch` is an emoji modifier that attaches to a preceding emoji base without adding advance
/// (Fitzpatrick skin-tone modifiers `U+1F3FB..=U+1F3FF` or Combining Enclosing Keycap `U+20E3`).
pub fn is_emoji_modifier(ch: char) -> bool {
    matches!(ch as u32, 0x20E3 | 0x1F3FB..=0x1F3FF)
}

/// Returns `true` if `ch` is a Regional Indicator symbol (`U+1F1E6..=U+1F1FF`, used in pairs for flags).
pub fn is_regional_indicator(ch: char) -> bool {
    matches!(ch as u32, 0x1F1E6..=0x1F1FF)
}

/// Returns `true` if `ch` has strong Right-to-Left directionality (Hebrew, Arabic, Syriac,
/// Thaana, NKo, Presentation Forms).
pub fn is_rtl_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF
    )
}

/// Returns `true` if `ch` has strong Left-to-Right directionality (Latin, Greek, Cyrillic, Han, Kana, etc.).
fn is_strong_ltr_char(ch: char) -> bool {
    !is_rtl_char(ch)
        && !is_combining_mark(ch)
        && !is_zero_width_modifier(ch)
        && !ch.is_ascii_digit()
        && !matches!(ch as u32, 0x0660..=0x0669 | 0x06F0..=0x06F9)
        && (ch.is_alphabetic() || matches!(ch as u32, 0x3040..=0x9FFF | 0xAC00..=0xD7AF))
}

fn match_standard_ligature(rest: &str) -> Option<(usize, char)> {
    if rest.starts_with("ffi") {
        Some((3, '\u{FB03}'))
    } else if rest.starts_with("ffl") {
        Some((3, '\u{FB04}'))
    } else if rest.starts_with("fi") {
        Some((2, '\u{FB01}'))
    } else if rest.starts_with("fl") {
        Some((2, '\u{FB02}'))
    } else if rest.starts_with("ff") {
        Some((2, '\u{FB00}'))
    } else {
        None
    }
}

fn kern_pair_units(face: &Face<'_>, left: GlyphId, right: GlyphId) -> f32 {
    let Some(kern) = face.tables().kern else {
        return 0.;
    };
    let mut total = 0i32;
    for subtable in kern.subtables {
        if subtable.horizontal && !subtable.variable {
            if let Some(val) = subtable.glyphs_kerning(left, right) {
                total += i32::from(val);
            }
        }
    }
    total as f32
}

/// Shapes a single line of text with standard ligature substitution (`fi`, `fl`, `ff`, `ffi`, `ffl`),
/// OpenType horizontal kerning (`kern`), non-spacing combining mark attachment (zero advance with
/// centered mark offset over the base glyph), Unicode BiDi visual run reordering for RTL scripts,
/// and fallback glyph detection.
pub fn shape_text(text: &str, font_size: f32) -> Vec<ShapedGlyph> {
    if !font_size.is_finite() || font_size <= 0. || text.is_empty() {
        return Vec::new();
    }
    let face = font();
    let table = font_table();
    let scale = font_size / table.units_per_em as f32;

    #[derive(Clone)]
    struct LogicalCluster {
        glyph_id: GlyphId,
        font_index: u8,
        synthetic: bool,
        cluster: usize,
        advance_units: f32,
        x_offset_units: f32,
        y_offset_units: f32,
        rtl: bool,
        combining: bool,
        fallback: bool,
    }

    let mut clusters: Vec<LogicalCluster> = Vec::new();
    let mut byte_idx = 0;
    while byte_idx < text.len() {
        let rest = &text[byte_idx..];
        if let Some((lig_len, lig_char)) = match_standard_ligature(rest) {
            if let Some(id) = face.glyph_index(lig_char) {
                let adv = advance(face, id);
                clusters.push(LogicalCluster {
                    glyph_id: id,
                    font_index: 0,
                    synthetic: false,
                    cluster: byte_idx,
                    advance_units: adv,
                    x_offset_units: 0.,
                    y_offset_units: 0.,
                    rtl: false,
                    combining: false,
                    fallback: false,
                });
                byte_idx += lig_len;
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        let ch_len = ch.len_utf8();
        let zero_mod = is_zero_width_modifier(ch);
        let emoji_mod = is_emoji_modifier(ch);
        let combining = is_combining_mark(ch) || zero_mod || emoji_mod;
        let rtl = is_rtl_char(ch) || (combining && clusters.last().is_some_and(|prev| prev.rtl));
        let prev_was_zwj = clusters.last().is_some_and(|prev| {
            text.get(prev.cluster..byte_idx)
                .is_some_and(|s| s.ends_with('\u{200D}'))
        });
        let prev_was_ri = is_regional_indicator(ch)
            && clusters.last().is_some_and(|prev| {
                !prev.combining
                    && text
                        .get(prev.cluster..)
                        .and_then(|s| s.chars().next())
                        .is_some_and(is_regional_indicator)
            });
        let is_fallback = face.glyph_index(ch).is_none();
        let (resolved, advance_units) = resolve_glyph_with_table(ch, table);
        let id = resolved.id;
        let synthetic = (is_fallback && resolved.face == 0) || zero_mod;
        let raw_adv = if synthetic {
            table.units_per_em as f32 * 0.6
        } else {
            advance_units
        };
        if combining || prev_was_zwj || prev_was_ri {
            let base_adv = clusters
                .iter()
                .rev()
                .find(|c| !c.combining)
                .map_or(raw_adv, |c| c.advance_units);
            clusters.push(LogicalCluster {
                glyph_id: if zero_mod { GlyphId(0) } else { id },
                font_index: resolved.face,
                synthetic,
                cluster: byte_idx,
                advance_units: 0.,
                x_offset_units: if zero_mod { 0. } else { -base_adv * 0.5 },
                y_offset_units: if is_combining_mark(ch) {
                    table.ascender * 0.15
                } else {
                    0.
                },
                rtl,
                combining: true,
                fallback: is_fallback,
            });
        } else {
            clusters.push(LogicalCluster {
                glyph_id: id,
                font_index: resolved.face,
                synthetic,
                cluster: byte_idx,
                advance_units: raw_adv,
                x_offset_units: 0.,
                y_offset_units: 0.,
                rtl,
                combining: false,
                fallback: is_fallback,
            });
        }
        byte_idx += ch_len;
    }

    // Resolve neutral/whitespace BiDi characters bounded by RTL on both sides (UAX #9 N1).
    let len = clusters.len();
    for i in 0..len {
        let ch = text[clusters[i].cluster..].chars().next().unwrap_or(' ');
        if !is_rtl_char(ch)
            && !is_strong_ltr_char(ch)
            && !clusters[i].combining
            && !ch.is_ascii_digit()
        {
            let prev_rtl = (0..i)
                .rev()
                .find(|&j| {
                    let cj = text[clusters[j].cluster..].chars().next().unwrap_or(' ');
                    is_rtl_char(cj) || is_strong_ltr_char(cj)
                })
                .is_some_and(|j| clusters[j].rtl);
            let next_rtl = ((i + 1)..len)
                .find(|&j| {
                    let cj = text[clusters[j].cluster..].chars().next().unwrap_or(' ');
                    is_rtl_char(cj) || is_strong_ltr_char(cj)
                })
                .is_some_and(|j| clusters[j].rtl);
            if prev_rtl && next_rtl {
                clusters[i].rtl = true;
            }
        }
    }

    // Apply horizontal kerning between adjacent non-combining base glyphs in the same direction.
    let mut prev_base: Option<usize> = None;
    for i in 0..clusters.len() {
        if clusters[i].combining {
            continue;
        }
        if let Some(prev) = prev_base {
            if clusters[prev].rtl == clusters[i].rtl
                && clusters[prev].font_index == clusters[i].font_index
                && !clusters[prev].synthetic
                && !clusters[i].synthetic
            {
                let pair_face = glyph_face(clusters[i].font_index);
                let kern =
                    kern_pair_units(pair_face, clusters[prev].glyph_id, clusters[i].glyph_id)
                        * table.units_per_em as f32
                        / pair_face.units_per_em() as f32;
                if kern != 0. {
                    clusters[prev].advance_units = (clusters[prev].advance_units + kern).max(0.);
                }
            }
        }
        prev_base = Some(i);
    }

    // Group into BiDi directional runs; reverse visual order of base+mark groups inside RTL runs.
    let mut visual: Vec<ShapedGlyph> = Vec::with_capacity(clusters.len());
    let mut run_start = 0;
    while run_start < clusters.len() {
        let run_rtl = clusters[run_start].rtl;
        let mut run_end = run_start + 1;
        while run_end < clusters.len() && clusters[run_end].rtl == run_rtl {
            run_end += 1;
        }
        let slice = &clusters[run_start..run_end];
        if !run_rtl {
            for c in slice {
                visual.push(ShapedGlyph {
                    glyph_id: if c.synthetic { 0 } else { c.glyph_id.0 },
                    font_index: c.font_index,
                    cluster: c.cluster,
                    x_advance: c.advance_units * scale,
                    x_offset: c.x_offset_units * scale,
                    y_offset: c.y_offset_units * scale,
                    rtl: c.rtl,
                    combining: c.combining,
                    fallback: c.fallback,
                });
            }
        } else {
            // Keep combining marks attached after their base glyph while reversing base glyph order.
            let mut groups: Vec<&[LogicalCluster]> = Vec::new();
            let mut g_start = 0;
            while g_start < slice.len() {
                let mut g_end = g_start + 1;
                while g_end < slice.len() && slice[g_end].combining {
                    g_end += 1;
                }
                groups.push(&slice[g_start..g_end]);
                g_start = g_end;
            }
            for group in groups.into_iter().rev() {
                for c in group {
                    visual.push(ShapedGlyph {
                        glyph_id: if c.synthetic { 0 } else { c.glyph_id.0 },
                        font_index: c.font_index,
                        cluster: c.cluster,
                        x_advance: c.advance_units * scale,
                        x_offset: c.x_offset_units * scale,
                        y_offset: c.y_offset_units * scale,
                        rtl: c.rtl,
                        combining: c.combining,
                        fallback: c.fallback,
                    });
                }
            }
        }
        run_start = run_end;
    }
    visual
}

/// Measures the total horizontal advance of shaped text (including ligatures, kerning, and zero-advance combining marks)
/// without allocating a visual [`ShapedGlyph`] output vector.
pub fn measure_shaped_text(text: &str, font_size: f32) -> f32 {
    if !font_size.is_finite() || font_size <= 0. || text.is_empty() {
        return 0.;
    }
    let face = font();
    let table = font_table();
    let scale = font_size / table.units_per_em as f32;
    let mut total_units = 0.0_f32;
    let mut prev_base: Option<(GlyphId, u8, bool, bool, f32)> = None;
    let mut prev_rtl = false;
    let mut prev_cluster = 0usize;
    let mut prev_combining = false;
    let mut has_prev = false;
    let mut byte_idx = 0;
    while byte_idx < text.len() {
        let rest = &text[byte_idx..];
        if let Some((lig_len, lig_char)) = match_standard_ligature(rest) {
            if let Some(id) = face.glyph_index(lig_char) {
                let adv = advance(face, id);
                if let Some((p_id, p_font, p_rtl, p_syn, p_adv)) = prev_base {
                    if !p_rtl && p_font == 0 && !p_syn {
                        let kern = kern_pair_units(face, p_id, id);
                        if kern != 0. {
                            total_units += (p_adv + kern).max(0.) - p_adv;
                        }
                    }
                }
                total_units += adv;
                prev_base = Some((id, 0, false, false, adv));
                prev_rtl = false;
                prev_cluster = byte_idx;
                prev_combining = false;
                has_prev = true;
                byte_idx += lig_len;
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        let ch_len = ch.len_utf8();
        let zero_mod = is_zero_width_modifier(ch);
        let emoji_mod = is_emoji_modifier(ch);
        let combining = is_combining_mark(ch) || zero_mod || emoji_mod;
        let rtl = is_rtl_char(ch) || (combining && has_prev && prev_rtl);
        let prev_was_zwj = has_prev
            && text
                .get(prev_cluster..byte_idx)
                .is_some_and(|s| s.ends_with('\u{200D}'));
        let prev_was_ri = is_regional_indicator(ch)
            && has_prev
            && !prev_combining
            && text
                .get(prev_cluster..)
                .and_then(|s| s.chars().next())
                .is_some_and(is_regional_indicator);
        let is_fallback = face.glyph_index(ch).is_none();
        let (resolved, advance_units) = resolve_glyph_with_table(ch, table);
        let id = resolved.id;
        let synthetic = (is_fallback && resolved.face == 0) || zero_mod;
        let raw_adv = if synthetic {
            table.units_per_em as f32 * 0.6
        } else {
            advance_units
        };
        let is_zero_adv = combining || prev_was_zwj || prev_was_ri;
        if !is_zero_adv {
            if let Some((p_id, p_font, p_rtl, p_syn, p_adv)) = prev_base {
                if p_rtl == rtl && p_font == resolved.face && !p_syn && !synthetic {
                    let pair_face = glyph_face(resolved.face);
                    let kern = kern_pair_units(pair_face, p_id, id) * table.units_per_em as f32
                        / pair_face.units_per_em() as f32;
                    if kern != 0. {
                        total_units += (p_adv + kern).max(0.) - p_adv;
                    }
                }
            }
            total_units += raw_adv;
            prev_base = Some((id, resolved.face, rtl, synthetic, raw_adv));
        }
        prev_rtl = rtl;
        prev_cluster = byte_idx;
        prev_combining = is_zero_adv;
        has_prev = true;
        byte_idx += ch_len;
    }
    total_units * scale
}

/// Visual soft-wrapped line produced by [`wrap_shaped_text`].
#[derive(Clone, Debug, PartialEq)]
pub struct SoftWrappedLine {
    /// UTF-8 byte range in the original source text covered by this visual line (excluding trailing `\r` or `\n`).
    pub byte_range: std::ops::Range<usize>,
    /// Shaped glyphs belonging to this visual line in visual order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Total horizontal advance width of this visual line in logical pixels.
    pub width: f32,
}

/// Breaks `text` into visual lines constrained by `max_width` logical pixels, respecting hard line breaks (`\n`, `\r\n`),
/// UAX #29 / CJK / script word break opportunities, and grapheme-cluster emergency fallback when a single word exceeds `max_width`.
pub fn wrap_shaped_text(text: &str, font_size: f32, max_width: f32) -> Vec<SoftWrappedLine> {
    wrap_text_ranges(text, font_size, max_width)
        .into_iter()
        .map(|(byte_range, width)| {
            let mut glyphs = shape_text(&text[byte_range.clone()], font_size);
            for g in &mut glyphs {
                g.cluster += byte_range.start;
            }
            SoftWrappedLine {
                byte_range,
                glyphs,
                width,
            }
        })
        .collect()
}

/// Breaks `text` into visual line `(byte_range, width)` pairs constrained by `max_width` logical pixels
/// without allocating per-line [`ShapedGlyph`] vectors.
pub fn wrap_text_ranges(
    text: &str,
    font_size: f32,
    max_width: f32,
) -> Vec<(std::ops::Range<usize>, f32)> {
    wrap_text_ranges_in(FontFamily::Sans, text, font_size, max_width)
}

/// [`wrap_text_ranges`] for a chosen family. The monospace and bold faces are measured by plain
/// advances, the same sum their renderer uses: shaping covers the default face only.
pub fn wrap_text_ranges_in(
    family: FontFamily,
    text: &str,
    font_size: f32,
    max_width: f32,
) -> Vec<(std::ops::Range<usize>, f32)> {
    if !font_size.is_finite() || font_size <= 0. {
        return Vec::new();
    }
    let measure_shaped_text = |value: &str, size: f32| match family {
        FontFamily::Sans => measure_shaped_text(value, size),
        FontFamily::Mono | FontFamily::SansBold => measure_text_in(family, value, size),
    };
    let unbounded = !max_width.is_finite() || max_width <= 0.;
    let mut result = Vec::new();
    let mut hard_start = 0;
    for raw_line in text.split('\n') {
        let raw_len = raw_line.len();
        let line = if hard_start + raw_len < text.len() {
            raw_line.strip_suffix('\r').unwrap_or(raw_line)
        } else {
            raw_line
        };
        if line.is_empty() || unbounded {
            let width = measure_shaped_text(line, font_size);
            result.push((hard_start..(hard_start + line.len()), width));
            hard_start += raw_len + 1;
            continue;
        }
        use unicode_segmentation::UnicodeSegmentation;
        let mut cur_start = 0;
        while cur_start < line.len() {
            let rest = &line[cur_start..];
            let full_w = measure_shaped_text(rest, font_size);
            if full_w <= max_width {
                result.push(((hard_start + cur_start)..(hard_start + line.len()), full_w));
                break;
            }
            let mut best_word_end = None;
            let mut running_w = 0.0_f32;
            let mut trailing_ws_w = 0.0_f32;
            for (seg_idx, seg) in rest.split_word_bound_indices() {
                let end = seg_idx + seg.len();
                let seg_w = measure_shaped_text(seg, font_size);
                running_w += seg_w;
                if seg.bytes().all(|b| b == b' ' || b == b'\t') {
                    trailing_ws_w += seg_w;
                } else {
                    let trimmed_seg = seg.trim_end_matches([' ', '\t']);
                    trailing_ws_w = if trimmed_seg.len() == seg.len() {
                        0.0
                    } else {
                        measure_shaped_text(&seg[trimmed_seg.len()..], font_size)
                    };
                }
                let w = (running_w - trailing_ws_w).max(0.0);
                if w <= max_width && end > 0 {
                    best_word_end = Some(end);
                } else {
                    break;
                }
            }
            let break_rel = if let Some(word_end) = best_word_end {
                word_end
            } else {
                let mut g_end = 0;
                let mut g_running_w = 0.0_f32;
                for (gi, g_str) in rest.grapheme_indices(true) {
                    let cand = gi + g_str.len();
                    g_running_w += measure_shaped_text(g_str, font_size);
                    if g_running_w <= max_width || g_end == 0 {
                        g_end = cand;
                    } else {
                        break;
                    }
                }
                g_end.max(1)
            };
            let slice = &rest[..break_rel];
            let width = measure_shaped_text(slice, font_size);
            result.push((
                (hard_start + cur_start)..(hard_start + cur_start + break_rel),
                width,
            ));
            cur_start += break_rel;
        }
        hard_start += raw_len + 1;
    }
    if text.is_empty() && result.is_empty() {
        result.push((0..0, 0.));
    }
    result
}

/// Hit-tests a logical `(x, y)` point against soft-wrapped lines produced by [`wrap_shaped_text`],
/// returning a grapheme-aligned UTF-8 byte offset in `text`.
pub fn hit_wrapped_text(
    text: &str,
    font_size: f32,
    max_width: f32,
    line_height: f32,
    x: f32,
    y: f32,
) -> usize {
    let lines = wrap_text_ranges(text, font_size, max_width);
    if lines.is_empty() {
        return 0;
    }
    let lh = if line_height.is_finite() && line_height > 0. {
        line_height
    } else {
        font_size * 1.5
    };
    let row = (y / lh).floor().max(0.) as usize;
    let (range, _) = &lines[row.min(lines.len() - 1)];
    let slice = &text[range.clone()];
    range.start + hit_grapheme(slice, font_size, x)
}

fn font() -> &'static Face<'static> {
    static FACE: OnceLock<Face<'static>> = OnceLock::new();
    FACE.get_or_init(|| Face::parse(FONT, 0).expect("embedded font"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FontGlyph {
    face: u8,
    id: GlyphId,
}

/// Return an immutable bundled face corresponding to [`ShapedGlyph::font_index`].
/// Fallback variable fonts are fixed at weight400, matching shaping/rendering.
pub fn font_face(index: u8) -> Option<&'static Face<'static>> {
    (index <= 4).then(|| glyph_face(index))
}

fn glyph_face(index: u8) -> &'static Face<'static> {
    fn parsed(bytes: &'static [u8]) -> Face<'static> {
        let mut face = Face::parse(bytes, 0).expect("embedded fallback font");
        face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), 400.);
        face
    }
    match index {
        1 => {
            static CJK: OnceLock<Face<'static>> = OnceLock::new();
            CJK.get_or_init(|| parsed(include_bytes!("../assets/fallback/NotoSansSC.ttf")))
        }
        2 => {
            static EMOJI: OnceLock<Face<'static>> = OnceLock::new();
            EMOJI
                .get_or_init(|| parsed(include_bytes!("../assets/fallback/FormaEmoji-Regular.ttf")))
        }
        3 => {
            static MONO_FACE: OnceLock<Face<'static>> = OnceLock::new();
            MONO_FACE.get_or_init(|| Face::parse(MONO, 0).expect("embedded monospace font"))
        }
        4 => {
            static BOLD_FACE: OnceLock<Face<'static>> = OnceLock::new();
            BOLD_FACE.get_or_init(|| Face::parse(BOLD, 0).expect("embedded bold font"))
        }
        _ => font(),
    }
}

// Advances are normalized to primary-font units so measuring, caret hit tests
// and both renderers sum the same values even when faces use different UPEM.
#[cfg(test)]
fn resolve_glyph(ch: char) -> (FontGlyph, f32) {
    resolve_glyph_with_table(ch, font_table())
}
fn resolve_glyph_with_table(ch: char, table: &FontTable) -> (FontGlyph, f32) {
    let code = ch as usize;
    if code < 1104 && table.present[code] {
        return (
            FontGlyph {
                face: table.primary,
                id: table.glyphs[code],
            },
            table.advances[code],
        );
    }
    // The family's own face first, then the shared chain. For the default family this is
    // the historical 0, 1, 2 order.
    for index in std::iter::once(table.primary).chain((0..=2).filter(|&i| i != table.primary)) {
        let face = glyph_face(index);
        if let Some(id) = face.glyph_index(ch) {
            let units = advance(face, id) * table.units_per_em as f32 / face.units_per_em() as f32;
            return (FontGlyph { face: index, id }, units);
        }
    }
    (
        FontGlyph {
            face: table.primary,
            id: table.glyphs[b'?' as usize],
        },
        table.advances[b'?' as usize],
    )
}

struct FontTable {
    primary: u8,
    glyphs: [GlyphId; 1104],
    present: [bool; 1104],
    advances: [f32; 1104],
    units_per_em: u16,
    ascender: f32,
    descender: f32,
    line_gap: f32,
}

fn font_table() -> &'static FontTable {
    font_table_for(FontFamily::Sans)
}

fn font_table_for(family: FontFamily) -> &'static FontTable {
    static SANS: OnceLock<FontTable> = OnceLock::new();
    static MONO_TABLE: OnceLock<FontTable> = OnceLock::new();
    static BOLD_TABLE: OnceLock<FontTable> = OnceLock::new();
    let cell = match family {
        FontFamily::Sans => &SANS,
        FontFamily::Mono => &MONO_TABLE,
        FontFamily::SansBold => &BOLD_TABLE,
    };
    cell.get_or_init(|| {
        let primary = family.primary();
        let face = glyph_face(primary);
        let fallback = face
            .glyph_index('?')
            .or_else(|| face.glyph_index(' '))
            .unwrap_or(GlyphId(0));
        let mut glyphs = [fallback; 1104];
        let mut advances = [0.; 1104];
        let mut present = [false; 1104];
        for code in 0..1104u32 {
            if let Some(ch) = char::from_u32(code) {
                let resolved = face.glyph_index(ch);
                present[code as usize] = resolved.is_some();
                let id = resolved.unwrap_or(fallback);
                glyphs[code as usize] = id;
                advances[code as usize] = face.glyph_hor_advance(id).unwrap_or(0) as f32;
            }
        }
        FontTable {
            primary,
            glyphs,
            present,
            advances,
            units_per_em: face.units_per_em(),
            ascender: face.ascender() as f32,
            descender: face.descender() as f32,
            line_gap: face.line_gap() as f32,
        }
    })
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

    fn distance_sq_to_segment(self, a: Self, b: Self) -> f32 {
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

struct Contours {
    #[cfg(test)]
    tolerance: f32,
    edges: Vec<(Point, Point)>,
    current: Point,
    start: Point,
    origin: Point,
    units_to_pixels: f32,
}

impl Contours {
    fn tolerance(&self) -> f32 {
        #[cfg(test)]
        {
            self.tolerance
        }
        #[cfg(not(test))]
        {
            CURVE_TOLERANCE
        }
    }
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

    fn cubic(&mut self, a: Point, b: Point, c: Point, d: Point, depth: u8) {
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
fn glyph(face: &Face<'_>, character: char) -> GlyphId {
    let code = character as usize;
    let table = font_table();
    if code < 1104 {
        table.glyphs[code]
    } else {
        face.glyph_index(character)
            .unwrap_or(table.glyphs[b'?' as usize])
    }
}

pub fn measure_line(value: &str, font_size: f32) -> [f32; 2] {
    measure_line_in(FontFamily::Sans, value, font_size)
}

/// [`measure_line`] for a chosen family.
pub fn measure_line_in(family: FontFamily, value: &str, font_size: f32) -> [f32; 2] {
    if !font_size.is_finite() || font_size <= 0. {
        return [0.; 2];
    }
    let table = font_table_for(family);
    let height =
        (table.ascender - table.descender + table.line_gap) * font_size / table.units_per_em as f32;
    [measure_text_in(family, value, font_size), height]
}

fn advance(face: &Face<'_>, glyph: GlyphId) -> f32 {
    face.glyph_hor_advance(glyph).unwrap_or(0) as f32
}

/// Single-line advance width in logical pixels. No complex shaping yet.
pub fn measure_text(text: &str, font_size: f32) -> f32 {
    measure_text_in(FontFamily::Sans, text, font_size)
}

/// [`measure_text`] for a chosen family: the exact sum the renderers advance by.
pub fn measure_text_in(family: FontFamily, text: &str, font_size: f32) -> f32 {
    if !font_size.is_finite() || font_size <= 0. {
        return 0.;
    }
    let table = font_table_for(family);
    let mut units = 0.;
    for c in text.chars() {
        units += resolve_glyph_with_table(c, table).1;
    }
    pixels_from_units(units, font_size, table.units_per_em)
}

/// Advance units to logical pixels. One definition, because the vector path sums its own
/// advances while resolving outlines and has to land on the same float as `measure_text`.
fn pixels_from_units(units: f32, font_size: f32, units_per_em: u16) -> f32 {
    units * font_size / units_per_em as f32
}

/// Find a caret boundary in one pass. Summing font units before converting to
/// pixels matches measure_text(prefix) without measuring every prefix again.
#[cfg(test)]
pub(crate) fn hit_character(text: &str, font_size: f32, x: f32) -> usize {
    let table = font_table();
    let mut units = 0.;
    let mut previous = 0.;
    for (offset, ch) in text.char_indices() {
        units += resolve_glyph_with_table(ch, table).1;
        let next = units * font_size / table.units_per_em as f32;
        if x < (previous + next) * 0.5 {
            return offset;
        }
        previous = next;
    }
    text.len()
}

/// Hit a user-perceived character using the same unshaped glyph advances as
/// measure_line, without exposing a caret inside an extended grapheme cluster.
pub(crate) fn hit_grapheme(text: &str, font_size: f32, x: f32) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    let table = font_table();
    let mut units = 0.;
    let mut previous = 0.;
    for (offset, cluster) in text.grapheme_indices(true) {
        for ch in cluster.chars() {
            units += resolve_glyph_with_table(ch, table).1;
        }
        let next = units * font_size / table.units_per_em as f32;
        if x < (previous + next) * 0.5 {
            return offset;
        }
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
    edges: Vec<(Point, Point)>,
    ids: Vec<(FontGlyph, f32)>,
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
fn glyph_ids(family: FontFamily, text: &str, ids: &mut Vec<(FontGlyph, f32)>) -> f32 {
    ids.clear();
    let mut units = 0.;
    let table = font_table_for(family);
    for ch in text.chars() {
        let resolved = resolve_glyph_with_table(ch, table);
        units += resolved.1;
        ids.push(resolved);
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

use std::cell::RefCell;

#[derive(Default)]
struct CpuRasterScratch {
    ids: Vec<(FontGlyph, f32)>,
    edges: Vec<(Point, Point)>,
    area: Vec<f32>,
    full: Vec<f32>,
}

thread_local! {
    static CPU_SCRATCH: RefCell<CpuRasterScratch> = RefCell::new(CpuRasterScratch::default());
}

/// Straight-alpha RGBA raster destination in logical/physical canvas space.
pub struct RasterCanvas<'a> {
    pub pixels: &'a mut [u8],
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub region: [u32; 4],
}

impl<'a> RasterCanvas<'a> {
    #[inline]
    pub fn full(pixels: &'a mut [u8], width: u32, height: u32, scale: f32) -> Self {
        Self {
            pixels,
            width,
            height,
            scale,
            region: [0, 0, width, height],
        }
    }

    #[inline]
    pub fn subregion(
        pixels: &'a mut [u8],
        width: u32,
        height: u32,
        scale: f32,
        region: [u32; 4],
    ) -> Self {
        Self {
            pixels,
            width,
            height,
            scale,
            region,
        }
    }
}

/// Draw a centered, single-line label into a straight-alpha RGBA pixel buffer.
/// `rect = [x, y, width, height]` and font size are logical pixels; `canvas.scale` is
/// device pixels per logical pixel. Coverage is clipped to both rect and canvas.
pub fn draw_text(
    canvas: &mut RasterCanvas<'_>,
    text: &str,
    font_size: f32,
    rect: [f32; 4],
    color: [u8; 4],
) {
    draw_text_in(FontFamily::Sans, canvas, text, font_size, rect, color);
}

/// [`draw_text`] for a chosen family.
pub fn draw_text_in(
    family: FontFamily,
    canvas: &mut RasterCanvas<'_>,
    text: &str,
    font_size: f32,
    rect: [f32; 4],
    color: [u8; 4],
) {
    draw_text_region(
        family,
        &mut RasterCanvas::full(canvas.pixels, canvas.width, canvas.height, canvas.scale),
        text,
        font_size,
        rect,
        color,
    );
}

/// Draw into a physical subregion while retaining global contour coordinates.
pub(crate) fn draw_text_region(
    family: FontFamily,
    canvas: &mut RasterCanvas<'_>,
    text: &str,
    font_size: f32,
    rect: [f32; 4],
    color: [u8; 4],
) {
    let pixels: &mut [u8] = canvas.pixels;
    let width = canvas.width;
    let height = canvas.height;
    let scale = canvas.scale;
    let region = canvas.region;
    let Some(required) = (region[2] as usize)
        .checked_mul(region[3] as usize)
        .and_then(|size| size.checked_mul(4))
    else {
        return;
    };
    if !valid_raster_region(pixels.len(), width, height, region)
        || pixels.len() < required
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
    let face = glyph_face(family.primary());
    let units_to_pixels = font_size * scale / face.units_per_em() as f32;
    let (mut ids, mut edges) = CPU_SCRATCH.with(|cell| {
        let mut s = cell.borrow_mut();
        (std::mem::take(&mut s.ids), std::mem::take(&mut s.edges))
    });
    let text_width = glyph_ids(family, text, &mut ids) * units_to_pixels;
    let origin = Point {
        x: (rect[0] + rect[2] * 0.5) * scale - text_width * 0.5,
        y: (rect[1] + rect[3] * 0.5) * scale
            + (face.ascender() as f32 + face.descender() as f32) * units_to_pixels * 0.5,
    };
    if !units_to_pixels.is_finite() || !origin.x.is_finite() || !origin.y.is_finite() {
        CPU_SCRATCH.with(|cell| {
            let mut s = cell.borrow_mut();
            s.ids = ids;
            s.edges = edges;
        });
        return;
    }
    edges.clear();
    let mut contours = Contours {
        #[cfg(test)]
        tolerance: CURVE_TOLERANCE,
        edges,
        current: origin,
        start: origin,
        origin,
        units_to_pixels,
    };
    for &(glyph, adv) in &ids {
        let resolved_face = glyph_face(glyph.face);
        contours.units_to_pixels = font_size * scale / resolved_face.units_per_em() as f32;
        resolved_face.outline_glyph(glyph.id, &mut contours);
        contours.origin.x += adv * units_to_pixels;
    }
    rasterize_region(pixels, width, height, region, &contours.edges, clip, color);
    CPU_SCRATCH.with(|cell| {
        let mut s = cell.borrow_mut();
        s.ids = ids;
        s.edges = contours.edges;
    });
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
    let xa = a.x + (ya - a.y) * slope;
    let xb = a.x + (yb - a.y) * slope;
    let area = crate::geometry::column_area(xa, xb, px);
    (if b.y > a.y { area } else { -area }) * (yb - ya)
}

/// Fill a closed contour set (physical-pixel edges) into straight-alpha RGBA with
/// exact area coverage under the nonzero rule, clipped to `clip` (physical
/// pixels, fractional edges scale the boundary column/row). Shared by text and
/// composed shapes on native and WASM.
#[cfg(test)]
pub(crate) fn rasterize(
    pixels: &mut [u8],
    width: u32,
    edges: &[(Point, Point)],
    clip: [f32; 4],
    color: [u8; 4],
) {
    if width == 0 {
        return;
    }
    let height = (pixels.len() / 4 / width as usize).min(u32::MAX as usize) as u32;
    rasterize_region(
        pixels,
        width,
        height,
        [0, 0, width, height],
        edges,
        clip,
        color,
    );
}

fn valid_raster_region(len: usize, width: u32, height: u32, region: [u32; 4]) -> bool {
    region[2] > 0
        && region[3] > 0
        && region[0]
            .checked_add(region[2])
            .is_some_and(|end| end <= width)
        && region[1]
            .checked_add(region[3])
            .is_some_and(|end| end <= height)
        && (region[2] as usize)
            .checked_mul(region[3] as usize)
            .and_then(|n| n.checked_mul(4))
            .is_some_and(|n| n <= len)
}

/// Edges/clip stay in global physical coordinates; only storage indexing is local.
/// Keep the original horizontal accumulation prefix to preserve f32 rounding.
pub(crate) fn rasterize_region(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    region: [u32; 4],
    edges: &[(Point, Point)],
    clip: [f32; 4],
    color: [u8; 4],
) {
    if !valid_raster_region(pixels.len(), width, height, region)
        || !clip.iter().all(|n| n.is_finite())
    {
        return;
    }
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
    let top = (bounds[1].max(clip[1]).floor().max(0.) as usize).max(region[1] as usize);
    let right = (bounds[2].min(clip[2]).ceil().max(0.) as usize).min(width as usize);
    let bottom =
        (bounds[3].min(clip[3]).ceil().max(0.) as usize).min((region[1] + region[3]) as usize);
    if left >= right || top >= bottom {
        return;
    }
    let columns = right - left;
    // Per row: exact per-pixel areas where an edge crosses the column, plus a
    // difference array for the full-height contribution of every column to the
    // left of the edge. Scratch is one clipped row, never a full canvas.
    let (mut area, mut full) = CPU_SCRATCH.with(|cell| {
        let mut s = cell.borrow_mut();
        (std::mem::take(&mut s.area), std::mem::take(&mut s.full))
    });
    if area.len() < columns {
        area.resize(columns, 0.);
    }
    if full.len() < columns + 1 {
        full.resize(columns + 1, 0.);
    }
    for y in top..bottom {
        let row_top = (y as f32).max(clip[1]);
        let row_bottom = (y as f32 + 1.).min(clip[3]);
        if row_top >= row_bottom {
            continue;
        }
        area[..columns].fill(0.);
        full[..columns + 1].fill(0.);
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
            let global_x = left + offset;
            if global_x < region[0] as usize || global_x >= (region[0] + region[2]) as usize {
                continue;
            }
            let px = global_x as f32;
            let horizontal = ((px + 1.).min(clip[2]) - px.max(clip[0])).clamp(0., 1.);
            let coverage = (running + area[offset]).abs().min(1.) * horizontal;
            if coverage <= 0. {
                continue;
            }
            let pixel =
                ((y - region[1] as usize) * region[2] as usize + global_x - region[0] as usize) * 4;
            crate::blend::blend(&mut pixels[pixel..pixel + 4], color, coverage);
        }
    }
    CPU_SCRATCH.with(|cell| {
        let mut s = cell.borrow_mut();
        s.area = area;
        s.full = full;
    });
}

#[cfg(test)]
mod tests;
