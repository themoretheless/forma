use forma::control_state::TextEdit;
#[test]
fn line_deletion_preserves_crlf_and_one_undo_restores_original_selection() {
    for newline in ["\n", "\r\n"] {
        let value = format!("alpha{newline}é👨‍👩‍👧‍👦omega{newline}last");
        let start = 5 + newline.len();
        let end = value.rfind(newline).unwrap();
        for forward in [false, true] {
            let mut edit = TextEdit::with_multiline(value.clone(), true).unwrap();
            let at = start + "é".len();
            edit.set_selection(at, at).unwrap();
            edit.delete_to_line_edge(forward);
            let expected = if forward {
                format!("{}{}", &value[..at], &value[end..])
            } else {
                format!("{}{}", &value[..start], &value[at..])
            };
            assert_eq!(edit.value(), expected);
            assert!(edit.undo());
            assert_eq!(edit.value(), value);
            assert_eq!(edit.caret(), at);
            assert!(edit.redo());
            assert_eq!(edit.value(), expected);
        }
    }
}
#[test]
fn line_deletion_rounds_partial_grapheme_selection_and_keeps_history() {
    let value = "AéB\r\nlast";
    let mut edit = TextEdit::with_multiline(value.into(), true).unwrap();
    edit.set_selection(2, 4).unwrap();
    edit.delete_to_line_edge(false);
    assert_eq!(edit.value(), "AB\r\nlast");
    edit.undo();
    assert_eq!(edit.selection(), 2..4);
    assert_eq!(edit.value(), value);
}

#[test]
fn line_edges_and_scalar_interior_crlf_do_not_remove_separator() {
    for forward in [false, true] {
        let mut edit = TextEdit::with_multiline("first\r\nlast".into(), true).unwrap();
        edit.set_selection(6, 6).unwrap();
        edit.delete_to_line_edge(forward);
        assert_eq!(
            edit.value(),
            if forward { "first\r\nlast" } else { "\r\nlast" }
        );
    }
}
#[test]
fn line_selection_deletion_realigns_caret_when_neighbors_join_a_new_cluster() {
    for (value, start, end, expected) in [
        ("🇬X🇪", 4, 5, "🇬🇪"),
        ("👨‍X👩", 7, 8, "👨‍👩"),
        ("🇬\r\n🇪", 4, 6, "🇬🇪"),
    ] {
        for forward in [false, true] {
            let mut edit = TextEdit::with_multiline(value.into(), true).unwrap();
            edit.set_selection(start, end).unwrap();
            edit.delete_to_line_edge(forward);
            assert_eq!(edit.value(), expected);
            assert_eq!(
                edit.caret(),
                expected.len(),
                "joined cluster must not contain caret"
            );
            assert!(edit.undo());
            assert_eq!(edit.value(), value);
            assert_eq!(edit.selection(), start..end);
            assert!(edit.redo());
            assert_eq!(edit.value(), expected);
            assert_eq!(edit.caret(), expected.len());
        }
    }
}

#[test]
fn runtime_multiline_metadata_comes_from_editor_spec() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { Column { TextInput { multiline:false; } TextInput { multiline:true; } Button {} } }".into())]);
    let mut files = files;
    files.insert("Button.component.ui".into(), forma::BUTTON_COMPONENT.into());
    let runtime = forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert!(!runtime.control_multiline(0));
    assert!(runtime.control_multiline(1));
    assert!(!runtime.control_multiline(2));
    assert!(!runtime.control_multiline(usize::MAX));
}

#[test]
fn runtime_selection_utf16_matches_dom_surrogates_and_direction() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        "component App { Column { TextInput { multiline:true; value:'A😀é'; } } }".into(),
    )]);
    let mut r = forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert!(r.text_selection_utf16().is_empty());
    r.focus_control(0);
    r.text_key("End", false, false);
    assert_eq!(r.text_selection_utf16(), vec![5, 5]);
    r.text_key("ArrowLeft", true, false);
    assert_eq!(r.text_selection_utf16(), vec![5, 3]);
    r.text_key("ArrowLeft", true, false);
    assert_eq!(r.text_selection_utf16(), vec![5, 1]);
    r.set_binding_value(0, "A😀\r\né", true).unwrap();
    r.text_key("End", false, false);
    assert_eq!(r.text_selection_utf16(), vec![6, 6]);
    r.text_key("ArrowLeft", true, false);
    assert_eq!(r.text_selection_utf16(), vec![6, 4]);
    assert_eq!(r.text_value(0), "A😀\r\né");
}

