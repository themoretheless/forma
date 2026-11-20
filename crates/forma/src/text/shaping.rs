//! OpenType-aware text shaping: `GSUB` substitutions/ligatures, `GPOS` kerning/mark anchors,
//! contextual Arabic joining, BiDi visual run ordering, and fallback glyph resolution.

use super::*;

/// Shaped glyph placement produced by [`shape_text`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Resolved font glyph identifier (`GlyphId(0)` when synthesized by the fallback chain).
    pub glyph_id: u16,
    /// Bundled face: 0=Ubuntu, 1=NotoSansSC, 2=FormaEmoji, 3=IBM Plex Mono, 4=Ubuntu Bold,
    /// 5=Noto Sans Hebrew, 6=Noto Sans Arabic; see [`font_face`].
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
    arabic_form: arabic::ArabicForm,
    ch: char,
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

#[inline]
fn scripts_for(font_index: u8, rtl: bool) -> &'static [[u8; 4]] {
    const ARAB: &[[u8; 4]] = &[*b"arab", *b"DFLT"];
    const HEBR: &[[u8; 4]] = &[*b"hebr", *b"DFLT"];
    const CJK: &[[u8; 4]] = &[*b"hani", *b"kana", *b"DFLT"];
    const RTL: &[[u8; 4]] = &[*b"arab", *b"hebr", *b"DFLT"];
    const LATN: &[[u8; 4]] = &[*b"latn", *b"DFLT"];
    match font_index {
        6 => ARAB,
        5 => HEBR,
        1 => CJK,
        _ if rtl => RTL,
        _ => LATN,
    }
}

#[inline]
fn face_scale(primary_upem: u16, font_index: u8) -> f32 {
    let face = glyph_face(font_index);
    let sub_upem = face.units_per_em();
    if sub_upem == 0 {
        1.0
    } else {
        primary_upem as f32 / sub_upem as f32
    }
}

