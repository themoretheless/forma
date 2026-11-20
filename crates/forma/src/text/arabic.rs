//! Contextual Arabic presentation forms (`U+0621..=U+06FF` and `U+FB50..=U+FEFF`).
//!
//! Each Arabic letter resolves to its contextual form (`Isolated`, `Final`, `Initial`, `Medial`)
//! across transparent combining marks (`tashkil`/`harakat`). Letters with Unicode Presentation
//! Forms-A/B codepoints are mapped by `joined`, with vocalized `LAM + marks* + ALEF` collapsing
//! onto the Lam-Alef ligature while the Alef becomes [`ABSORBED`]. Letters without legacy PFA
//! codepoints retain their base scalar in `joined` while `arabic_forms` preserves their joining
//! context for OpenType `GSUB` (`isol`/`fina`/`init`/`medi`) substitution in `shaping`.

/// Placeholder emitted in place of an alef absorbed into a preceding Lam-Alef ligature.
pub(super) const ABSORBED: char = '\0';

const LAM: char = '\u{0644}';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArabicForm {
    None,
    Isolated,
    Final,
    Initial,
    Medial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JoinType {
    None,
    Transparent,
    Right,
    Dual,
}

#[inline]
pub(super) fn is_transparent(ch: char) -> bool {
    matches!(
        ch,
        '\u{0610}'..='\u{061A}'
            | '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{06D6}'..='\u{06ED}'
            | '\u{0300}'..='\u{036F}'
    )
}

#[inline]
fn forms_for(ch: char) -> Option<[u16; 4]> {
    // Each entry: (base, [isolated, final, initial, medial]). 0 means form does not exist.
    const TABLE: &[(char, [u16; 4])] = &[
        ('\u{0621}', [0xFE80, 0x0000, 0x0000, 0x0000]), // ء
        ('\u{0622}', [0xFE81, 0xFE82, 0x0000, 0x0000]), // آ
        ('\u{0623}', [0xFE83, 0xFE84, 0x0000, 0x0000]), // أ
        ('\u{0624}', [0xFE85, 0xFE86, 0x0000, 0x0000]), // ؤ
        ('\u{0625}', [0xFE87, 0xFE88, 0x0000, 0x0000]), // إ
        ('\u{0626}', [0xFE89, 0xFE8A, 0xFE8B, 0xFE8C]), // ئ
        ('\u{0627}', [0xFE8D, 0xFE8E, 0x0000, 0x0000]), // ا
        ('\u{0628}', [0xFE8F, 0xFE90, 0xFE91, 0xFE92]), // ب
        ('\u{0629}', [0xFE93, 0xFE94, 0x0000, 0x0000]), // ة
        ('\u{062A}', [0xFE95, 0xFE96, 0xFE97, 0xFE98]), // ت
        ('\u{062B}', [0xFE99, 0xFE9A, 0xFE9B, 0xFE9C]), // ث
        ('\u{062C}', [0xFE9D, 0xFE9E, 0xFE9F, 0xFEA0]), // ج
        ('\u{062D}', [0xFEA1, 0xFEA2, 0xFEA3, 0xFEA4]), // ح
        ('\u{062E}', [0xFEA5, 0xFEA6, 0xFEA7, 0xFEA8]), // خ
        ('\u{062F}', [0xFEA9, 0xFEAA, 0x0000, 0x0000]), // د
        ('\u{0630}', [0xFEAB, 0xFEAC, 0x0000, 0x0000]), // ذ
        ('\u{0631}', [0xFEAD, 0xFEAE, 0x0000, 0x0000]), // ر
        ('\u{0632}', [0xFEAF, 0xFEB0, 0x0000, 0x0000]), // ز
        ('\u{0633}', [0xFEB1, 0xFEB2, 0xFEB3, 0xFEB4]), // س
        ('\u{0634}', [0xFEB5, 0xFEB6, 0xFEB7, 0xFEB8]), // ش
        ('\u{0635}', [0xFEB9, 0xFEBA, 0xFEBB, 0xFEBC]), // ص
        ('\u{0636}', [0xFEBD, 0xFEBE, 0xFEBF, 0xFEC0]), // ض
        ('\u{0637}', [0xFEC1, 0xFEC2, 0xFEC3, 0xFEC4]), // ط
        ('\u{0638}', [0xFEC5, 0xFEC6, 0xFEC7, 0xFEC8]), // ظ
        ('\u{0639}', [0xFEC9, 0xFECA, 0xFECB, 0xFECC]), // ع
        ('\u{063A}', [0xFECD, 0xFECE, 0xFECF, 0xFED0]), // غ
        ('\u{0641}', [0xFED1, 0xFED2, 0xFED3, 0xFED4]), // ف
        ('\u{0642}', [0xFED5, 0xFED6, 0xFED7, 0xFED8]), // ق
        ('\u{0643}', [0xFED9, 0xFEDA, 0xFEDB, 0xFEDC]), // ك
        ('\u{0644}', [0xFEDD, 0xFEDE, 0xFEDF, 0xFEE0]), // ل
        ('\u{0645}', [0xFEE1, 0xFEE2, 0xFEE3, 0xFEE4]), // م
        ('\u{0646}', [0xFEE5, 0xFEE6, 0xFEE7, 0xFEE8]), // ن
        ('\u{0647}', [0xFEE9, 0xFEEA, 0xFEEB, 0xFEEC]), // ه
        ('\u{0648}', [0xFEED, 0xFEEE, 0x0000, 0x0000]), // و
        ('\u{0649}', [0xFEEF, 0xFEF0, 0xFBE8, 0xFBE9]), // ى
        ('\u{064A}', [0xFEF1, 0xFEF2, 0xFEF3, 0xFEF4]), // ي
        ('\u{0671}', [0xFB50, 0xFB51, 0x0000, 0x0000]), // ٱ
        ('\u{0677}', [0xFBDD, 0x0000, 0x0000, 0x0000]), // ٷ
        ('\u{0679}', [0xFB66, 0xFB67, 0xFB68, 0xFB69]), // ٹ
        ('\u{067A}', [0xFB5E, 0xFB5F, 0xFB60, 0xFB61]), // ٺ
        ('\u{067B}', [0xFB52, 0xFB53, 0xFB54, 0xFB55]), // ٻ
        ('\u{067E}', [0xFB56, 0xFB57, 0xFB58, 0xFB59]), // پ
        ('\u{067F}', [0xFB62, 0xFB63, 0xFB64, 0xFB65]), // ٿ
        ('\u{0680}', [0xFB5A, 0xFB5B, 0xFB5C, 0xFB5D]), // ڀ
        ('\u{0683}', [0xFB76, 0xFB77, 0xFB78, 0xFB79]), // ڃ
        ('\u{0684}', [0xFB72, 0xFB73, 0xFB74, 0xFB75]), // ڄ
        ('\u{0686}', [0xFB7A, 0xFB7B, 0xFB7C, 0xFB7D]), // چ
        ('\u{0687}', [0xFB7E, 0xFB7F, 0xFB80, 0xFB81]), // ڇ
        ('\u{0688}', [0xFB88, 0xFB89, 0x0000, 0x0000]), // ڈ
        ('\u{068C}', [0xFB84, 0xFB85, 0x0000, 0x0000]), // ڌ
        ('\u{068D}', [0xFB82, 0xFB83, 0x0000, 0x0000]), // ڍ
        ('\u{068E}', [0xFB86, 0xFB87, 0x0000, 0x0000]), // ڎ
        ('\u{0691}', [0xFB8C, 0xFB8D, 0x0000, 0x0000]), // ڑ
        ('\u{0698}', [0xFB8A, 0xFB8B, 0x0000, 0x0000]), // ژ
        ('\u{06A4}', [0xFB6A, 0xFB6B, 0xFB6C, 0xFB6D]), // ڤ
        ('\u{06A6}', [0xFB6E, 0xFB6F, 0xFB70, 0xFB71]), // ڦ
        ('\u{06A9}', [0xFB8E, 0xFB8F, 0xFB90, 0xFB91]), // ک
        ('\u{06AD}', [0xFBD3, 0xFBD4, 0xFBD5, 0xFBD6]), // ڭ
        ('\u{06AF}', [0xFB92, 0xFB93, 0xFB94, 0xFB95]), // گ
        ('\u{06B1}', [0xFB9A, 0xFB9B, 0xFB9C, 0xFB9D]), // ڱ
        ('\u{06B3}', [0xFB96, 0xFB97, 0xFB98, 0xFB99]), // ڳ
        ('\u{06BA}', [0xFB9E, 0xFB9F, 0x0000, 0x0000]), // ں
        ('\u{06BB}', [0xFBA0, 0xFBA1, 0xFBA2, 0xFBA3]), // ڻ
        ('\u{06BE}', [0xFBAA, 0xFBAB, 0xFBAC, 0xFBAD]), // ھ
        ('\u{06C0}', [0xFBA4, 0xFBA5, 0x0000, 0x0000]), // ۀ
        ('\u{06C1}', [0xFBA6, 0xFBA7, 0xFBA8, 0xFBA9]), // ہ
        ('\u{06C5}', [0xFBE0, 0xFBE1, 0x0000, 0x0000]), // ۅ
        ('\u{06C6}', [0xFBD9, 0xFBDA, 0x0000, 0x0000]), // ۆ
        ('\u{06C7}', [0xFBD7, 0xFBD8, 0x0000, 0x0000]), // ۇ
        ('\u{06C8}', [0xFBDB, 0xFBDC, 0x0000, 0x0000]), // ۈ
        ('\u{06C9}', [0xFBE2, 0xFBE3, 0x0000, 0x0000]), // ۉ
        ('\u{06CB}', [0xFBDE, 0xFBDF, 0x0000, 0x0000]), // ۋ
        ('\u{06CC}', [0xFBFC, 0xFBFD, 0xFBFE, 0xFBFF]), // ی
        ('\u{06D0}', [0xFBE4, 0xFBE5, 0xFBE6, 0xFBE7]), // ې
        ('\u{06D2}', [0xFBAE, 0xFBAF, 0x0000, 0x0000]), // ے
        ('\u{06D3}', [0xFBB0, 0xFBB1, 0x0000, 0x0000]), // ۓ
    ];
    TABLE
        .binary_search_by_key(&ch, |(base, _)| *base)
        .ok()
        .map(|i| TABLE[i].1)
}

#[inline]
fn join_type(ch: char) -> JoinType {
    if is_transparent(ch) {
        return JoinType::Transparent;
    }
    if ch == '\u{0640}' {
        return JoinType::Dual;
    }
    if let Some(f) = forms_for(ch) {
        return if f[2] != 0 {
            JoinType::Dual
        } else if f[1] != 0 {
            JoinType::Right
        } else {
            JoinType::None
        };
    }
    match ch {
        '\u{0672}'..='\u{0673}'
        | '\u{0675}'..='\u{0676}'
        | '\u{0688}'..='\u{0699}'
        | '\u{06C0}'
        | '\u{06C3}'..='\u{06CB}'
        | '\u{06CD}'
        | '\u{06CF}'
        | '\u{06D2}'..='\u{06D3}'
        | '\u{06D5}'
        | '\u{06EE}'..='\u{06EF}' => JoinType::Right,
        '\u{0678}'..='\u{0687}'
        | '\u{069A}'..='\u{06BF}'
        | '\u{06C1}'..='\u{06C2}'
        | '\u{06CC}'
        | '\u{06CE}'
        | '\u{06D0}'..='\u{06D1}'
        | '\u{06FA}'..='\u{06FC}'
        | '\u{06FF}' => JoinType::Dual,
        _ => JoinType::None,
    }
}

#[inline]
fn lam_alef(alef: char) -> Option<u16> {
    match alef {
        '\u{0622}' => Some(0xFEF5),
        '\u{0623}' => Some(0xFEF7),
        '\u{0625}' => Some(0xFEF9),
        '\u{0627}' => Some(0xFEFB),
        _ => None,
    }
}

#[inline]
fn prev_non_transparent(chars: &[char], mut index: usize) -> Option<usize> {
    while index > 0 {
        index -= 1;
        if join_type(chars[index]) != JoinType::Transparent {
            return Some(index);
        }
    }
    None
}

#[inline]
fn next_non_transparent(chars: &[char], mut index: usize) -> Option<usize> {
    index += 1;
    while index < chars.len() {
        if join_type(chars[index]) != JoinType::Transparent {
            return Some(index);
        }
        index += 1;
    }
    None
}

pub(super) fn has_arabic(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch,
            '\u{0600}'..='\u{06FF}'
                | '\u{0750}'..='\u{077F}'
                | '\u{08A0}'..='\u{08FF}'
                | '\u{FB50}'..='\u{FDFF}'
                | '\u{FE70}'..='\u{FEFF}'
        )
    })
}

