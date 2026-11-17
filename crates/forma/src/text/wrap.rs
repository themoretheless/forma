//! Soft wrapping and hit testing of wrapped text.

use super::*;

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
            let mut best_word_end = None;
            let mut running_w = 0.0_f32;
            let mut trailing_ws_w = 0.0_f32;
            let mut overflowed = false;
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
                    overflowed = true;
                    break;
                }
            }
            if !overflowed || best_word_end == Some(rest.len()) {
                let full_w = measure_shaped_text(rest, font_size);
                if full_w <= max_width {
                    result.push(((hard_start + cur_start)..(hard_start + line.len()), full_w));
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
