//! What a text field does today with text beyond Latin letters: emoji sequences, combining
//! marks, right-to-left scripts, scripts without spaces and line breaks. Recorded before the
//! shaping and input work of the roadmap, so a change in any of these is visible.

#[path = "support/golden.rs"]
mod golden;

use forma::Runtime;
use std::collections::BTreeMap;
use std::fmt::Write;

fn field(value: &str) -> Runtime {
    let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");
    let source = format!(
        "component App {{ state text: String = '{escaped}'; Column {{ width: 480; height: 120; \
         TextInput {{ key: 'f'; width: 440; height: 40; value <-> state.text; }} }} }}"
    );
    let files = BTreeMap::from([("App.ui".to_owned(), source)]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert!(runtime.focus_control(0));
    runtime.text_key("End", false, false);
    runtime
}

/// Each script and what it exercises.
const SAMPLES: &[(&str, &str)] = &[
    ("ascii", "hello world"),
    ("cyrillic", "привет мир"),
    ("combining acute", "cafe\u{301} ok"),
    ("combining acute at the end", "ok cafe\u{301}"),
    ("thai two words", "สวัสดี ครับ"),
    ("flag", "go \u{1F1F7}\u{1F1FA}"),
    (
        "zwj family",
        "hi \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
    ),
    ("skin tone", "ok \u{1F44D}\u{1F3FD}"),
    ("hebrew", "שלום עולם"),
    ("arabic", "مرحبا بالعالم"),
    ("mixed direction", "abc אבג def"),
    ("japanese", "日本語のテキスト"),
    ("thai", "สวัสดีครับ"),
];

fn chars(text: &str) -> usize {
    text.chars().count()
}

#[test]
fn text_editing_handles_scripts_as_recorded() {
    let mut out = String::new();
    for (title, sample) in SAMPLES {
        let mut runtime = field(sample);
        writeln!(out, "{title}: {} chars", chars(sample)).unwrap();

        runtime.text_key("Backspace", false, false);
        let after = runtime.text_value(0);
        writeln!(
            out,
            "  backspace removes {} char(s)",
            chars(sample) - chars(&after)
        )
        .unwrap();

        let mut runtime = field(sample);
        runtime.text_key("ArrowLeft", false, false);
        let caret = runtime.text_selection_utf16();
        writeln!(out, "  left arrow puts the caret at utf16 {caret:?}").unwrap();

        let mut runtime = field(sample);
        runtime.text_word_key("Backspace", false);
        let after = runtime.text_value(0);
        writeln!(out, "  word backspace leaves {after:?}").unwrap();

        let mut runtime = field(sample);
        runtime.text_key("Home", false, false);
        let mut positions = Vec::new();
        for _ in 0..=chars(sample) {
            positions.push(
                runtime
                    .text_caret_bounds()
                    .first()
                    .copied()
                    .unwrap_or(f32::NAN),
            );
            runtime.text_key("ArrowRight", false, false);
        }
        let rightward = positions.windows(2).all(|pair| pair[1] >= pair[0]);
        // Arrows move the way the line looks, so the right arrow never moves the caret left. On
        // a right-to-left line Home is its right end, and the right arrow stays there.
        writeln!(
            out,
            "  caret advances rightward through the whole text: {rightward}"
        )
        .unwrap();

        let mut runtime = field(sample);
        runtime.text_word_key("ArrowLeft", false);
        writeln!(
            out,
            "  word left from the end stops at utf16 {:?}",
            runtime.text_selection_utf16()
        )
        .unwrap();

        let mut runtime = field(sample);
        runtime.text_key("Home", false, false);
        runtime.text_word_key("ArrowRight", false);
        writeln!(
            out,
            "  word right from start stops at utf16 {:?}",
            runtime.text_selection_utf16()
        )
        .unwrap();
    }
    golden::assert_golden("text_editing_scripts.txt", &out);
}