/// Computes the contextual Arabic joining form of every character in `chars`.
pub(super) fn arabic_forms(chars: &[char]) -> Vec<ArabicForm> {
    let mut forms = vec![ArabicForm::None; chars.len()];
    for (i, &ch) in chars.iter().enumerate() {
        let jt = join_type(ch);
        if !matches!(jt, JoinType::Right | JoinType::Dual) {
            continue;
        }
        let left_joins = matches!(jt, JoinType::Dual)
            && next_non_transparent(chars, i)
                .is_some_and(|ni| matches!(join_type(chars[ni]), JoinType::Right | JoinType::Dual));
        let right_joins =
            prev_non_transparent(chars, i).is_some_and(|pi| join_type(chars[pi]) == JoinType::Dual);
        forms[i] = match (right_joins, left_joins) {
            (false, false) => ArabicForm::Isolated,
            (true, false) => ArabicForm::Final,
            (false, true) => ArabicForm::Initial,
            (true, true) => ArabicForm::Medial,
        };
    }
    forms
}

/// Replaces Arabic letters with their contextual Unicode Presentation Forms while keeping
/// a 1:1 index alignment with `text.chars()` (`ABSORBED` marks absorbed Lam-Alef alefs).
pub(super) fn joined(text: &str) -> Vec<char> {
    let chars: Vec<char> = text.chars().collect();
    let ctx = arabic_forms(&chars);
    let mut out = chars.clone();
    for (index, &ch) in chars.iter().enumerate() {
        if let Some(pi) = prev_non_transparent(&chars, index) {
            if chars[pi] == LAM && lam_alef(ch).is_some() {
                out[index] = ABSORBED;
                continue;
            }
        }
        if ch == LAM {
            if let Some(ni) = next_non_transparent(&chars, index) {
                if let Some(lig) = lam_alef(chars[ni]) {
                    let joins_in = matches!(ctx[index], ArabicForm::Final | ArabicForm::Medial);
                    out[index] = char::from_u32(u32::from(lig + u16::from(joins_in))).unwrap();
                    continue;
                }
            }
        }
        let Some(table) = forms_for(ch) else {
            continue;
        };
        let code = match ctx[index] {
            ArabicForm::Isolated | ArabicForm::None => table[0],
            ArabicForm::Final => table[1],
            ArabicForm::Initial => table[2],
            ArabicForm::Medial => table[3],
        };
        if code != 0 {
            if let Some(mapped) = char::from_u32(u32::from(code)) {
                out[index] = mapped;
            }
        }
    }
    out
}
