//! Visual order and paragraph direction for text mixing LTR and RTL runs.

use super::*;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Declared base direction of a text or input block (`ltr` default, `rtl`, or `auto` first-strong).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum TextDirection {
    #[default]
    Ltr,
    Rtl,
    Auto,
}

impl TextDirection {
    pub fn parse(mode: &str) -> Result<Self, String> {
        match mode.trim() {
            "" | "ltr" => Ok(Self::Ltr),
            "rtl" => Ok(Self::Rtl),
            "auto" => Ok(Self::Auto),
            other => Err(format!("Unsupported text direction '{other}'")),
        }
    }
}

/// Concrete paragraph direction after resolving `TextDirection::Auto` (UAX #9 P2/P3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedDirection {
    Ltr,
    Rtl,
}

/// Resolves `dir` against `text` using the first strongly directional character when `Auto`.
pub fn resolve_direction(text: &str, dir: TextDirection) -> ResolvedDirection {
    match dir {
        TextDirection::Ltr => ResolvedDirection::Ltr,
        TextDirection::Rtl => ResolvedDirection::Rtl,
        TextDirection::Auto => {
            for ch in text.chars() {
                if is_rtl_char(ch) {
                    return ResolvedDirection::Rtl;
                }
                if is_strong_ltr_char(ch) {
                    return ResolvedDirection::Ltr;
                }
            }
            ResolvedDirection::Ltr
        }
    }
}

/// Whether the line needs visual reordering at all. Lines without such characters keep the
/// plain prefix-sum layout and never reach the rest of this module.
pub(crate) fn has_rtl(text: &str) -> bool {
    // Every right-to-left block starts at U+0590, which takes two UTF-8 bytes from 0xD6 on.
    text.bytes().any(|byte| byte >= 0xD6) && text.chars().any(is_rtl_char)
}

/// One grapheme cluster of a line where it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VisualCluster {
    /// Bytes of the line this cluster covers.
    pub range: Range<usize>,
    pub rtl: bool,
    /// Left edge and width in logical pixels.
    pub x: f32,
    pub width: f32,
}

/// The clusters of `line` in logical order, each with its place on the line (LTR paragraph).
pub(crate) fn visual_line(family: FontFamily, line: &str, font_size: f32) -> Vec<VisualCluster> {
    visual_line_with_dir(family, line, font_size, TextDirection::Ltr)
}

/// The clusters of `line` in logical order, laid out under `dir` (`Ltr`, `Rtl`, or `Auto`).
pub(crate) fn visual_line_with_dir(
    family: FontFamily,
    line: &str,
    font_size: f32,
    dir: TextDirection,
) -> Vec<VisualCluster> {
    let table = font_table_for(family);
    let base_rtl = resolve_direction(line, dir) == ResolvedDirection::Rtl;
    // Arabic letters measure in the form their neighbours give them, as they are drawn.
    let forms = arabic::has_arabic(line).then(|| arabic::joined(line));
    let mut position = 0;
    let mut clusters: Vec<(Range<usize>, bool, bool, f32)> = line
        .grapheme_indices(true)
        .map(|(start, cluster)| {
            let first = cluster.chars().next().unwrap_or(' ');
            let units: f32 = cluster
                .chars()
                .map(|ch| {
                    let form = forms.as_ref().map_or(ch, |forms| forms[position]);
                    position += 1;
                    resolve_form_with_table(form, ch, table).1
                })
                .sum();
            let rtl = is_rtl_char(first);
            let strong = rtl || is_strong_ltr_char(first) || first.is_ascii_digit();
            (start..start + cluster.len(), rtl, strong, units)
        })
        .collect();
    // Resolve neutral/whitespace BiDi runs against adjacent strong characters and paragraph base.
    let mut before: Option<bool> = None;
    let mut index = 0;
    while index < clusters.len() {
        if clusters[index].2 {
            before = Some(clusters[index].1);
            index += 1;
            continue;
        }
        let end = (index..clusters.len())
            .find(|&next| clusters[next].2)
            .unwrap_or(clusters.len());
        let after = clusters.get(end).map(|next| next.1);
        let neutral_rtl = match (before, after) {
            (Some(true), Some(true)) => true,
            (Some(false), Some(false)) => false,
            _ => base_rtl,
        };
        if neutral_rtl {
            for neutral in &mut clusters[index..end] {
                neutral.1 = true;
            }
        }
        index = end;
    }
    let pixels = |units: f32| pixels_from_units(units, font_size, table.units_per_em);
    let mut placed = Vec::with_capacity(clusters.len());
    let total_units: f32 = clusters.iter().map(|cluster| cluster.3).sum();
    let mut units = if base_rtl { total_units } else { 0. };
    let mut start = 0;
    while start < clusters.len() {
        let rtl = clusters[start].1;
        let end = (start..clusters.len())
            .find(|&next| clusters[next].1 != rtl)
            .unwrap_or(clusters.len());
        let run: f32 = clusters[start..end].iter().map(|cluster| cluster.3).sum();
        let run_left = if base_rtl {
            units -= run;
            units
        } else {
            let left = units;
            units += run;
            left
        };
        let mut pen = if rtl { run_left + run } else { run_left };
        for (range, _, _, advance) in &clusters[start..end] {
            let left = if rtl {
                pen -= advance;
                pen
            } else {
                let left = pen;
                pen += advance;
                left
            };
            placed.push(VisualCluster {
                range: range.clone(),
                rtl,
                x: pixels(left),
                width: pixels(left + advance) - pixels(left),
            });
        }
        start = end;
    }
    placed
}

