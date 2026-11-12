//! Character classes the shaper distinguishes.

/// Returns `true` if `ch` is a non-spacing combining mark (Unicode Combining Diacritical Marks,
/// Hebrew/Arabic vowel points, or enclosing marks).
pub fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F
            | 0x0483..=0x0489
            | 0x0591..=0x05BD
            | 0x05BF
            | 0x05C1..=0x05C2
            | 0x05C4..=0x05C5
            | 0x05C7
            | 0x0610..=0x061A
            | 0x064B..=0x065F
            | 0x0670
            | 0x06D6..=0x06ED
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F
    )
}

/// Returns `true` if `ch` is a zero-advance joiner or variation selector (`ZWJ`, `ZWNJ`, `VS1..=VS16`, `VS17..=VS256`).
pub fn is_zero_width_modifier(ch: char) -> bool {
    matches!(
        ch as u32,
        0x200C..=0x200D | 0xFE00..=0xFE0F | 0xE0100..=0xE01EF
    )
}

/// Returns `true` if `ch` is an emoji modifier that attaches to a preceding emoji base without adding advance
/// (Fitzpatrick skin-tone modifiers `U+1F3FB..=U+1F3FF` or Combining Enclosing Keycap `U+20E3`).
pub fn is_emoji_modifier(ch: char) -> bool {
    matches!(ch as u32, 0x20E3 | 0x1F3FB..=0x1F3FF)
}

/// Returns `true` if `ch` is a Regional Indicator symbol (`U+1F1E6..=U+1F1FF`, used in pairs for flags).
pub fn is_regional_indicator(ch: char) -> bool {
    matches!(ch as u32, 0x1F1E6..=0x1F1FF)
}

/// Returns `true` if `ch` has strong Right-to-Left directionality (Hebrew, Arabic, Syriac,
/// Thaana, NKo, Presentation Forms).
pub fn is_rtl_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF
    )
}

/// Returns `true` if `ch` has strong Left-to-Right directionality (Latin, Greek, Cyrillic, Han, Kana, etc.).
pub(super) fn is_strong_ltr_char(ch: char) -> bool {
    !is_rtl_char(ch)
        && !is_combining_mark(ch)
        && !is_zero_width_modifier(ch)
        && !ch.is_ascii_digit()
        && !matches!(ch as u32, 0x0660..=0x0669 | 0x06F0..=0x06F9)
        && (ch.is_alphabetic() || matches!(ch as u32, 0x3040..=0x9FFF | 0xAC00..=0xD7AF))
}
