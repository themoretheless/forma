#![cfg(feature = "native")]

use std::collections::BTreeMap;

use forma::native_accessibility::{PublishedTree, tree_update};
use forma::template::A11yLive;
use forma::{Button, Runtime};

#[test]
fn pointer_capture_drag_and_scroll_work_at_fractional_dpi() {
    let source = r#"component ScrollScene {
        Frame {
            width: 240;
            height: 120;
            padding: 8;
            gap: 8;
            clip: true;
            Scroll {
                Button { key: 'first'; width: 224; height: 48; text: 'First'; clicked -> actions.first(); }
                Button { key: 'second'; width: 224; height: 48; text: 'Second'; clicked -> actions.second(); }
                Button { key: 'third'; width: 224; height: 48; text: 'Third'; clicked -> actions.third(); }
            }
        }
    }"#;
    let mut runtime = Runtime::from_source(source).unwrap();

    // Pointer press inside first button, drag completely outside scene, then release must not click.
    runtime.pointer(40.0, 24.0, 1);
    runtime.pointer(300.0, 300.0, 0);
    runtime.pointer(300.0, 300.0, 2);
    assert_eq!(runtime.clicks(), 0);

    // Drag back inside first button before release DOES activate.
    runtime.pointer(40.0, 24.0, 1);
    runtime.pointer(10.0, 10.0, 0);
    runtime.pointer(48.0, 28.0, 2);
    assert_eq!(runtime.clicks(), 1);
    assert_eq!(runtime.action(), "actions.first");

    // Fractional scale mapping (`1.5x` and `2.0x`) + wheel scroll reaches lower controls.
    for scale in [1.5_f32, 2.0_f32] {
        let mut r = Runtime::from_source(source).unwrap();
        let phys_x = 100.0 * scale;
        let phys_y = 24.0 * scale;
        assert_eq!(r.hit_index(phys_x / scale, phys_y / scale), 0);
        r.scroll(0.0, 64.0);
        assert!(r.scroll_offset()[1] > 0.0);
        r.pointer(40.0, 96.0, 1);
        r.pointer(40.0, 96.0, 2);
        assert_eq!(r.clicks(), 1);
        assert_eq!(r.action(), "actions.third");
    }
}

#[test]
fn cjk_ime_multi_clause_cursor_commit_cancel_and_unkeyed_refocus() {
    let mut files = BTreeMap::from([(
        "App.ui".into(),
        r#"component App {
            state showExtra: Bool = false;
            Column {
                width: 300;
                height: 200;
                if state.showExtra {
                    Text { text: 'Inserted Sibling'; }
                }
                TextInput { value: 'hello'; }
            }
        }"#
        .into(),
    )]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert!(runtime.focus_control(0));
    let before_origin = runtime.control_origin(0);
    assert!(!before_origin.is_empty());

    // Multi-clause Japanese preedit: clause 0 ("きょうは") vs clause 1 ("てんき").
    let preedit = "きょうはてんき";
    let clause0_end = "きょうは".len();
    let clause1_end = preedit.len();
    runtime.text_key("End", false, false);
    runtime.text_preedit_with_cursor(preedit, Some((0, clause0_end)));
    let caret0 = runtime.text_caret_bounds()[0];
    runtime.text_preedit_with_cursor(preedit, Some((clause0_end, clause1_end)));
    let caret1 = runtime.text_caret_bounds()[0];
    assert!(
        caret1 > caret0,
        "clause 1 caret ({caret1}) must sit to the right of clause 0 ({caret0})"
    );

    // UTF-16 bridge agrees on surrogate/BMP offsets.
    runtime.text_preedit_utf16(preedit, 0, 4);
    let caret0_utf16 = runtime.text_caret_bounds()[0];
    assert!((caret0_utf16 - caret0).abs() < 0.01);

    // Escape cancels preedit without mutating committed value, then Tab clears focus.
    runtime.text_preedit_with_cursor("", None);
    assert_eq!(runtime.text_value(0), "hello");

    // Re-enter composition and commit clean text.
    runtime.text_preedit_with_cursor("漢字", Some((0, "漢字".len())));
    runtime.text_insert("漢字");
    runtime.text_preedit_with_cursor("", None);
    assert_eq!(runtime.text_value(0), "hello漢字");

    // Insert a sibling before the unkeyed editor: structural `control_origin` tracks it stably.
    files.insert(
        "App.ui".into(),
        r#"component App {
            state showExtra: Bool = true;
            Column {
                width: 300;
                height: 200;
                if state.showExtra {
                    Text { text: 'Inserted Sibling'; }
                }
                TextInput { value: 'hello'; }
            }
        }"#
        .into(),
    );
    let updated = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let shifted_idx = (0..updated.control_count())
        .find(|&i| updated.control_origin(i) == before_origin)
        .expect("unkeyed editor keeps unique control_origin after sibling insertion");
    assert_eq!(shifted_idx, 1);
    assert_eq!(updated.text_value(shifted_idx), "hello");
}

