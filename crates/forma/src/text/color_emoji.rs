//! Color emoji support: OpenType `COLR`/`CPAL` v0 tables plus semantic multi-tone palette
//! synthesis for monochrome outline emoji (`FormaEmoji`, face 2), honoring VS15/VS16 and
//! Fitzpatrick skin-tone modifiers (`U+1F3FB..=U+1F3FF`).

use super::opentype::{find_table, u16_be, u32_be};
use super::outline::Point;

#[inline]
fn scale_alpha(color: [u8; 4], alpha_scale: u8) -> [u8; 4] {
    [
        color[0],
        color[1],
        color[2],
        ((u16::from(color[3]) * u16::from(alpha_scale) + 127) / 255) as u8,
    ]
}

/// Resolves OpenType `COLR` v0 + `CPAL` v0 layers `(layer_glyph_id, rgba)` for `glyph_id`.
/// Returns `None` if `sfnt` lacks `COLR`/`CPAL` v0 or `glyph_id` is not a color base glyph.
pub fn colr_cpal_layers(
    sfnt: &[u8],
    glyph_id: u16,
    palette_index: u16,
    fg_color: [u8; 4],
) -> Option<Vec<(u16, [u8; 4])>> {
    let colr = find_table(sfnt, *b"COLR")?;
    let cpal = find_table(sfnt, *b"CPAL")?;
    if u16_be(colr, 0)? != 0 || u16_be(cpal, 0)? != 0 {
        return None;
    }

    let base_count = usize::from(u16_be(colr, 2)?);
    let base_off = usize::try_from(u32_be(colr, 4)?).ok()?;
    let layer_off = usize::try_from(u32_be(colr, 8)?).ok()?;
    let layer_count = usize::from(u16_be(colr, 12)?);

    let bases = colr.get(base_off..base_off.checked_add(base_count.checked_mul(6)?)?)?;
    let mut lo = 0usize;
    let mut hi = base_count;
    let mut found = None;
    while lo < hi {
        let mid = (lo + hi) / 2;
        let rec = mid * 6;
        let g = u16_be(bases, rec)?;
        if g < glyph_id {
            lo = mid + 1;
        } else if g > glyph_id {
            hi = mid;
        } else {
            let first_layer = usize::from(u16_be(bases, rec + 2)?);
            let num_layers = usize::from(u16_be(bases, rec + 4)?);
            found = Some((first_layer, num_layers));
            break;
        }
    }
    let (first_layer, num_layers) = found?;
    if num_layers == 0 || first_layer.checked_add(num_layers)? > layer_count {
        return None;
    }

    let palette_entries = u16_be(cpal, 2)?;
    let num_palettes = u16_be(cpal, 4)?;
    let num_colors = usize::from(u16_be(cpal, 6)?);
    let colors_off = usize::try_from(u32_be(cpal, 8)?).ok()?;
    if num_palettes == 0 || palette_entries == 0 {
        return None;
    }
    let p_idx = usize::from(palette_index.min(num_palettes - 1));
    let color_base = usize::from(u16_be(cpal, 12 + p_idx * 2)?);
    let colors = cpal.get(colors_off..colors_off.checked_add(num_colors.checked_mul(4)?)?)?;

    let layers_slice = colr.get(
        layer_off.checked_add(first_layer.checked_mul(4)?)?
            ..layer_off.checked_add((first_layer + num_layers).checked_mul(4)?)?,
    )?;

    let mut out = Vec::with_capacity(num_layers);
    for entry in layers_slice.chunks_exact(4) {
        let l_gid = u16_be(entry, 0)?;
        let pal_entry = u16_be(entry, 2)?;
        let rgba = if pal_entry == 0xFFFF {
            fg_color
        } else {
            if pal_entry >= palette_entries {
                return None;
            }
            let c_idx = color_base.checked_add(usize::from(pal_entry))?;
            let rec = c_idx.checked_mul(4)?;
            let bgra = colors.get(rec..rec + 4)?;
            scale_alpha([bgra[2], bgra[1], bgra[0], bgra[3]], fg_color[3])
        };
        out.push((l_gid, rgba));
    }
    Some(out)
}