fn shape_logical_clusters(
    family: FontFamily,
    text: &str,
    dir: TextDirection,
) -> (Vec<LogicalCluster>, u16, bool) {
    let table = font_table_for(family);
    let primary_face = glyph_face(table.primary);
    let base_rtl = resolve_direction(text, dir) == ResolvedDirection::Rtl;
    let arabic_present = arabic::has_arabic(text);
    let chars: Vec<char> = text.chars().collect();
    let arab_forms = arabic_present.then(|| arabic::arabic_forms(&chars));
    let arab_pfa = arabic_present.then(|| arabic::joined(text));

    let mut clusters: Vec<LogicalCluster> = Vec::with_capacity(chars.len());
    let mut byte_idx = 0usize;
    let mut char_idx = 0usize;
    while char_idx < chars.len() {
        let ch = chars[char_idx];
        let ch_len = ch.len_utf8();
        let zero_mod = is_zero_width_modifier(ch);
        let emoji_mod = is_emoji_modifier(ch);
        let base_for_class = resolve_glyph_with_table(ch, table);
        let ot_mark = opentype::OpenTypeTables::for_bundled(base_for_class.0.face)
            .is_some_and(|ot| ot.is_mark_glyph(base_for_class.0.id.0));
        let combining = is_combining_mark(ch) || zero_mod || emoji_mod || ot_mark;
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

        let aform = arab_forms
            .as_ref()
            .map_or(arabic::ArabicForm::None, |f| f[char_idx]);
        let pfa = arab_pfa.as_ref().map(|v| v[char_idx]);
        if pfa == Some(arabic::ABSORBED) {
            byte_idx += ch_len;
            char_idx += 1;
            continue;
        }
        let is_lam_alef = pfa.is_some_and(|c| matches!(c, '\u{FEF5}'..='\u{FEFC}'));
        let is_fallback = primary_face.glyph_index(ch).is_none();

        let (resolved_glyph, mut raw_adv) = if is_lam_alef {
            resolve_form_with_table(pfa.unwrap(), ch, table)
        } else if !matches!(aform, arabic::ArabicForm::None) {
            let base = resolve_glyph_with_table(ch, table);
            let ot = opentype::OpenTypeTables::for_bundled(base.0.face);
            let tag = match aform {
                arabic::ArabicForm::Isolated => Some(*b"isol"),
                arabic::ArabicForm::Final => Some(*b"fina"),
                arabic::ArabicForm::Initial => Some(*b"init"),
                arabic::ArabicForm::Medial => Some(*b"medi"),
                arabic::ArabicForm::None => None,
            };
            if let (Some(ot), Some(tag)) = (ot, tag) {
                if let Some(sub_gid) = ot.single_subst(&[*b"arab", *b"DFLT"], tag, base.0.id.0) {
                    let g = GlyphId(sub_gid);
                    let adv = advance(glyph_face(base.0.face), g)
                        * face_scale(table.units_per_em, base.0.face);
                    (
                        FontGlyph {
                            id: g,
                            face: base.0.face,
                        },
                        adv,
                    )
                } else if let Some(pfa_ch) = pfa {
                    resolve_form_with_table(pfa_ch, ch, table)
                } else {
                    base
                }
            } else {
                base
            }
        } else {
            resolve_glyph_with_table(ch, table)
        };

        let synthetic = (is_fallback && resolved_glyph.face == 0) || zero_mod;
        if synthetic {
            raw_adv = table.units_per_em as f32 * 0.6;
        }
        let is_zero_adv = combining || prev_was_zwj || prev_was_ri;
        clusters.push(LogicalCluster {
            glyph_id: if zero_mod {
                GlyphId(0)
            } else {
                resolved_glyph.id
            },
            font_index: resolved_glyph.face,
            synthetic,
            cluster: byte_idx,
            advance_units: if is_zero_adv { 0. } else { raw_adv },
            x_offset_units: 0.,
            y_offset_units: 0.,
            rtl,
            combining: is_zero_adv,
            fallback: is_fallback,
            arabic_form: aform,
            ch,
        });
        byte_idx += ch_len;
        char_idx += 1;
    }

    // Apply `GSUB` ligature substitutions (`ccmp`, `rlig`, and for proportional faces `liga`),
    // falling back to standard Latin `ff/fi/fl/ffi/ffl` ligatures when `GSUB` has no match.
    let mut i = 0;
    while i < clusters.len() {
        if clusters[i].synthetic || clusters[i].combining {
            i += 1;
            continue;
        }
        let fidx = clusters[i].font_index;
        let run_rtl = clusters[i].rtl;
        let mut run_end = i + 1;
        while run_end < clusters.len()
            && clusters[run_end].font_index == fidx
            && clusters[run_end].rtl == run_rtl
            && !clusters[run_end].synthetic
        {
            run_end += 1;
        }
        let slice_len = run_end - i;
        let gids: Vec<u16> = clusters[i..run_end].iter().map(|c| c.glyph_id.0).collect();
        let marks: Vec<bool> = clusters[i..run_end].iter().map(|c| c.combining).collect();
        let features: &[[u8; 4]] = if family == FontFamily::Mono {
            &[*b"ccmp", *b"rlig"]
        } else {
            &[*b"ccmp", *b"rlig", *b"liga"]
        };
        let mut substituted = false;
        if let Some(ot) = opentype::OpenTypeTables::for_bundled(fidx) {
            if let Some((lig_gid, consumed)) =
                ot.ligature_subst(scripts_for(fidx, run_rtl), features, &gids, &marks)
            {
                let gid = GlyphId(lig_gid);
                clusters[i].glyph_id = gid;
                clusters[i].advance_units =
                    advance(glyph_face(fidx), gid) * face_scale(table.units_per_em, fidx);
                // Remove consumed component indices (backwards to preserve indices; keep intervening marks).
                for &rel in consumed.iter().skip(1).rev() {
                    clusters.remove(i + rel);
                }
                substituted = true;
            }
        }
        if !substituted && family != FontFamily::Mono && fidx == table.primary && slice_len >= 2 {
            let start_byte = clusters[i].cluster;
            if let Some((lig_len, lig_char)) = match_standard_ligature(&text[start_byte..]) {
                let end_byte = start_byte + lig_len;
                if let Some(lig_gid) = primary_face.glyph_index(lig_char) {
                    let count = clusters[i..run_end]
                        .iter()
                        .take_while(|c| c.cluster < end_byte)
                        .count();
                    if count >= 2
                        && clusters[i..i + count]
                            .iter()
                            .all(|c| !c.combining && c.font_index == fidx)
                    {
                        clusters[i].glyph_id = lig_gid;
                        clusters[i].advance_units = advance(primary_face, lig_gid);
                        for _ in 1..count {
                            clusters.remove(i + 1);
                        }
                    }
                }
            }
        }
        let _ = clusters[i].arabic_form;
        i += 1;
    }

    // Resolve neutral/whitespace BiDi characters against adjacent strong characters & paragraph direction.
    let len = clusters.len();
    for idx in 0..len {
        let ch = clusters[idx].ch;
        if !is_rtl_char(ch)
            && !is_strong_ltr_char(ch)
            && !clusters[idx].combining
            && !ch.is_ascii_digit()
        {
            let prev_rtl = (0..idx)
                .rev()
                .find(|&j| {
                    let cj = clusters[j].ch;
                    is_rtl_char(cj) || is_strong_ltr_char(cj) || cj.is_ascii_digit()
                })
                .map(|j| clusters[j].rtl);
            let next_rtl = ((idx + 1)..len)
                .find(|&j| {
                    let cj = clusters[j].ch;
                    is_rtl_char(cj) || is_strong_ltr_char(cj) || cj.is_ascii_digit()
                })
                .map(|j| clusters[j].rtl);
            clusters[idx].rtl = match (prev_rtl, next_rtl) {
                (Some(true), Some(true)) => true,
                (Some(false), Some(false)) => false,
                _ => base_rtl,
            };
        }
    }

    // Apply `GPOS` horizontal kerning (`kern`) with fallback to legacy `kern` table.
    let mut prev_base: Option<usize> = None;
    for idx in 0..clusters.len() {
        if clusters[idx].combining {
            continue;
        }
        if let Some(prev) = prev_base {
            if clusters[prev].rtl == clusters[idx].rtl
                && clusters[prev].font_index == clusters[idx].font_index
                && !clusters[prev].synthetic
                && !clusters[idx].synthetic
                && family != FontFamily::Mono
            {
                let fidx = clusters[idx].font_index;
                let s = face_scale(table.units_per_em, fidx);
                let gpos_adj = opentype::OpenTypeTables::for_bundled(fidx).and_then(|ot| {
                    ot.has_gpos_kern().then(|| {
                        ot.pair_kern(
                            scripts_for(fidx, clusters[idx].rtl),
                            clusters[prev].glyph_id.0,
                            clusters[idx].glyph_id.0,
                        )
                    })?
                });
                if let Some(adj) = gpos_adj {
                    let adv1 = f32::from(adj.x_advance_1) * s;
                    let xp1 = f32::from(adj.x_placement_1) * s;
                    let adv2 = f32::from(adj.x_advance_2) * s;
                    let xp2 = f32::from(adj.x_placement_2) * s;
                    if adv1 != 0. {
                        clusters[prev].advance_units =
                            (clusters[prev].advance_units + adv1).max(0.);
                    }
                    if xp1 != 0. {
                        clusters[prev].x_offset_units += xp1;
                    }
                    if adv2 != 0. {
                        clusters[idx].advance_units = (clusters[idx].advance_units + adv2).max(0.);
                    }
                    if xp2 != 0. {
                        clusters[idx].x_offset_units += xp2;
                    }
                } else {
                    let pair_face = glyph_face(fidx);
                    let kern =
                        kern_pair_units(pair_face, clusters[prev].glyph_id, clusters[idx].glyph_id)
                            * s;
                    if kern != 0. {
                        clusters[prev].advance_units =
                            (clusters[prev].advance_units + kern).max(0.);
                    }
                }
            }
        }
        prev_base = Some(idx);
    }

    // Apply `GPOS` `mark` / `mkmk` anchors for combining marks, with geometric fallback.
    let mut last_base_idx: Option<usize> = None;
    let mut last_mark_idx: Option<usize> = None;
    for idx in 0..clusters.len() {
        let c = &clusters[idx];
        if !c.combining {
            last_base_idx = Some(idx);
            last_mark_idx = None;
            continue;
        }
        let ch = c.ch;
        let zero_mod = is_zero_width_modifier(ch);
        let mark_like = is_combining_mark(ch) || is_emoji_modifier(ch);
        if zero_mod || !mark_like || c.synthetic {
            continue;
        }
        let fidx = c.font_index;
        let rtl = c.rtl;
        let gid = c.glyph_id.0;
        let s = face_scale(table.units_per_em, fidx);
        let ot = opentype::OpenTypeTables::for_bundled(fidx);
        let scripts = scripts_for(fidx, rtl);

        let attached_mkmk = last_mark_idx.and_then(|pm| {
            let prev_m = &clusters[pm];
            if prev_m.font_index == fidx && !prev_m.synthetic {
                let delta = ot.and_then(|o| o.mark_to_mark(scripts, prev_m.glyph_id.0, gid))?;
                Some((
                    prev_m.x_offset_units + f32::from(delta.x) * s,
                    prev_m.y_offset_units + f32::from(delta.y) * s,
                ))
            } else {
                None
            }
        });
        let attached = attached_mkmk.or_else(|| {
            let bi = last_base_idx?;
            let base = &clusters[bi];
            if base.font_index == fidx && !base.synthetic {
                let delta = ot.and_then(|o| o.mark_to_base(scripts, base.glyph_id.0, gid))?;
                Some((
                    base.x_offset_units + f32::from(delta.x) * s - base.advance_units,
                    base.y_offset_units + f32::from(delta.y) * s,
                ))
            } else {
                None
            }
        });
        if let Some((ox, oy)) = attached {
            clusters[idx].x_offset_units = ox;
            clusters[idx].y_offset_units = oy;
        } else {
            let base_adv = last_base_idx.map_or(table.units_per_em as f32 * 0.5, |bi| {
                clusters[bi].advance_units
            });
            clusters[idx].x_offset_units = -base_adv * 0.5;
            clusters[idx].y_offset_units = if is_combining_mark(ch) {
                table.ascender * 0.15
            } else {
                0.
            };
        }
        last_mark_idx = Some(idx);
    }

    (clusters, table.units_per_em, base_rtl)
}