#[test]
fn dom_selection_import_preserves_direction_clusters_crlf_and_no_history() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        "component App { Column { TextInput { multiline:true; } } }".into(),
    )]);
    let mut r = forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert!(!r.text_set_selection_utf16(0, 1));
    r.set_binding_value(0, "A😀\r\né", true).unwrap();
    r.focus_control(0);
    assert!(r.text_set_selection_utf16(6, 1));
    assert_eq!(r.text_selected(), "😀\r\né");
    assert_eq!(r.text_selection_utf16(), vec![6, 1]);
    assert!(!r.text_set_selection_utf16(6, 1));
    r.text_set_selection_utf16(2, 2);
    assert_eq!(r.text_selection_utf16(), vec![3, 3]);
    r.text_set_selection_utf16(2, 3);
    assert_eq!(r.text_selected(), "😀");
    r.text_set_selection_utf16(4, 5);
    assert_eq!(r.text_selected(), "é");
    r.text_set_selection_utf16(u32::MAX, u32::MAX);
    assert_eq!(r.text_selection_utf16(), vec![6, 6]);
    r.text_key("z", false, true);
    assert_eq!(r.text_value(0), "A😀\r\né");
}

#[test]
fn input_accessible_name_is_stable_and_distinct_from_value() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { Column { TextInput { key:'a'; label:'Search'; placeholder:'Hint'; value:'secret'; } TextInput { key:'b'; placeholder:'Email'; } TextInput { key:'c'; value:'private'; } } }".into())]);
    let mut r = forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert_eq!(r.control_input_label(0), "Search");
    assert_eq!(r.control_input_label(1), "Email");
    assert_eq!(r.control_input_label(2), "c");
    r.focus_control(0);
    r.text_key("a", false, true);
    r.text_insert("changed");
    assert_eq!(r.control_input_label(0), "Search");
    let tree = r.accessibility_tree();
    let a = tree.nodes.iter().find(|n| n.key == "a").unwrap();
    assert_eq!(a.label, "Search");
    assert_eq!(a.value.as_deref(), Some("changed"));
}
#[test]
fn accessibility_snapshot_roundtrips_control_characters_as_valid_json() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        "component App { Column { TextInput { key:'editor'; label:'Stable'; multiline:true; } } }"
            .into(),
    )]);
    let mut r = forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let value = "quote\" slash\\ null\0 escape\u{1b} tab\t newline\n";
    r.set_binding_value(0, value, true).unwrap();
    r.focus_control(0);
    let snapshot: serde_json::Value = serde_json::from_str(&r.accessibility_tree_json())
        .expect("JSON escaping must not use Rust debug escapes");
    let node = snapshot["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["key"] == "editor")
        .unwrap();
    assert_eq!(node["value"], value);
    assert_eq!(node["label"], "Stable");
    assert_eq!(snapshot["focusedId"], node["id"]);
}
#[test]
fn readonly_input_keeps_focus_selection_copy_and_external_binding_but_refuses_user_edits() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { Column { TextInput { key:'locked'; label:'Reference'; readOnly:true; value:'A😀é'; } } }".into())]);
    let mut r = forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert!(r.control_readonly(0));
    assert!(r.focus_control(0));
    assert!(r.text_editing());
    r.text_key("a", false, true);
    assert_eq!(r.text_selected(), "A😀é");
    assert!(!r.text_insert("changed"));
    for key in ["Backspace", "Delete", "Enter"] {
        assert!(r.text_key(key, false, false));
        assert!(r.text_key(key, false, true));
    }
    for key in ["Backspace", "Delete"] {
        assert!(r.text_word_key(key, false));
    }
    r.text_preedit("marked");
    assert_eq!(r.text_value(0), "A😀é");
    assert_eq!(r.text_selected(), "A😀é");
    r.text_set_selection_utf16(5, 1);
    assert_eq!(r.text_selected(), "😀é");
    r.set_binding_value(0, "external", true).unwrap();
    assert_eq!(r.text_value(0), "external");
    assert!(r.control_readonly(0));
    let tree = r.accessibility_tree();
    let node = tree.nodes.iter().find(|n| n.key == "locked").unwrap();
    assert!(node.read_only);
    assert!(node.actions.contains(&"focus"));
    assert!(!node.actions.contains(&"set_value"));
    assert!(!node.disabled);
}
#[test]
fn live_readonly_binding_retires_ime_without_losing_committed_history() {
    let files=std::collections::BTreeMap::from([
  ("App.ui".into(),"component App { state locked: Bool = false; state query: String = 'seed'; Column { TextInput { key:'editor'; label:'Query'; readOnly:state.locked; value <-> state.query; } Toggle { key:'lock'; checked <-> state.locked; } } }".into()),
  ("Toggle.component.ui".into(),"component Toggle { prop checked: Bool = false; width:100; height:24; Rectangle { PointerArea { clicked -> events.clicked(); } } }".into()),
 ]);
    for svet in [false, true] {
        let mut r = forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        r.set_svet_document_scheduling(svet);
        r.focus_control(0);
        r.text_key("End", false, false);
        r.text_insert("!");
        r.sync_ui_bindings_native().unwrap();
        r.text_preedit("pending");
        r.activate_control(1);
        assert!(r.sync_ui_bindings_native().unwrap());
        assert!(r.control_readonly(0));
        assert_eq!(r.text_value(0), "seed!");
        assert!(r.focus_control(0));
        assert!(r.native_editor_session().is_none());
        assert!(!r.text_insert("blocked"));
        r.text_key("z", false, true);
        assert_eq!(r.text_value(0), "seed!");
        r.activate_control(1);
        assert!(r.sync_ui_bindings_native().unwrap());
        assert!(!r.control_readonly(0));
        assert!(r.focus_control(0));
        assert!(r.native_editor_session().is_some());
        r.text_key("z", false, true);
        assert_eq!(r.text_value(0), "seed");
        r.sync_ui_bindings_native().unwrap();
        let state: serde_json::Value = serde_json::from_str(&r.ui_state_json()).unwrap();
        assert_eq!(state["query"], "seed");
        assert_eq!(state["locked"], false);
        assert!(!r.ui_state_json().contains("pending"));
        assert!(!r.ui_state_json().contains("blocked"));
    }
}

