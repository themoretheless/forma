//! The bundled fonts and their glyph tables.

use super::*;

pub(super) const FONT: &[u8] = include_bytes!("../../assets/Ubuntu-Light.ttf");

pub(super) const MONO: &[u8] = include_bytes!("../../assets/mono/IBMPlexMono-Regular.ttf");

pub(super) const BOLD: &[u8] = include_bytes!("../../assets/bold/Ubuntu-Bold.ttf");

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
    pub(super) fn primary(self) -> u8 {
        match self {
            Self::Sans => 0,
            Self::Mono => 3,
            Self::SansBold => 4,
        }
    }
}

pub(super) fn font() -> &'static Face<'static> {
    static FACE: OnceLock<Face<'static>> = OnceLock::new();
    FACE.get_or_init(|| Face::parse(FONT, 0).expect("embedded font"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FontGlyph {
    pub(super) face: u8,
    pub(super) id: GlyphId,
}

/// Return an immutable bundled face corresponding to [`ShapedGlyph::font_index`].
/// Fallback variable fonts are fixed at weight400, matching shaping/rendering.
pub fn font_face(index: u8) -> Option<&'static Face<'static>> {
    (index <= 6).then(|| glyph_face(index))
}

pub(super) fn glyph_face(index: u8) -> &'static Face<'static> {
    fn parsed(bytes: &'static [u8]) -> Face<'static> {
        let mut face = Face::parse(bytes, 0).expect("embedded fallback font");
        face.set_variation(ttf_parser::Tag::from_bytes(b"wght"), 400.);
        face
    }
    match index {
        1 => {
            static CJK: OnceLock<Face<'static>> = OnceLock::new();
            CJK.get_or_init(|| parsed(include_bytes!("../../assets/fallback/NotoSansSC.ttf")))
        }
        2 => {
            static EMOJI: OnceLock<Face<'static>> = OnceLock::new();
            EMOJI.get_or_init(|| {
                parsed(include_bytes!(
                    "../../assets/fallback/FormaEmoji-Regular.ttf"
                ))
            })
        }
        3 => {
            static MONO_FACE: OnceLock<Face<'static>> = OnceLock::new();
            MONO_FACE.get_or_init(|| Face::parse(MONO, 0).expect("embedded monospace font"))
        }
        5 => {
            static HEBREW: OnceLock<Face<'static>> = OnceLock::new();
            HEBREW
                .get_or_init(|| parsed(include_bytes!("../../assets/fallback/NotoSansHebrew.ttf")))
        }
        6 => {
            static ARABIC: OnceLock<Face<'static>> = OnceLock::new();
            ARABIC
                .get_or_init(|| parsed(include_bytes!("../../assets/fallback/NotoSansArabic.ttf")))
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
pub(super) fn resolve_glyph(ch: char) -> (FontGlyph, f32) {
    resolve_glyph_with_table(ch, font_table())
}

/// The glyph of a character in the form `arabic::joined` chose; when no face has that
/// presentation form, the letter's own glyph.
pub(super) fn resolve_form_with_table(form: char, ch: char, table: &FontTable) -> (FontGlyph, f32) {
    if form != ch {
        for index in [6, table.primary] {
            let face = glyph_face(index);
            if let Some(id) = face.glyph_index(form) {
                let units =
                    advance(face, id) * table.units_per_em as f32 / face.units_per_em() as f32;
                return (FontGlyph { face: index, id }, units);
            }
        }
    }
    resolve_glyph_with_table(ch, table)
}

pub(super) fn resolve_glyph_with_table(ch: char, table: &FontTable) -> (FontGlyph, f32) {
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
    // the historical 0, 1, 2 order; the Hebrew and Arabic faces come last, so nothing that
    // resolved before resolves differently.
    for index in std::iter::once(table.primary)
        .chain([0, 1, 2, 5, 6].into_iter().filter(|&i| i != table.primary))
    {
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

pub(super) struct FontTable {
    pub(super) primary: u8,
    pub(super) glyphs: [GlyphId; 1104],
    pub(super) present: [bool; 1104],
    pub(super) advances: [f32; 1104],
    pub(super) units_per_em: u16,
    pub(super) ascender: f32,
    pub(super) descender: f32,
    pub(super) line_gap: f32,
}

pub(super) fn font_table() -> &'static FontTable {
    font_table_for(FontFamily::Sans)
}

pub(super) fn font_table_for(family: FontFamily) -> &'static FontTable {
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
