//! Shaping: ligatures, kerning and glyph placement.

use super::*;

/// Shaped glyph placement produced by [`shape_text`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Resolved font glyph identifier (`GlyphId(0)` when synthesized by the fallback chain).
    pub glyph_id: u16,
    /// Bundled face: 0=Ubuntu, 1=NotoSansSC, 2=NotoEmoji, 3=IBM Plex Mono, 4=Ubuntu Bold,
    /// 5=Noto Sans Hebrew, 6=Noto Sans Arabic;
    /// see [`font_face`].
    /// Shaping itself only produces 0..=2, 5 and 6: the monospace face is not shaped.
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

pub(super) fn match_standard_ligature(rest: &str) -> Option<(usize, char)> {
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

pub(super) fn kern_pair_units(face: &Face<'_>, left: GlyphId, right: GlyphId) -> f32 {
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
