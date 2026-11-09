use forma::control_state::TextEdit;

#[test]
fn word_segments_include_punctuation_and_emoji_but_skip_whitespace() {
    let family = "👨‍👩‍👧‍👦";
    let text = format!("one,  two {family} e\u{301}");
    let family_start = text.find(family).unwrap();
    let family_end = family_start + family.len();
    let mut edit = TextEdit::new(text.clone()).unwrap();
    edit.home(false);
    for expected in [3, 4, 9, family_end, text.len()] {
        edit.move_words(1, false);
        assert_eq!(edit.caret(), expected);
    }
    for expected in [family_end + 1, family_start, 6, 3, 0] {
        edit.move_words(-1, false);
        assert_eq!(edit.caret(), expected);
    }
    edit.set_selection(family_end, family_end).unwrap();
    edit.move_words(-1, true);
    assert_eq!(edit.selected_text(), family);
    edit.move_words(1, false);
    assert_eq!(
        edit.caret(),
        family_end,
        "collapsing selection consumes one step"
    );
}

#[test]
fn word_deletion_and_history_keep_clusters_whole_from_scalar_positions() {
    for forward in [false, true] {
        let text = "A👨‍👩‍👧‍👦B";
        let mut edit = TextEdit::new(text.into()).unwrap();
        edit.set_selection(5, 5).unwrap();
        if forward {
            edit.delete_word();
        } else {
            edit.backspace_word();
        }
        assert_eq!(edit.value(), "AB");
        edit.undo();
        assert_eq!(edit.value(), text);
        assert_eq!(edit.caret(), 5);
        edit.redo();
        assert_eq!(edit.value(), "AB");
    }
    let mut edit = TextEdit::new("alpha  e\u{301}".into()).unwrap();
    edit.backspace_word();
    assert_eq!(edit.value(), "alpha  ");
    edit.backspace_word();
    assert_eq!(edit.value(), "");
    edit.undo();
    assert_eq!(edit.value(), "alpha  ");
    let mut lines = TextEdit::with_multiline("one\n  two".into(), true).unwrap();
    lines.home(false);
    lines.delete_word();
    assert_eq!(lines.value(), "\n  two");
    lines.delete_word();
    assert_eq!(lines.value(), "");
    let mut spaces = TextEdit::new("   ".into()).unwrap();
    spaces.move_words(isize::MIN, false);
    assert_eq!(spaces.caret(), 0);
    spaces.move_words(isize::MAX, false);
    assert_eq!(spaces.caret(), 3);
}

#[test]
fn word_policy_retains_apostrophes_and_decimal_segments() {
    let text = "can't 29.3, мир";
    let mut edit = TextEdit::new(text.into()).unwrap();
    edit.home(false);
    edit.move_words(1, true);
    assert_eq!(edit.selected_text(), "can't");
    edit.set_selection(6, 6).unwrap();
    edit.move_words(1, true);
    assert_eq!(edit.selected_text(), "29.3");
    edit.end(false);
    edit.move_words(-1, true);
    assert_eq!(edit.selected_text(), "мир");
}

#[test]
fn runtime_word_keys_commit_and_undo_without_touching_unrelated_state() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = 'one two'; Column { width:240; height:100; TextInput { width:200; height:32; value <-> state.query; } } }".into())]);
    let state = forma::form_document::Properties::from([(
        "marker".into(),
        forma::form_document::Value::Text("keep".into()),
    )]);
    let mut runtime = forma::Runtime::from_ui_files(&files, "App.ui", &state).unwrap();
    assert!(runtime.focus_control(0));
    runtime.text_key("End", false, false);
    assert!(runtime.text_word_key("ArrowLeft", true));
    assert_eq!(runtime.text_selected(), "two");
    assert!(runtime.text_word_key("Backspace", false));
    assert!(runtime.sync_ui_bindings_native().unwrap());
    assert_eq!(runtime.text_value(0), "one ");
    runtime.text_key("z", false, true);
    assert!(runtime.sync_ui_bindings_native().unwrap());
    assert_eq!(runtime.text_value(0), "one two");
    assert!(!runtime.text_word_key("z", false));
    assert_eq!(
        runtime.ui_state_properties()["marker"],
        forma::form_document::Value::Text("keep".into())
    );
    runtime.focus(false);
    assert!(!runtime.text_word_key("Backspace", false));
}
