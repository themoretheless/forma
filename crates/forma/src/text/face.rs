//! Font parsing, variation selection, metrics, kerning, and unhinted vector outlines via `skrifa`.

use skrifa::instance::{Location, Size};
use skrifa::outline::pen::PathStyle;
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::raw::tables::kern::SubtableKind;
use skrifa::raw::tables::mvar::tags::{CPHT, HASC, HCLA, HCLD, HDSC, HLGP, XHGT};
use skrifa::raw::tables::os2::SelectionFlags;
use skrifa::{FontRef, GlyphId as SkrifaGlyphId, MetadataProvider, Tag};

/// Glyph identifier within a parsed font face.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphId(pub u16);

/// Contour sink receiving vector commands from [`FontFace::outline_glyph`].
pub trait OutlineBuilder {
    /// Start a new contour at `(x, y)` in font design units.
    fn move_to(&mut self, x: f32, y: f32);
    /// Draw a line segment to `(x, y)`.
    fn line_to(&mut self, x: f32, y: f32);
    /// Draw a quadratic Bezier segment with control `(x1, y1)` and endpoint `(x, y)`.
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32);
    /// Draw a cubic Bezier segment with controls `(x1, y1)`, `(x2, y2)` and endpoint `(x, y)`.
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32);
    /// Close the active contour.
    fn close(&mut self);
}

struct OutlinePenAdapter<'a> {
    builder: &'a mut dyn OutlineBuilder,
    emitted: bool,
}

