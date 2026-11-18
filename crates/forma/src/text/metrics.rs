//! Line and text measurement.

use super::*;

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

pub(super) fn advance(face: &Face<'_>, glyph: GlyphId) -> f32 {
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
    if arabic::has_arabic(text) {
        for (form, c) in arabic::joined(text).into_iter().zip(text.chars()) {
            units += resolve_form_with_table(form, c, table).1;
        }
    } else {
        for c in text.chars() {
            units += resolve_glyph_with_table(c, table).1;
        }
    }
    pixels_from_units(units, font_size, table.units_per_em)
}

/// Advance units to logical pixels. One definition, because the vector path sums its own
/// advances while resolving outlines and has to land on the same float as `measure_text`.
pub(super) fn pixels_from_units(units: f32, font_size: f32, units_per_em: u16) -> f32 {
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
