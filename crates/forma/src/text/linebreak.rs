//! UAX #14 and JIS X 4051 (CJK kinsoku) line-breaking opportunities.
//!
//! Used uniformly by both `text::wrap` (single-style wrapping) and `text::runs` (multi-run layout).

use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BreakClass {
    Newline,
    Space,
    Unit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PairRole {
    Glue,
    OpenPunct,
    CloseOrKinsoku,
    BreakAfter,
    Ideograph,
    Other,
}

#[inline]
fn is_glue(ch: char) -> bool {
    matches!(
        ch,
        '\u{00A0}' // NBSP
            | '\u{202F}' // NNBSP
            | '\u{2060}' // Word Joiner
            | '\u{FEFF}' // ZWNBSP
            | '\u{200D}' // ZWJ
            | '\u{200E}'
            | '\u{200F}'
            | '\u{FE0E}'
            | '\u{FE0F}'
            | '\u{20E3}' // Combining Enclosing Keycap
            | '\u{1F3FB}'..='\u{1F3FF}' // Fitzpatrick skin tones
    )
}

#[inline]
fn is_open_punct(ch: char) -> bool {
    matches!(
        ch,
        '(' | '['
            | '{'
            | '“'
            | '‘'
            | '«'
            | '‹'
            | '「'
            | '『'
            | '【'
            | '《'
            | '〈'
            | '（'
            | '［'
            | '｛'
    )
}

#[inline]
fn is_close_or_kinsoku(ch: char) -> bool {
    matches!(
        ch,
        '.' | ','
            | '!'
            | '?'
            | ':'
            | ';'
            | ')'
            | ']'
            | '}'
            | '”'
            | '’'
            | '»'
            | '›'
            | '、'
            | '。'
            | '，'
            | '．'
            | '！'
            | '？'
            | '：'
            | '；'
            | '）'
            | '］'
            | '｝'
            | '」'
            | '』'
            | '】'
            | '》'
            | '〉'
            | '々'
            | 'ー'
            | 'ぁ'
            | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'っ'
            | 'ゃ'
            | 'ゅ'
            | 'ょ'
            | 'ゎ'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ッ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ヮ'
    )
}

#[inline]
fn is_ideographic(ch: char) -> bool {
    matches!(
        ch,
        '\u{3040}'..='\u{30FF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{AC00}'..='\u{D7AF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FF00}'..='\u{FFEF}'
    ) || super::color_emoji::is_color_emoji_scalar(ch)
}

#[inline]
fn role_of(cluster: &str) -> PairRole {
    if cluster.contains('\u{200D}') {
        return PairRole::Ideograph;
    }
    let ch = cluster.chars().next().unwrap_or(' ');
    if is_glue(ch) {
        PairRole::Glue
    } else if is_open_punct(ch) {
        PairRole::OpenPunct
    } else if is_close_or_kinsoku(ch) {
        PairRole::CloseOrKinsoku
    } else if ch == '-' || ch == '\u{2014}' {
        PairRole::BreakAfter
    } else if is_ideographic(ch) {
        PairRole::Ideograph
    } else {
        PairRole::Other
    }
}

/// Reports whether a soft line break is legal directly between `left` and `right` non-whitespace
/// clusters under UAX #14 and CJK kinsoku rules.
pub(super) fn can_break_between(left: &str, right: &str) -> bool {
    let l = role_of(left);
    let r = role_of(right);
    if l == PairRole::Glue || r == PairRole::Glue {
        return false;
    }
    if l == PairRole::OpenPunct || r == PairRole::CloseOrKinsoku {
        return false;
    }
    if l == PairRole::BreakAfter {
        return true;
    }
    l == PairRole::Ideograph || r == PairRole::Ideograph
}

/// Splits a single line (`\n`-free) into unbreakable UAX #14 / kinsoku segments (`Unit` or `Space`).
pub(super) fn segment_units(text: &str) -> Vec<(Range<usize>, BreakClass)> {
    let mut out = Vec::new();
    let mut graphemes = text.grapheme_indices(true).peekable();
    while let Some((start, g)) = graphemes.next() {
        if g.chars().all(|c| c == ' ' || c == '\t') {
            let mut end = start + g.len();
            while let Some(&(_, next_g)) = graphemes.peek() {
                if next_g.chars().all(|c| c == ' ' || c == '\t') {
                    end += next_g.len();
                    graphemes.next();
                } else {
                    break;
                }
            }
            out.push((start..end, BreakClass::Space));
            continue;
        }
        let mut end = start + g.len();
        let mut prev = g;
        while let Some(&(_, next_g)) = graphemes.peek() {
            if next_g.chars().all(|c| c == ' ' || c == '\t') || can_break_between(prev, next_g) {
                break;
            }
            end += next_g.len();
            prev = next_g;
            graphemes.next();
        }
        out.push((start..end, BreakClass::Unit));
    }
    out
}

/// Splits multi-line text into atoms (`Newline`, `Space`, `Unit`) and marks whether a legal break
/// precedes a contiguous `Unit` (after hyphens, CJK ideographs, etc.).
pub(super) fn segment_atoms(text: &str) -> Vec<(Range<usize>, BreakClass)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for chunk in text.split_inclusive('\n') {
        if let Some(body) = chunk.strip_suffix('\n') {
            for (r, cls) in segment_units(body) {
                out.push(((offset + r.start)..(offset + r.end), cls));
            }
            let nl_start = offset + body.len();
            out.push((nl_start..nl_start + 1, BreakClass::Newline));
        } else {
            for (r, cls) in segment_units(chunk) {
                out.push(((offset + r.start)..(offset + r.end), cls));
            }
        }
        offset += chunk.len();
    }
    out
}
