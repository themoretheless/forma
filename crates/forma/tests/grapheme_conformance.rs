//! Independent boundary oracle: Unicode17 GraphemeBreakTest (vendored with license).
use forma::control_state::TextEdit;

fn corpus() -> Vec<(usize, String, Vec<usize>)> {
    assert_eq!(unicode_segmentation::UNICODE_VERSION, (17, 0, 0));
    let source = include_str!("fixtures/unicode-17.0.0/GraphemeBreakTest.txt");
    assert!(source.starts_with("# GraphemeBreakTest-17.0.0.txt"));
    let cases: Vec<_> = source
        .lines()
        .enumerate()
        .filter_map(|(line, text)| {
            let data = text.split('#').next().unwrap().trim();
            if data.is_empty() {
                return None;
            }
            let mut value = String::new();
            let mut boundaries = Vec::new();
            for token in data.split_whitespace() {
                match token {
                    "÷" => boundaries.push(value.len()),
                    "×" => {}
                    code => {
                        value.push(char::from_u32(u32::from_str_radix(code, 16).unwrap()).unwrap())
                    }
                }
            }
            assert_eq!(boundaries.first(), Some(&0));
            assert_eq!(boundaries.last(), Some(&value.len()));
            Some((line + 1, value, boundaries))
        })
        .collect();
    assert_eq!(cases.len(), 766, "full pinned corpus is required");
    cases
}

#[test]
fn unicode17_reference_boundaries_match_forward_reverse_and_selection() {
    for (line, value, bounds) in corpus() {
        let mut model = TextEdit::with_multiline(value, true).unwrap();
        model.home(false);
        for &next in bounds.iter().skip(1) {
            model.move_graphemes(1, false);
            assert_eq!(model.caret(), next, "forward line{line}");
        }
        for &previous in bounds.iter().rev().skip(1) {
            model.move_graphemes(-1, false);
            assert_eq!(model.caret(), previous, "reverse line{line}");
        }
        for pair in bounds.windows(2) {
            model.set_selection(pair[0], pair[0]).unwrap();
            model.move_graphemes(1, true);
            assert_eq!(model.selection(), pair[0]..pair[1], "selection line{line}");
        }
    }
}

#[test]
fn unicode17_deletion_undo_and_redo_remove_exact_reference_clusters() {
    for (line, value, bounds) in corpus() {
        for pair in bounds.windows(2) {
            let expected = format!("{}{}", &value[..pair[0]], &value[pair[1]..]);
            for forward in [false, true] {
                let mut model = TextEdit::with_multiline(value.clone(), true).unwrap();
                let at = if forward { pair[0] } else { pair[1] };
                model.set_selection(at, at).unwrap();
                if forward {
                    model.delete_grapheme();
                } else {
                    model.backspace_grapheme();
                }
                assert_eq!(
                    model.value(),
                    expected,
                    "delete forward={forward} line{line}"
                );
                assert!(model.undo());
                assert_eq!(model.value(), value);
                assert_eq!(model.caret(), at);
                assert!(model.redo());
                assert_eq!(model.value(), expected);
            }
        }
    }
}

#[test]
fn unicode17_arbitrary_scalar_offsets_snap_using_full_string_context() {
    for (line, value, bounds) in corpus() {
        for at in value
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(value.len()))
        {
            let mut model = TextEdit::with_multiline(value.clone(), true).unwrap();
            model.set_selection(at, at).unwrap();
            let ceil = *bounds.iter().find(|&&b| b >= at).unwrap();
            assert_eq!(
                model.grapheme_selection(),
                ceil..ceil,
                "snap line{line} byte{at}"
            );
            model.move_graphemes(-1, false);
            assert_eq!(
                model.caret(),
                bounds.iter().copied().rfind(|&b| b < at).unwrap_or(0),
                "left line{line} byte{at}"
            );
            model.set_selection(at, at).unwrap();
            model.move_graphemes(1, false);
            assert_eq!(
                model.caret(),
                bounds
                    .iter()
                    .copied()
                    .find(|&b| b > at)
                    .unwrap_or(value.len()),
                "right line{line} byte{at}"
            );
        }
    }
}

#[test]
fn unicode17_partial_scalar_replacements_expand_to_reference_clusters() {
    for (line, value, bounds) in corpus() {
        for pair in bounds.windows(2) {
            let expected = format!("{}X{}", &value[..pair[0]], &value[pair[1]..]);
            for (offset, ch) in value[pair[0]..pair[1]].char_indices() {
                let lo = pair[0] + offset;
                let hi = lo + ch.len_utf8();
                let mut model = TextEdit::with_multiline(value.clone(), true).unwrap();
                model.set_selection(lo, hi).unwrap();
                assert_eq!(
                    model.grapheme_selection(),
                    pair[0]..pair[1],
                    "selection line{line} scalar{lo}"
                );
                model.insert_graphemes("X").unwrap();
                assert_eq!(model.value(), expected, "replace line{line} scalar{lo}");
                model.undo();
                assert_eq!(model.value(), value);
                assert_eq!(model.selection(), lo..hi);
                model.redo();
                assert_eq!(model.value(), expected);
            }
        }
    }
}

#[test]
fn unicode17_word_actions_remain_on_reference_grapheme_boundaries() {
    for (line, value, bounds) in corpus() {
        for at in value
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(value.len()))
        {
            let floor = *bounds.iter().rev().find(|&&b| b <= at).unwrap();
            let ceil = *bounds.iter().find(|&&b| b >= at).unwrap();
            for forward in [false, true] {
                let mut model = TextEdit::with_multiline(value.clone(), true).unwrap();
                model.set_selection(at, at).unwrap();
                model.move_words(if forward { 1 } else { -1 }, false);
                let target = model.caret();
                assert!(bounds.contains(&target), "word move line{line} scalar{at}");
                let expected = if forward {
                    format!("{}{}", &value[..floor], &value[target..])
                } else {
                    format!("{}{}", &value[..target], &value[ceil..])
                };
                model.set_selection(at, at).unwrap();
                if forward {
                    model.delete_word();
                } else {
                    model.backspace_word();
                }
                assert_eq!(model.value(), expected, "word erase line{line} scalar{at}");
            }
        }
    }
}
