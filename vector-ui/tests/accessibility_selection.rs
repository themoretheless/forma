use forma::Runtime;
fn runtime(readonly: bool) -> Runtime {
    let value = serde_json::to_string("A😀\r\né").unwrap();
    let source = format!(
        "component App {{ state query: String = {value}; Column {{ width:300; height:120; TextInput {{ key:'editor'; width:240; height:40; multiline:true; readOnly:{readonly}; value <-> state.query; }} Button {{ key:'other'; text:'Other'; }} }} }}"
    );
    Runtime::from_ui_files(
        &std::collections::BTreeMap::from([
            ("App.ui".into(), source),
            ("Button.component.ui".into(), forma::BUTTON_COMPONENT.into()),
        ]),
        "App.ui",
        &Default::default(),
    )
    .unwrap()
}
#[test]
fn semantic_selection_roundtrips_original_crlf_offsets_and_readonly() {
    for readonly in [false, true] {
        let mut runtime = runtime(readonly);
        let node = runtime
            .accessibility_tree()
            .nodes
            .into_iter()
            .find(|n| n.key == "editor")
            .unwrap();
        assert!(node.actions.contains(&"set_text_selection"));
        assert!(runtime.accessibility_set_text_selection(node.id, 7, 5));
        assert_eq!(runtime.text_selected(), "é");
        assert_eq!(runtime.text_selection_utf16(), vec![6, 4]);
        let tree = runtime.accessibility_tree();
        let selected = tree.nodes.iter().find(|n| n.id == node.id).unwrap();
        assert_eq!(selected.selection_utf16, Some([7, 5]));
        assert!(selected.focused);
        assert!(runtime.accessibility_set_text_selection(node.id, 7, 5));
        assert_eq!(runtime.text_value(0), "A😀\r\né");
        assert!(runtime.accessibility_set_text_selection(node.id, 2, 2));
        assert_eq!(
            runtime.text_selection_utf16(),
            vec![3, 3],
            "surrogate interior rounds forward"
        );
    }
}
#[test]
fn invalid_disabled_and_noneditor_actions_do_not_steal_focus() {
    let mut runtime = runtime(false);
    runtime.focus_control(1);
    let tree = runtime.accessibility_tree();
    let editor = tree.nodes.iter().find(|n| n.key == "editor").unwrap().id;
    let button = tree.nodes.iter().find(|n| n.key == "other").unwrap().id;
    assert!(!runtime.accessibility_set_text_selection(usize::MAX, 0, 0));
    assert!(!runtime.accessibility_set_text_selection(button, 0, 0));
    runtime.set_binding_disabled(0, true).unwrap();
    assert!(!runtime.accessibility_set_text_selection(editor, 0, 0));
    assert_eq!(runtime.focused_index(), 1);
}
#[test]
fn semantic_action_respects_modal_scope_and_keeps_prior_history() {
    let source = "component App { Frame { width:300; height:160; Button { key:'base'; } Modal { key:'dialog'; width:260; height:120; Button { key:'top'; } } } }";
    let mut runtime=Runtime::from_sources(source,"component Button { Rectangle { ContentInput { width:80; height:30; value:'seed'; } PointerArea { clicked -> events.clicked(); } } }").unwrap();
    let tree = runtime.accessibility_tree();
    let base = tree.nodes.iter().find(|n| n.key == "base").unwrap();
    let top = tree.nodes.iter().find(|n| n.key == "top").unwrap();
    assert!(base.modal_blocked);
    assert!(!base.actions.contains(&"set_text_selection"));
    assert!(!runtime.accessibility_set_text_selection(base.id, 0, 4));
    assert!(runtime.accessibility_set_text_selection(top.id, 4, 4));
    runtime.text_insert("!");
    runtime.text_preedit("未確定");
    assert!(runtime.accessibility_set_text_selection(top.id, 5, 0));
    assert_eq!(runtime.text_selected(), "seed!");
    runtime.text_key("z", false, true);
    assert_eq!(runtime.text_value(1), "seed");
    assert_eq!(runtime.text_value(0), "seed");
}
#[test]
fn semantic_set_value_is_atomic_undoable_and_updates_bindings() {
    let mut runtime = runtime(false);
    let node = runtime
        .accessibility_tree()
        .nodes
        .into_iter()
        .find(|n| n.key == "editor")
        .unwrap()
        .id;
    runtime.focus_control(0);
    runtime.text_key("a", false, true);
    assert!(runtime.accessibility_set_text_value(node, "你好\r\n🙂"));
    assert_eq!(runtime.text_value(0), "你好\n🙂");
    runtime.sync_ui_bindings_native().unwrap();
    let state: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
    assert_eq!(state["query"], "你好\n🙂");
    runtime.text_key("z", false, true);
    assert_eq!(runtime.text_value(0), "A😀\r\né");
    runtime.text_key("z", true, true);
    assert_eq!(runtime.text_value(0), "你好\n🙂");
    assert!(runtime.accessibility_set_text_value(node, ""));
    assert_eq!(runtime.text_value(0), "");
    runtime.text_key("z", false, true);
    assert_eq!(runtime.text_value(0), "你好\n🙂");
}
#[test]
fn refused_semantic_value_preserves_focus_and_selection() {
    for readonly in [false, true] {
        let mut runtime = runtime(readonly);
        let node = runtime
            .accessibility_tree()
            .nodes
            .into_iter()
            .find(|n| n.key == "editor")
            .unwrap()
            .id;
        runtime.focus_control(1);
        assert!(!runtime.accessibility_set_text_value(node, &"x".repeat(100_001)));
        assert!(!runtime.accessibility_set_text_value(usize::MAX, "bad"));
        if readonly {
            assert!(!runtime.accessibility_set_text_value(node, "bad"));
        } else {
            runtime.set_binding_disabled(0, true).unwrap();
            assert!(!runtime.accessibility_set_text_value(node, "bad"));
        }
        assert_eq!(runtime.focused_index(), 1);
        assert_eq!(runtime.text_value(0), "A😀\r\né");
    }
}