/// Reports whether `ch` is an emoji base scalar eligible for color presentation unless
/// suppressed by VS15 (`U+FE0E`).
#[inline]
pub fn is_color_emoji_scalar(ch: char) -> bool {
    matches!(
        ch,
        '\u{231A}'..='\u{231B}'
            | '\u{23E9}'..='\u{23F3}'
            | '\u{23F8}'..='\u{23FA}'
            | '\u{2600}'..='\u{27BF}'
            | '\u{2B50}'
            | '\u{1F004}'..='\u{1FAFF}'
    ) && !matches!(ch, '\u{1F3FB}'..='\u{1F3FF}')
}

#[inline]
pub(super) fn skin_tone_fill(modifier: char) -> Option<[u8; 4]> {
    Some(match modifier {
        '\u{1F3FB}' => [250, 220, 196, 255],
        '\u{1F3FC}' => [229, 190, 155, 255],
        '\u{1F3FD}' => [198, 148, 104, 255],
        '\u{1F3FE}' => [158, 104, 63, 255],
        '\u{1F3FF}' => [94, 60, 38, 255],
        _ => return None,
    })
}

/// Returns `(fill_rgba, detail_rgba)` for a `FormaEmoji` base glyph (`face == 2`), or `None` if
/// text presentation (`U+FE0E`) was requested or `ch` is non-emoji.
pub(super) fn emoji_palette_for_cluster(
    ch: char,
    vs15_text: bool,
    skin_tone: Option<char>,
    fg_alpha: u8,
) -> Option<([u8; 4], [u8; 4])> {
    if vs15_text || !is_color_emoji_scalar(ch) {
        return None;
    }
    let fill = if let Some(tone) = skin_tone.and_then(skin_tone_fill) {
        tone
    } else {
        match ch {
            '\u{1F49A}' | '\u{1F330}'..='\u{1F343}' => [34, 197, 94, 255],
            '\u{2764}'
            | '\u{1F493}'..='\u{1F49F}'
            | '\u{1F9E1}'
            | '\u{1F525}'
            | '\u{1F680}'
            | '\u{1F980}' => [239, 68, 68, 255],
            '\u{1F30D}'..='\u{1F30F}' | '\u{1F30A}' => [56, 189, 248, 255],
            '\u{1F1E6}'..='\u{1F1FF}' => [59, 130, 246, 255],
            '\u{1F600}'..='\u{1F64F}'
            | '\u{1F910}'..='\u{1F92F}'
            | '\u{1F970}'..='\u{1F97A}'
            | '\u{1F440}'..='\u{1F450}'
            | '\u{2B50}' => [255, 204, 77, 255],
            _ => match (ch as u32) % 4 {
                0 => [245, 158, 11, 255],
                1 => [59, 130, 246, 255],
                2 => [236, 72, 153, 255],
                _ => [16, 185, 129, 255],
            },
        }
    };
    let detail = [
        (fill[0] / 3).max(28),
        (fill[1] / 3).max(24),
        (fill[2] / 3).max(24),
        255,
    ];
    Some((scale_alpha(fill, fg_alpha), scale_alpha(detail, fg_alpha)))
}

/// Picks the largest bounding-box subpath inside a multi-contour glyph (`breaks` stores starting
/// edge indices per closed contour). That outer loop gives a hole-free silhouette for the emoji's
/// primary fill color underneath eye/mouth/stroke detail contours.
pub(super) fn largest_contour_range(
    edges: &[(Point, Point)],
    breaks: &[usize],
) -> Option<(usize, usize)> {
    if breaks.len() < 2 {
        return None;
    }
    let mut best = None;
    let mut best_area = -1.0_f32;
    for w in 0..breaks.len() {
        let start = breaks[w];
        let end = breaks.get(w + 1).copied().unwrap_or(edges.len());
        if start >= end {
            continue;
        }
        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for &(a, b) in &edges[start..end] {
            min_x = min_x.min(a.x).min(b.x);
            max_x = max_x.max(a.x).max(b.x);
            min_y = min_y.min(a.y).min(b.y);
            max_y = max_y.max(a.y).max(b.y);
        }
        let area = (max_x - min_x) * (max_y - min_y);
        if area > best_area {
            best_area = area;
            best = Some((start, end));
        }
    }
    best
}