/// Shapes a single line of text in [`FontFamily::Sans`] using [`TextDirection::Ltr`].
pub fn shape_text(text: &str, font_size: f32) -> Vec<ShapedGlyph> {
    shape_text_with_dir(FontFamily::Sans, text, font_size, TextDirection::Ltr)
}

/// Shapes a single line of text using the specified font family.
pub fn shape_text_in(family: FontFamily, text: &str, font_size: f32) -> Vec<ShapedGlyph> {
    shape_text_with_dir(family, text, font_size, TextDirection::Ltr)
}

/// Shapes a single line of text with explicit paragraph direction (`Ltr`, `Rtl`, or `Auto`).
pub fn shape_text_with_dir(
    family: FontFamily,
    text: &str,
    font_size: f32,
    dir: TextDirection,
) -> Vec<ShapedGlyph> {
    if !font_size.is_finite() || font_size <= 0. || text.is_empty() {
        return Vec::new();
    }
    let (clusters, units_per_em, base_rtl) = shape_logical_clusters(family, text, dir);
    let scale = font_size / units_per_em as f32;

    let mut runs: Vec<(bool, usize, usize)> = Vec::new();
    let mut run_start = 0;
    while run_start < clusters.len() {
        let run_rtl = clusters[run_start].rtl;
        let mut run_end = run_start + 1;
        while run_end < clusters.len() && clusters[run_end].rtl == run_rtl {
            run_end += 1;
        }
        runs.push((run_rtl, run_start, run_end));
        run_start = run_end;
    }

    let mut visual: Vec<ShapedGlyph> = Vec::with_capacity(clusters.len());
    let push_glyph = |c: &LogicalCluster, visual: &mut Vec<ShapedGlyph>| {
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
    };

    let ordered: Box<dyn Iterator<Item = (bool, usize, usize)>> = if base_rtl {
        Box::new(runs.into_iter().rev())
    } else {
        Box::new(runs.into_iter())
    };
    for (run_rtl, start, end) in ordered {
        let slice = &clusters[start..end];
        if !run_rtl {
            for c in slice {
                push_glyph(c, &mut visual);
            }
        } else {
            // Keep combining marks attached after their base glyph while reversing base order.
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
                    push_glyph(c, &mut visual);
                }
            }
        }
    }
    visual
}

/// Measures the total horizontal advance of shaped text in [`FontFamily::Sans`].
pub fn measure_shaped_text(text: &str, font_size: f32) -> f32 {
    measure_shaped_text_in(FontFamily::Sans, text, font_size)
}

/// Measures the total horizontal advance of shaped text in `family`, matching [`shape_text_in`] exactly.
pub fn measure_shaped_text_in(family: FontFamily, text: &str, font_size: f32) -> f32 {
    if !font_size.is_finite() || font_size <= 0. || text.is_empty() {
        return 0.;
    }
    let (clusters, units_per_em, _) = shape_logical_clusters(family, text, TextDirection::Ltr);
    let scale = font_size / units_per_em as f32;
    let total: f32 = clusters.iter().map(|c| c.advance_units).sum();
    total * scale
}
