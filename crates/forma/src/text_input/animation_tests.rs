use super::*;
#[test]
fn ui_hit_and_keys_keep_extended_grapheme_clusters_atomic() {
    for cluster in ["e\u{301}", "👍🏽", "👨‍👩‍👧‍👦", "🇬🇪", "1\u{fe0f}\u{20e3}"]
    {
        let value = format!("A{cluster}B");
        let end = 1 + cluster.len();
        let mut editor = Editor::new(
            Input {
                value: value.clone(),
                bounds: [0., 0., 500., 30.],
                ..Input::default()
            },
            vec![],
        )
        .unwrap();
        for x in -10..500 {
            editor.hit(x as f32 * 0.5, 15., false);
            assert!(
                [0, 1, end, value.len()].contains(&editor.model.caret()),
                "hit split {cluster}"
            );
        }
        editor.model.set_selection(end, end).unwrap();
        assert!(editor.key("ArrowLeft", true, false));
        assert_eq!(editor.model.selected_text(), cluster);
        editor.key("Backspace", false, false);
        assert_eq!(editor.model.value(), "AB");
        editor.key("z", false, true);
        assert_eq!(editor.model.value(), value);
        editor.model.set_selection(1, 1).unwrap();
        editor.key("Delete", false, false);
        assert_eq!(editor.model.value(), "AB");
    }
}
#[test]
fn ui_insert_joined_cluster_and_partial_replacement_keep_fades_valid() {
    let mut editor = Editor::new(
        Input {
            value: "\u{301}R".into(),
            ..Input::default()
        },
        vec![],
    )
    .unwrap();
    editor.model.home(false);
    editor.insert("e");
    assert_eq!(editor.model.caret(), 3);
    editor.content(true);
    editor.model.set_selection(1, 3).unwrap();
    editor.insert("X");
    assert_eq!(editor.model.value(), "XR");
    assert_eq!(editor.model.caret(), 1);
    editor.content(true);
    editor.tick(100.);
    editor.content(true);
}
#[test]
fn readonly_blocks_existing_undo_history_without_discarding_it() {
    let mut e = Editor::new(
        Input {
            value: "seed".into(),
            ..Input::default()
        },
        vec![],
    )
    .unwrap();
    e.insert("!");
    let edited = e.model.value().to_owned();
    e.spec.read_only = true;
    e.key("z", false, true);
    e.key("y", false, true);
    e.insert("blocked");
    e.set_preedit("blocked");
    assert_eq!(e.model.value(), edited);
    assert!(e.preedit.is_empty());
    e.spec.read_only = false;
    e.key("z", false, true);
    assert_eq!(e.model.value(), "seed");
}
#[test]
fn command_deletion_targets_logical_line_edges_and_undo_is_atomic() {
    for forward in [false, true] {
        let mut e = Editor::new(
            Input {
                value: "one\r\ntwo three\r\nlast".into(),
                multiline: true,
                ..Input::default()
            },
            vec![],
        )
        .unwrap();
        e.model.set_selection(9, 9).unwrap();
        e.set_preedit("pending");
        assert!(e.key(if forward { "Delete" } else { "Backspace" }, false, true));
        assert!(e.preedit.is_empty());
        assert_eq!(
            e.model.value(),
            if forward {
                "one\r\ntwo \r\nlast"
            } else {
                "one\r\nthree\r\nlast"
            }
        );
        e.key("z", false, true);
        assert_eq!(e.model.value(), "one\r\ntwo three\r\nlast");
        assert_eq!(e.model.caret(), 9);
    }
}
#[test]
fn vertical_navigation_retains_desired_column_across_short_rows() {
    let mut e = Editor::new(
        Input {
            value: "abcdefgh\nx\nabcdefgh".into(),
            multiline: true,
            bounds: [0., 0., 300., 100.],
            ..Input::default()
        },
        vec![],
    )
    .unwrap();
    e.model.set_selection(7, 7).unwrap();
    e.key("ArrowDown", false, false);
    assert_eq!(e.model.caret(), 10);
    e.key("ArrowDown", false, false);
    assert_eq!(
        e.model.caret(),
        18,
        "return to original column after short row"
    );
    assert!(!e.key("Shift", true, false));
    e.key("ArrowUp", true, false);
    assert!(!e.key("Shift", true, false));
    e.key("ArrowUp", true, false);
    assert_eq!(e.model.caret(), 7);
    assert_eq!(e.model.anchor(), 18);
    e.key("ArrowRight", false, false);
    e.key("ArrowUp", false, false);
    e.key("ArrowUp", false, false);
    assert_eq!(
        e.model.caret(),
        7,
        "horizontal collapse resets desired column"
    );
    e.hit(0., 1., false);
    e.key("ArrowDown", false, false);
    e.key("ArrowDown", false, false);
    assert_eq!(e.model.caret(), 11, "pointer resets desired column");
    e.model.set_selection(7, 7).unwrap();
    e.preferred_x = None;
    e.key("ArrowDown", false, false);
    e.content(true);
    e.content(false);
    e.content(true);
    assert!(
        e.preferred_x.is_none(),
        "focus transition resets desired column"
    );
}
#[test]
fn multiline_crlf_and_lf_have_identical_visible_content_and_selection_geometry() {
    for selected in [false, true] {
        let mut crlf = Editor::new(
            Input {
                value: "first\r\nlast".into(),
                multiline: true,
                bounds: [0., 0., 200., 80.],
                ..Input::default()
            },
            vec![],
        )
        .unwrap();
        let mut lf = Editor::new(
            Input {
                value: "first\nlast".into(),
                multiline: true,
                bounds: [0., 0., 200., 80.],
                ..Input::default()
            },
            vec![],
        )
        .unwrap();
        crlf.reduced_motion(true);
        lf.reduced_motion(true);
        if selected {
            crlf.model.select_all();
            lf.model.select_all();
        }
        assert_eq!(crlf.content(true), lf.content(true), "selected={selected}");
        assert_eq!(crlf.caret_rect(), lf.caret_rect());
        assert_eq!(
            crlf.model.value(),
            "first\r\nlast",
            "render must preserve bytes"
        );
    }
}
#[test]
fn multiline_hit_and_vertical_keys_do_not_land_inside_crlf() {
    let mut e = Editor::new(
        Input {
            value: "first\r\nlast".into(),
            multiline: true,
            bounds: [0., 0., 500., 100.],
            ..Input::default()
        },
        vec![],
    )
    .unwrap();
    e.hit(1000., 1., false);
    assert_eq!(e.model.caret(), 5, "first row right edge");
    e.hit(1000., 30., false);
    assert_eq!(e.model.caret(), 11, "last row right edge");
    e.key("ArrowUp", false, false);
    assert_ne!(e.model.caret(), 6);
    e.hit(1000., 1., false);
    e.key("ArrowDown", true, false);
    assert_eq!(e.model.selected_text(), "\r\nlast");
    for row in [1., 30.] {
        for x in -10..1000 {
            e.hit(x as f32, row, false);
            assert_ne!(e.model.caret(), 6, "CRLF interior at x{x} row{row}");
        }
    }
}
#[test]
fn multiline_end_never_splits_crlf_and_shift_deletion_preserves_line_break() {
    let value = "e\u{301}\r\nnext";
    let mut e = Editor::new(
        Input {
            value: value.into(),
            multiline: true,
            ..Input::default()
        },
        vec![],
    )
    .unwrap();
    e.model.home(false);
    assert!(e.key("End", false, false));
    assert_eq!(e.model.caret(), "e\u{301}".len());
    e.model.home(false);
    assert!(e.key("End", true, false));
    assert_eq!(e.model.selected_text(), "e\u{301}");
    e.key("Backspace", false, false);
    assert_eq!(e.model.value(), "\r\nnext");
    e.key("z", false, true);
    assert_eq!(e.model.value(), value);
    e.model.set_selection(4, 4).unwrap(); // interior scalar boundary between CR and LF
    e.key("End", false, false);
    assert_eq!(e.model.caret(), 3);
    e.model.end(false);
    e.key("Home", false, false);
    assert_eq!(e.model.caret(), 5);
    e.key("End", false, false);
    assert_eq!(e.model.caret(), value.len());
}
#[test]
fn explicit_editor_keys_cancel_marked_text_without_recording_it_in_history() {
    let mut e = Editor::new(
        Input {
            value: "one two".into(),
            ..Input::default()
        },
        vec![],
    )
    .unwrap();
    e.insert("!");
    let committed = e.model.value().to_owned();
    e.set_preedit("未確定");
    assert!(!e.key("unknown", false, false));
    assert_eq!(e.preedit, "未確定");
    assert!(e.key("z", false, true));
    assert!(e.preedit.is_empty());
    assert_eq!(e.model.value(), "one two");
    assert!(e.key("z", true, true));
    assert_eq!(e.model.value(), committed);
    assert!(e.preedit.is_empty());
    for key in [
        "ArrowLeft",
        "ArrowRight",
        "Home",
        "End",
        "Backspace",
        "Delete",
    ] {
        e.set_preedit("未確定");
        assert!(e.key(key, false, false));
        assert!(e.preedit.is_empty(), "{key}");
    }
    for key in ["ArrowLeft", "ArrowRight", "Backspace", "Delete"] {
        e.set_preedit("未確定");
        assert!(e.word_key(key, false));
        assert!(e.preedit.is_empty(), "word {key}");
    }
    assert!(!e.model.value().contains("未確定"));
}
#[test]
fn idle_content_is_clean_and_quantized_caret_changes_are_never_skipped() {
    let mut e = Editor::new(Input::default(), vec![]).unwrap();
    assert!(e.needs_content(false));
    e.content(false);
    e.tick(16.);
    assert!(!e.needs_content(false));
    assert!(e.needs_content(true));
    let mut cached = e.content(true);
    for _ in 0..1100 {
        e.tick(1.);
        if e.needs_content(true) {
            cached = e.content(true);
        }
        assert_eq!(cached, e.clone().content(true));
    }
    e.set_preedit("я");
    assert!(e.needs_content(true));
    e.content(true);
    e.set_preedit("я");
    assert!(!e.needs_content(true));
    e.set_preedit("");
    assert!(e.needs_content(true));
    e.content(true);
    e.reduced_motion(true);
    assert!(e.needs_content(true));
    e.content(true);
    e.tick(300.);
    assert!(!e.needs_content(true));
    assert!(e.needs_content(false));
}
#[test]
fn multiline_hit_clamps_rows_and_retains_unicode_byte_boundaries() {
    let spec = Input {
        value: "A\nБ🙂\n".into(),
        multiline: true,
        ..Input::default()
    };
    let mut e = Editor::new(spec, vec![]).unwrap();
    e.hit(-10., 30., false);
    assert_eq!(e.model.caret(), 2);
    e.hit(1000., 30., false);
    assert_eq!(e.model.caret(), 8);
    e.hit(1000., 10000., false);
    assert_eq!(e.model.caret(), 9);
    e.hit(-10., -10., true);
    assert_eq!(e.model.caret(), 0);
    assert_eq!(e.model.anchor(), 9);
}
#[test]
fn caret_is_smooth_resets_and_stops_without_focus() {
    let mut e = Editor::new(Input::default(), vec![]).unwrap();
    e.content(true);
    assert!(e.is_animating());
    assert_eq!(e.caret_alpha(), 1.);
    e.tick(275.);
    assert!((e.caret_alpha() - 0.5).abs() < 0.001);
    e.tick(275.);
    assert!(e.caret_alpha() < 0.001);
    e.insert("я");
    assert_eq!(e.caret_alpha(), 1.);
    e.tick(220.);
    e.content(false);
    assert!(!e.is_animating());
    e.content(true);
    e.reduced_motion(true);
    assert!(!e.is_animating());
    assert_eq!(e.caret_alpha(), 1.);
}
#[test]
fn new_unicode_runs_fade_independently_and_replacement_keeps_valid_offsets() {
    let mut e = Editor::new(Input::default(), vec![]).unwrap();
    e.insert("я");
    e.tick(110.);
    e.insert("🙂");
    let content = e.content(true);
    let runs: Vec<_> = content
        .iter()
        .filter_map(|c| {
            if let Content::Text { text, .. } = c {
                Some((text.text.as_str(), text.color[3]))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(runs, vec![("я", 128), ("🙂", 0)]);
    assert_eq!(e.model.value(), "я🙂");
    e.model.set_selection(0, 2).unwrap();
    e.insert("abc");
    e.content(true);
    assert_eq!(e.model.value(), "abc🙂");
    e.tick(220.);
    assert!(e.fades.is_empty());
    e.reduced_motion(true);
    e.insert("Z");
    assert!(e.fades.is_empty());
}
#[test]
fn soft_wrapped_lines_and_dictionary_word_movement_work_together() {
    let mut e = Editor::new(
        Input {
            value: "你好世界 Hello world from Forma".into(),
            multiline: true,
            bounds: [0., 0., 90., 80.],
            ..Input::default()
        },
        vec![],
    )
    .unwrap();
    let lines = e.soft_wrapped_lines();
    assert!(lines.len() >= 2);
    e.hit_soft_wrapped(1., e.line_height() * 1.5, false);
    assert_eq!(e.model.caret(), lines[1].byte_range.start);
    e.model.home(false);
    assert!(e.word_key("ArrowRight", false));
    assert_eq!(
        e.model.caret(),
        "你好".len(),
        "dictionary groups CJK compound 你好"
    );
    assert!(e.word_key("ArrowRight", false));
    assert_eq!(
        e.model.caret(),
        "你好世界".len(),
        "dictionary groups CJK compound 世界"
    );
    assert!(e.word_key("Backspace", false));
    assert!(e.model.value().starts_with("你好 Hello"));
    assert!(e.model.undo());
    assert_eq!(e.model.value(), "你好世界 Hello world from Forma");

    // Verify multiline + wrap: true renders multiple visual Content::Text rows and routes vertical ArrowDown/Up
    let mut wrapped_ed = Editor::new(
        Input {
            value: "Hello world from Forma soft wrapped editor".into(),
            multiline: true,
            wrap: true,
            bounds: [0., 0., 90., 80.],
            ..Input::default()
        },
        vec![],
    )
    .unwrap();
    let primitives = wrapped_ed.content(true);
    let text_rows: Vec<_> = primitives
        .iter()
        .filter(|c| matches!(c, Content::Text { .. }))
        .collect();
    assert!(
        text_rows.len() >= 2,
        "expected >= 2 visual wrapped rows in Editor::content, got {}",
        text_rows.len()
    );
    wrapped_ed.model.home(false);
    assert!(wrapped_ed.key("ArrowDown", false, false));
    assert!(
        wrapped_ed.model.caret() > 0,
        "ArrowDown in wrapped multiline editor moves caret to second soft line"
    );
}