impl OutlinePen for OutlinePenAdapter<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.emitted = true;
        self.builder.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.emitted = true;
        self.builder.line_to(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.emitted = true;
        self.builder.quad_to(cx0, cy0, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.emitted = true;
        self.builder.curve_to(cx0, cy0, cx1, cy1, x, y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

/// Immutable OpenType/TrueType font face backed by `skrifa`.
#[derive(Clone)]
pub struct FontFace<'a> {
    font: FontRef<'a>,
    location: Location,
}

/// Backwards-compatible alias for [`FontFace`].
pub type Face<'a> = FontFace<'a>;

impl<'a> FontFace<'a> {
    /// Parses a font or collection member at `index`.
    pub fn parse(bytes: &'a [u8], index: u32) -> Option<Self> {
        let font = FontRef::from_index(bytes, index).ok()?;
        // Reject malformed fonts lacking required `head` or `maxp` tables.
        font.head().ok()?;
        font.maxp().ok()?;
        Some(Self {
            font,
            location: Location::default(),
        })
    }

    /// Selects a single design-space variation coordinate by 4-byte OpenType axis tag.
    pub fn set_variation(&mut self, tag: [u8; 4], value: f32) -> Option<()> {
        let target = Tag::new(&tag);
        let axes = self.font.axes();
        if !axes.iter().any(|axis| axis.tag() == target) {
            return None;
        }
        self.location = axes.location([(target, value)]);
        Some(())
    }

    /// Design units per EM square.
    pub fn units_per_em(&self) -> u16 {
        self.font.head().map(|h| h.units_per_em()).unwrap_or(1000)
    }

    /// Total number of glyphs in the face (`maxp.numGlyphs`).
    pub fn number_of_glyphs(&self) -> u16 {
        self.font.maxp().map(|m| m.num_glyphs()).unwrap_or(0)
    }

    fn apply_mvar(&self, value: i16, tag: Tag) -> i16 {
        let coords = self.location.coords();
        if coords.is_empty() {
            return value;
        }
        let Ok(mvar) = self.font.mvar() else {
            return value;
        };
        let delta = mvar
            .metric_delta(tag, coords)
            .map(|d| d.to_f64())
            .unwrap_or(0.0);
        let adjusted = (f64::from(value) + delta).round();
        adjusted.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
    }

    /// Horizontal typographic/head ascender in design units.
    pub fn ascender(&self) -> i16 {
        if let Ok(os2) = self.font.os2() {
            if os2
                .fs_selection()
                .contains(SelectionFlags::USE_TYPO_METRICS)
            {
                return self.apply_mvar(os2.s_typo_ascender(), HASC);
            }
        }
        let value = self.font.hhea().map(|h| h.ascender().to_i16()).unwrap_or(0);
        if value != 0 {
            return value;
        }
        if let Ok(os2) = self.font.os2() {
            let typo = os2.s_typo_ascender();
            if typo != 0 {
                return self.apply_mvar(typo, HASC);
            }
            let win = i16::try_from(os2.us_win_ascent()).unwrap_or(i16::MAX);
            return self.apply_mvar(win, HCLA);
        }
        0
    }

    /// Horizontal typographic/head descender in design units.
    pub fn descender(&self) -> i16 {
        if let Ok(os2) = self.font.os2() {
            if os2
                .fs_selection()
                .contains(SelectionFlags::USE_TYPO_METRICS)
            {
                return self.apply_mvar(os2.s_typo_descender(), HDSC);
            }
        }
        let value = self
            .font
            .hhea()
            .map(|h| h.descender().to_i16())
            .unwrap_or(0);
        if value != 0 {
            return value;
        }
        if let Ok(os2) = self.font.os2() {
            let typo = os2.s_typo_descender();
            if typo != 0 {
                return self.apply_mvar(typo, HDSC);
            }
            let win = i32::from(os2.us_win_descent());
            let neg = (-win).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            return self.apply_mvar(neg, HCLD);
        }
        0
    }

    /// Recommended line gap in design units.
    pub fn line_gap(&self) -> i16 {
        if let Ok(os2) = self.font.os2() {
            if os2
                .fs_selection()
                .contains(SelectionFlags::USE_TYPO_METRICS)
            {
                return self.apply_mvar(os2.s_typo_line_gap(), HLGP);
            }
        }
        let Ok(hhea) = self.font.hhea() else {
            return 0;
        };
        let mut value = hhea.line_gap().to_i16();
        if hhea.ascender().to_i16() == 0 || hhea.descender().to_i16() == 0 {
            if let Ok(os2) = self.font.os2() {
                if os2.s_typo_ascender() != 0 || os2.s_typo_descender() != 0 {
                    value = self.apply_mvar(os2.s_typo_line_gap(), HLGP);
                } else {
                    value = 0;
                }
            }
        }
        value
    }

    /// Face x-height (`OS/2` v2+) in design units, if present.
    pub fn x_height(&self) -> Option<i16> {
        let os2 = self.font.os2().ok()?;
        os2.sx_height().map(|v| self.apply_mvar(v, XHGT))
    }

    /// Face cap-height (`OS/2` v2+) in design units, if present.
    pub fn capital_height(&self) -> Option<i16> {
        let os2 = self.font.os2().ok()?;
        os2.s_cap_height().map(|v| self.apply_mvar(v, CPHT))
    }

    /// Maps a Unicode scalar to a non-`.notdef` glyph identifier via `cmap`.
    pub fn glyph_index(&self, code_point: char) -> Option<GlyphId> {
        let gid = self.font.charmap().map(code_point)?;
        let raw = u16::try_from(gid.to_u32()).ok()?;
        (raw != 0).then_some(GlyphId(raw))
    }

    /// Visits every mapped non-zero Unicode code point in the active `cmap`.
    pub fn codepoints(&self, mut f: impl FnMut(u32)) {
        for (cp, gid) in self.font.charmap().mappings() {
            if gid != SkrifaGlyphId::NOTDEF {
                f(cp);
            }
        }
    }

    /// Horizontal advance in design units (`hmtx` + `HVAR`/`gvar` when varied).
    pub fn glyph_hor_advance(&self, glyph: GlyphId) -> Option<u16> {
        if glyph.0 >= self.number_of_glyphs() {
            return None;
        }
        let raw_gid = SkrifaGlyphId::from(glyph.0);
        let mut advance = f32::from(self.font.hmtx().ok()?.advance(raw_gid)?);
        let coords = self.location.coords();
        if !coords.is_empty() {
            if let Ok(hvar) = self.font.hvar() {
                if let Ok(delta) = hvar.advance_width_delta(raw_gid, coords) {
                    advance += delta.to_f64() as f32 + 0.5;
                }
            } else if let (Ok(loca), Ok(glyf), Ok(gvar)) =
                (self.font.loca(None), self.font.glyf(), self.font.gvar())
            {
                if let Ok(Some(deltas)) = gvar.phantom_point_deltas(&glyf, &loca, coords, raw_gid) {
                    advance += deltas[1].x.to_f64() as f32 + 0.5;
                }
            }
        }
        if !advance.is_finite() || advance < 0.0 || advance > f32::from(u16::MAX) {
            None
        } else {
            Some(advance as u16)
        }
    }

    /// Vertical advance in design units (`vmtx` + `VVAR`/`gvar` when varied).
    pub fn glyph_ver_advance(&self, glyph: GlyphId) -> Option<u16> {
        if glyph.0 >= self.number_of_glyphs() {
            return None;
        }
        let raw_gid = SkrifaGlyphId::from(glyph.0);
        let mut advance = f32::from(self.font.vmtx().ok()?.advance(raw_gid)?);
        let coords = self.location.coords();
        if !coords.is_empty() {
            if let Ok(vvar) = self.font.vvar() {
                if let Ok(delta) = vvar.advance_height_delta(raw_gid, coords) {
                    advance += delta.to_f64() as f32 + 0.5;
                }
            } else if let (Ok(loca), Ok(glyf), Ok(gvar)) =
                (self.font.loca(None), self.font.glyf(), self.font.gvar())
            {
                if let Ok(Some(deltas)) = gvar.phantom_point_deltas(&glyf, &loca, coords, raw_gid) {
                    advance += deltas[3].y.to_f64() as f32 + 0.5;
                }
            }
        }
        if !advance.is_finite() || advance < 0.0 || advance > f32::from(u16::MAX) {
            None
        } else {
            Some(advance as u16)
        }
    }

    /// Emits unscaled, unhinted vector contour commands in design units.
    ///
    /// Returns `None` when the glyph is missing or contains no outline segments.
    pub fn outline_glyph(&self, glyph: GlyphId, builder: &mut dyn OutlineBuilder) -> Option<()> {
        let outline = self
            .font
            .outline_glyphs()
            .get(SkrifaGlyphId::from(glyph.0))?;
        let settings = DrawSettings::unhinted(Size::unscaled(), &self.location)
            .with_path_style(PathStyle::HarfBuzz);
        let mut adapter = OutlinePenAdapter {
            builder,
            emitted: false,
        };
        outline.draw(settings, &mut adapter).ok()?;
        adapter.emitted.then_some(())
    }

    /// Legacy horizontal kerning adjustment from the SFNT `kern` table in design units.
    pub(super) fn kern_pair_units(&self, left: GlyphId, right: GlyphId) -> f32 {
        let Ok(kern) = self.font.kern() else {
            return 0.0;
        };
        let left_gid = SkrifaGlyphId::from(left.0);
        let right_gid = SkrifaGlyphId::from(right.0);
        let mut total = 0i32;
        for subtable in kern.subtables().flatten() {
            if !subtable.is_horizontal() || subtable.is_cross_stream() || subtable.is_variable() {
                continue;
            }
            let delta = match subtable.kind() {
                Ok(SubtableKind::Format0(st)) => st.kerning(left_gid, right_gid),
                Ok(SubtableKind::Format2(st)) => st.kerning(left_gid, right_gid),
                Ok(SubtableKind::Format3(st)) => st.kerning(left_gid, right_gid),
                _ => None,
            };
            if let Some(val) = delta {
                total += val;
            }
        }
        total as f32
    }
}