#[test]
fn native_semantic_selection_offsets_match_original_crlf_value() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        "component App { Column { TextInput { key:'editor'; multiline:true; } } }".into(),
    )]);
    let mut r = forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    r.set_binding_value(0, "A😀\r\né", true).unwrap();
    r.focus_control(0);
    r.text_key("End", false, false);
    r.text_key("ArrowLeft", true, false);
    let tree = r.accessibility_tree();
    let editor = tree.nodes.iter().find(|n| n.key == "editor").unwrap();
    assert!(editor.multiline);
    assert_eq!(editor.selection_utf16, Some([7, 5]));
    assert_eq!(r.text_selection_utf16(), vec![6, 4]);
    assert_eq!(editor.value.as_deref(), Some("A😀\r\né"));
    let json: serde_json::Value = serde_json::from_str(&r.accessibility_tree_json()).unwrap();
    let node = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["key"] == "editor")
        .unwrap();
    assert_eq!(node["multiline"], true);
    assert_eq!(node["selectionUtf16"], serde_json::json!([7, 5]));
    assert!(json["nodes"][0]["selectionUtf16"].is_null());
}

#[test]
fn semantic_json_preserves_fractional_f32_values_as_legacy_f64_numbers() {
    let r=forma::Runtime::from_sources("component App { Frame { width:320.25; height:180.5; Button { key:'fraction'; width:100.3; height:30.7; } } }",forma::BUTTON_COMPONENT).unwrap();
    let tree = r.accessibility_tree();
    let json: serde_json::Value = serde_json::from_str(&r.accessibility_tree_json()).unwrap();
    for node in &tree.nodes {
        let encoded = json["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == node.id)
            .unwrap();
        let legacy = serde_json::json!(node.bounds).to_string();
        let expected: serde_json::Value = serde_json::from_str(&legacy).unwrap();
        assert_eq!(encoded["bounds"], expected);
        assert!(
            r.accessibility_tree_json()
                .contains(&format!("\"bounds\":{legacy}"))
        );
    }
}
#[test]
fn semantic_editor_revision_tracks_model_changes_but_not_caret_blink() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        "component App { Column { TextInput { value:'seed'; } } }".into(),
    )]);
    let mut r = forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    r.focus_control(0);
    let initial = r.text_model_revision();
    r.tick(100.);
    assert_eq!(r.text_model_revision(), initial);
    r.text_key("ArrowLeft", true, false);
    let selected = r.text_model_revision();
    assert_ne!(selected, initial);
    r.text_insert("X");
    assert_ne!(r.text_model_revision(), selected);
    let edited = r.text_model_revision();
    r.set_binding_value(0, "external", true).unwrap();
    assert_ne!(r.text_model_revision(), edited);
    let replaced = r.text_model_revision();
    r.text_preedit("marked");
    assert_eq!(r.text_model_revision(), replaced);
}