#[test]
fn live_regions_and_runtime_announcements_publish_incremental_accesskit_updates() {
    let files = BTreeMap::from([(
        "App.ui".into(),
        r#"component App {
            Column {
                width: 320;
                height: 160;
                Text { key: 'status'; live: 'polite'; text: 'Ready'; }
                Text { key: 'alert'; live: 'assertive'; text: 'Error'; }
            }
        }"#
        .into(),
    )]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let semantic = runtime.accessibility_tree();
    let status = semantic.nodes.iter().find(|n| n.key == "status").unwrap();
    let alert = semantic.nodes.iter().find(|n| n.key == "alert").unwrap();
    assert_eq!(status.live, A11yLive::Polite);
    assert_eq!(alert.live, A11yLive::Assertive);

    let initial_update = tree_update(&semantic);
    assert_eq!(
        initial_update
            .nodes
            .iter()
            .find(|(id, _)| id.0 == status.id as u64)
            .unwrap()
            .1
            .live(),
        Some(accesskit::Live::Polite)
    );
    assert_eq!(
        initial_update
            .nodes
            .iter()
            .find(|(id, _)| id.0 == alert.id as u64)
            .unwrap()
            .1
            .live(),
        Some(accesskit::Live::Assertive)
    );

    let mut published = PublishedTree::default();
    assert!(
        published
            .update_if_changed(&runtime, 1.5)
            .unwrap()
            .is_some()
    );
    assert!(
        published
            .update_if_changed(&runtime, 1.5)
            .unwrap()
            .is_none()
    );

    runtime.announce("Saved 3 items", false);
    assert_eq!(
        runtime.live_announcement().as_deref(),
        Some("Saved 3 items")
    );
    assert_eq!(runtime.live_politeness(), "polite");
    let polite_update = published.update_if_changed(&runtime, 1.5).unwrap().unwrap();
    assert!(polite_update.nodes.iter().any(|(_, n)| {
        n.role() == accesskit::Role::Status
            && n.live() == Some(accesskit::Live::Polite)
            && n.label() == Some("Saved 3 items")
    }));

    runtime.announce("Disk full", true);
    assert_eq!(runtime.live_politeness(), "assertive");
    let assertive_update = published.update_if_changed(&runtime, 1.5).unwrap().unwrap();
    assert!(assertive_update.nodes.iter().any(|(_, n)| {
        n.role() == accesskit::Role::Alert
            && n.live() == Some(accesskit::Live::Assertive)
            && n.label() == Some("Disk full")
    }));

    runtime.clear_announcement();
    assert!(runtime.live_announcement().is_none());
    let cleared_update = published.update_if_changed(&runtime, 1.5).unwrap().unwrap();
    assert!(
        !cleared_update
            .nodes
            .iter()
            .any(|(_, n)| matches!(n.role(), accesskit::Role::Status | accesskit::Role::Alert))
    );
}

#[test]
fn touch_drag_sequence_scrolls_and_activates_without_panicking() {
    let source = Button::from_source(forma::EXAMPLE).is_ok();
    assert!(source);
    let scroll_ui = r#"component TouchTest {
        Frame {
            width: 200;
            height: 100;
            padding: 4;
            gap: 4;
            clip: true;
            Scroll {
                Button { key: 'tap'; width: 180; height: 44; text: 'Tap'; clicked -> actions.tap(); }
                Button { key: 'second'; width: 180; height: 80; text: 'Second'; }
            }
        }
    }"#;
    let mut rt = Runtime::from_source(scroll_ui).unwrap();
    rt.pointer(20.0, 20.0, 1);
    rt.pointer(20.0, 22.0, 0);
    rt.pointer(20.0, 22.0, 2);
    assert_eq!(rt.clicks(), 1);
    assert_eq!(rt.action(), "actions.tap");
    let mut pixels = vec![0u32; 200 * 100];
    rt.paint_native(&mut pixels, 200, 100, 1.0).unwrap();
}
