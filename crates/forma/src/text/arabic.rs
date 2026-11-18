//! Arabic joining: which of its forms a letter takes next to its neighbours.
//!
//! Each letter of the basic alphabet is replaced by its presentation form (isolated, final,
//! initial or medial, U+FE80..=U+FEF4), one character for one, so byte offsets and clusters of
//! the text are untouched. Lam followed directly by alef is the lam-alef ligature: the lam
//! becomes the ligature and the alef [`ABSORBED`], which takes no room and draws nothing.

/// First presentation form (the isolated one) and whether the letter also joins to the letter
/// after it. Indexed from U+0621; zero for characters that take no form.
const FORMS: [(u16, bool); 42] = [
    (0xFE80, false), // hamza: joins to nothing, see `joins_back`
    (0xFE81, false),
    (0xFE83, false),
    (0xFE85, false),
    (0xFE87, false),
    (0xFE89, true),
    (0xFE8D, false),
    (0xFE8F, true),
    (0xFE93, false),
    (0xFE95, true),
    (0xFE99, true),
    (0xFE9D, true),
    (0xFEA1, true),
    (0xFEA5, true),
    (0xFEA9, false),
    (0xFEAB, false),
    (0xFEAD, false),
    (0xFEAF, false),
    (0xFEB1, true),
    (0xFEB5, true),
    (0xFEB9, true),
    (0xFEBD, true),
    (0xFEC1, true),
    (0xFEC5, true),
    (0xFEC9, true),
    (0xFECD, true),
    (0, false),
    (0, false),
    (0, false),
    (0, false),
    (0, false),
    (0, false), // tatweel: joins both ways, has no form of its own
    (0xFED1, true),
    (0xFED5, true),
    (0xFED9, true),
    (0xFEDD, true),
    (0xFEE1, true),
    (0xFEE5, true),
    (0xFEE9, true),
    (0xFEED, false),
    (0xFEEF, false),
    (0xFEF1, true),
];
const TATWEEL: char = '\u{640}';
const LAM: char = '\u{644}';
/// Stands for an alef drawn as part of the lam-alef ligature before it.
pub(super) const ABSORBED: char = '\0';

/// The isolated lam-alef ligature for an alef; its final form is the next code point.
fn lam_alef(alef: char) -> Option<u16> {
    Some(match alef {
        '\u{622}' => 0xFEF5,
        '\u{623}' => 0xFEF7,
        '\u{625}' => 0xFEF9,
        '\u{627}' => 0xFEFB,
        _ => return None,
    })
}

fn entry(ch: char) -> Option<(u16, bool)> {
    let index = (ch as u32).checked_sub(0x621)? as usize;
    FORMS.get(index).copied().filter(|(first, _)| *first != 0)
}
/// Joins to the letter before it.
fn joins_back(ch: char) -> bool {
    ch == TATWEEL || (ch != '\u{621}' && entry(ch).is_some())
}
/// Joins to the letter after it.
fn joins_forward(ch: char) -> bool {
    ch == TATWEEL || entry(ch).is_some_and(|(_, dual)| dual)
}
/// Marks sit on a letter and do not interrupt joining.
fn transparent(ch: char) -> bool {
    matches!(ch as u32, 0x64B..=0x65F | 0x670 | 0x6D6..=0x6DC | 0x6DF..=0x6E4)
}

/// Whether the text has Arabic letters at all. U+0600..=U+06FF is the lead bytes 0xD8..=0xDB.
pub(super) fn has_arabic(text: &str) -> bool {
    text.bytes().any(|byte| (0xD8..=0xDB).contains(&byte))
}

/// The characters of `text` as they are drawn: Arabic letters in the form their neighbours ask
/// for, everything else unchanged. One output character per input character.
pub(super) fn joined(text: &str) -> Vec<char> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = chars.clone();
    for (index, &ch) in chars.iter().enumerate() {
        let Some((first, dual)) = entry(ch) else {
            continue;
        };
        let before = chars[..index]
            .iter()
            .rev()
            .find(|ch| !transparent(**ch))
            .is_some_and(|ch| joins_forward(*ch));
        let after = dual
            && chars[index + 1..]
                .iter()
                .find(|ch| !transparent(**ch))
                .is_some_and(|ch| joins_back(*ch));
        let before = before && joins_back(ch);
        if index > 0 && chars[index - 1] == LAM && lam_alef(ch).is_some() {
            out[index] = ABSORBED;
            continue;
        }
        if let Some(ligature) = (ch == LAM)
            .then(|| chars.get(index + 1).copied().and_then(lam_alef))
            .flatten()
        {
            let form = ligature + u16::from(before);
            out[index] = char::from_u32(u32::from(form)).unwrap_or(ch);
            continue;
        }
        // Forms are stored isolated, final, initial, medial.
        let form = first + u16::from(before) + 2 * u16::from(after);
        out[index] = char::from_u32(u32::from(form)).unwrap_or(ch);
    }
    out
}