/// Where the caret stands for a byte offset of the line: after the cluster that ends there, on
/// the side that cluster's direction continues to (or visual left edge at end of RTL paragraph).
pub(crate) fn caret_x(clusters: &[VisualCluster], offset: usize) -> f32 {
    if let (Some(first), Some(last)) = (clusters.first(), clusters.last()) {
        let rtl_para = clusters.len() > 1 && first.x > last.x + 0.01;
        if rtl_para && offset >= last.range.end && !last.rtl {
            return 0.;
        }
    }
    let before = clusters
        .iter()
        .rev()
        .find(|cluster| cluster.range.end <= offset);
    match (before, clusters.first()) {
        (Some(before), _) => before.x + if before.rtl { 0. } else { before.width },
        (None, Some(first)) => first.x + if first.rtl { first.width } else { 0. },
        (None, None) => 0.,
    }
}

/// The byte offset whose caret is nearest to `x`.
pub(crate) fn hit(clusters: &[VisualCluster], x: f32) -> usize {
    let mut best = (f32::INFINITY, 0);
    let mut consider = |offset: usize| {
        let distance = (caret_x(clusters, offset) - x).abs();
        if distance < best.0 {
            best = (distance, offset);
        }
    };
    consider(0);
    for cluster in clusters {
        consider(cluster.range.end);
    }
    best.1
}

/// The caret stop one step to the right (or left) of the one at `offset`, as seen on screen:
/// the nearest stop further that way, and of two at the same place the one closer in the text.
/// `None` at the end of the line on that side.
pub(crate) fn step(clusters: &[VisualCluster], offset: usize, right: bool) -> Option<usize> {
    let from = caret_x(clusters, offset);
    let mut best: Option<(f32, usize)> = None;
    for stop in std::iter::once(0).chain(clusters.iter().map(|cluster| cluster.range.end)) {
        let x = caret_x(clusters, stop);
        let ahead = if right { x - from } else { from - x };
        if ahead <= 0.01 {
            continue;
        }
        let closer = best.is_none_or(|(distance, other)| {
            ahead < distance - 0.01
                || ((ahead - distance).abs() <= 0.01
                    && stop.abs_diff(offset) < other.abs_diff(offset))
        });
        if closer {
            best = Some((ahead, stop));
        }
    }
    best.map(|(_, stop)| stop)
}

/// The stretches `[left, right]` a selection of bytes `range` covers, left to right. A selection
/// across a change of direction is more than one stretch.
pub(crate) fn selection_spans(clusters: &[VisualCluster], range: Range<usize>) -> Vec<[f32; 2]> {
    let mut spans: Vec<[f32; 2]> = clusters
        .iter()
        .filter(|cluster| cluster.range.start >= range.start && cluster.range.end <= range.end)
        .map(|cluster| [cluster.x, cluster.x + cluster.width])
        .collect();
    spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut merged: Vec<[f32; 2]> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last) if span[0] <= last[1] + 0.01 => last[1] = last[1].max(span[1]),
            _ => merged.push(span),
        }
    }
    merged
}

/// The character drawn in place of a bracket inside a right-to-left run, where "opening" faces
/// the other way.
pub(crate) fn mirrored(ch: char) -> Option<char> {
    Some(match ch {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        _ => return None,
    })
}

/// Reorders the per-character glyphs of `text` from logical to visual order. Characters of one
/// cluster stay together and in their own order, so a mark still follows its base. A bracket in
/// a right-to-left run is drawn mirrored (`mirror` resolves the glyph) and keeps its own advance,
/// so the line measures the same.
pub(super) fn reorder_glyphs_with_dir<G: Copy>(
    family: FontFamily,
    text: &str,
    dir: TextDirection,
    glyphs: &mut Vec<(G, f32)>,
    mirror: impl Fn(char) -> G,
) {
    let clusters = visual_line_with_dir(family, text, 1., dir);
    // Byte offset of every character, to turn a cluster's byte range into glyph indices.
    let offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
    if offsets.len() != glyphs.len() {
        return;
    }
    let base_rtl = resolve_direction(text, dir) == ResolvedDirection::Rtl;
    let logical = std::mem::take(glyphs);
    let mut take = |cluster: &VisualCluster| {
        let from = offsets.partition_point(|&offset| offset < cluster.range.start);
        let to = offsets.partition_point(|&offset| offset < cluster.range.end);
        for index in from..to {
            let (glyph, advance) = logical[index];
            let swapped = cluster
                .rtl
                .then(|| text[offsets[index]..].chars().next().and_then(mirrored))
                .flatten();
            glyphs.push((swapped.map_or(glyph, &mirror), advance));
        }
    };
    let mut emit_run = |run: &[VisualCluster]| {
        if run[0].rtl {
            run.iter().rev().for_each(&mut take);
        } else {
            run.iter().for_each(&mut take);
        }
    };
    if base_rtl {
        for run in clusters.chunk_by(|a, b| a.rtl == b.rtl).rev() {
            emit_run(run);
        }
    } else {
        for run in clusters.chunk_by(|a, b| a.rtl == b.rtl) {
            emit_run(run);
        }
    }
}
