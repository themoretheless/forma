//! Visual order of one line that mixes left-to-right and right-to-left text.
//!
//! The paragraph direction is always left to right. Inside it a run of right-to-left clusters
//! is laid out mirrored: its first cluster ends up rightmost. Neutral clusters (spaces,
//! punctuation) between two right-to-left clusters join their run; digits and everything else
//! keep the left-to-right direction. Bracket mirroring and Arabic joining are not done here.

use super::*;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

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

/// The clusters of `line` in logical order, each with its place on the line.
pub(crate) fn visual_line(family: FontFamily, line: &str, font_size: f32) -> Vec<VisualCluster> {
    let table = font_table_for(family);
    let mut clusters: Vec<(Range<usize>, bool, bool, f32)> = line
        .grapheme_indices(true)
        .map(|(start, cluster)| {
            let first = cluster.chars().next().unwrap_or(' ');
            let units: f32 = cluster
                .chars()
                .map(|ch| resolve_glyph_with_table(ch, table).1)
                .sum();
            let rtl = is_rtl_char(first);
            let strong = rtl || is_strong_ltr_char(first) || first.is_ascii_digit();
            (start..start + cluster.len(), rtl, strong, units)
        })
        .collect();
    // A neutral cluster takes the direction of its neighbours when both are right-to-left.
    let mut before = false;
    let mut index = 0;
    while index < clusters.len() {
        if clusters[index].2 {
            before = clusters[index].1;
            index += 1;
            continue;
        }
        let end = (index..clusters.len())
            .find(|&next| clusters[next].2)
            .unwrap_or(clusters.len());
        if before && clusters.get(end).is_some_and(|next| next.1) {
            for neutral in &mut clusters[index..end] {
                neutral.1 = true;
            }
        }
        index = end;
    }
    let pixels = |units: f32| pixels_from_units(units, font_size, table.units_per_em);
    let mut placed = Vec::with_capacity(clusters.len());
    let mut units = 0.;
    let mut start = 0;
    while start < clusters.len() {
        let rtl = clusters[start].1;
        let end = (start..clusters.len())
            .find(|&next| clusters[next].1 != rtl)
            .unwrap_or(clusters.len());
        let run: f32 = clusters[start..end].iter().map(|cluster| cluster.3).sum();
        let mut pen = if rtl { units + run } else { units };
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
        units += run;
        start = end;
    }
    placed
}

/// Where the caret stands for a byte offset of the line: after the cluster that ends there, on
/// the side that cluster's direction continues to.
pub(crate) fn caret_x(clusters: &[VisualCluster], offset: usize) -> f32 {
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

/// Reorders the per-character glyphs of `text` from logical to visual order. Characters of one
/// cluster stay together and in their own order, so a mark still follows its base.
pub(super) fn reorder_glyphs<T: Copy>(family: FontFamily, text: &str, glyphs: &mut Vec<T>) {
    let clusters = visual_line(family, text, 1.);
    // Byte offset of every character, to turn a cluster's byte range into glyph indices.
    let offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
    if offsets.len() != glyphs.len() {
        return;
    }
    let logical = std::mem::take(glyphs);
    let mut take = |cluster: &VisualCluster| {
        let from = offsets.partition_point(|&offset| offset < cluster.range.start);
        let to = offsets.partition_point(|&offset| offset < cluster.range.end);
        glyphs.extend_from_slice(&logical[from..to]);
    };
    for run in clusters.chunk_by(|a, b| a.rtl == b.rtl) {
        if run[0].rtl {
            run.iter().rev().for_each(&mut take);
        } else {
            run.iter().for_each(&mut take);
        }
    }
}
