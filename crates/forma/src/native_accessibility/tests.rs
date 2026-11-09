
use super::*;
#[test]
fn translates_real_editor_metadata_and_refuses_unadvertised_actions() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { Column { TextInput { key:'ref'; label:'Reference'; value:'你好🙂'; readOnly:true; multiline:true; } TextInput { key:'disabled'; disabled:true; } } }".into())]);
    let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let semantic = runtime.accessibility_tree();
    let reference = semantic.nodes.iter().find(|n| n.key == "ref").unwrap();
    let disabled = semantic.nodes.iter().find(|n| n.key == "disabled").unwrap();
    let update = tree_update(&semantic);
    assert_eq!(update.focus, id(semantic.root_id));
    let node = &update
        .nodes
        .iter()
        .find(|(key, _)| *key == id(reference.id))
        .unwrap()
        .1;
    assert_eq!(node.role(), Role::MultilineTextInput);
    assert_eq!(node.label(), Some("Reference"));
    assert_eq!(node.value(), Some("你好🙂"));
    assert!(node.is_read_only());
    assert!(node.supports_action(Action::Focus));
    assert!(node.supports_action(Action::SetTextSelection));
    assert!(!apply_action(&mut runtime, id(reference.id), Action::Click));
    assert!(!apply_action(&mut runtime, id(disabled.id), Action::Focus));
    assert!(apply_action(&mut runtime, id(reference.id), Action::Focus));
    assert_eq!(
        tree_update(&runtime.accessibility_tree()).focus,
        id(reference.id)
    );
    assert!(!apply_action(&mut runtime, NodeId(u64::MAX), Action::Focus));
}
#[test]
fn set_value_requires_text_payload_and_preserves_readonly() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = 'seed'; Column { TextInput { key:'editor'; value <-> state.query; } TextInput { key:'locked'; value:'reference'; readOnly:true; } } }".into())]);
    let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let semantic = runtime.accessibility_tree();
    let editor = semantic
        .nodes
        .iter()
        .find(|n| n.key == "editor")
        .unwrap()
        .id;
    let locked = semantic
        .nodes
        .iter()
        .find(|n| n.key == "locked")
        .unwrap()
        .id;
    let update = tree_update(&semantic);
    assert!(
        update
            .nodes
            .iter()
            .find(|(key, _)| *key == id(editor))
            .unwrap()
            .1
            .supports_action(Action::SetValue)
    );
    assert!(
        !update
            .nodes
            .iter()
            .find(|(key, _)| *key == id(locked))
            .unwrap()
            .1
            .supports_action(Action::SetValue)
    );
    assert!(!apply_action(&mut runtime, id(editor), Action::SetValue));
    assert!(!apply_action_data(
        &mut runtime,
        id(editor),
        Action::SetValue,
        Some(&accesskit::ActionData::NumericValue(2.))
    ));
    let payload = accesskit::ActionData::Value("你好\r\n🙂".into());
    assert!(!apply_action_data(
        &mut runtime,
        id(locked),
        Action::SetValue,
        Some(&payload)
    ));
    assert!(apply_action_data(
        &mut runtime,
        id(editor),
        Action::SetValue,
        Some(&payload)
    ));
    assert_eq!(runtime.text_value(0), "你好 🙂");
    runtime.sync_ui_bindings_native().unwrap();
    let state: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
    assert_eq!(state["query"], "你好 🙂");
    runtime.text_key("z", false, true);
    assert_eq!(runtime.text_value(0), "seed");
    assert_eq!(runtime.text_value(1), "reference");
}

#[test]
fn text_runs_roundtrip_graphemes_crlf_reverse_and_trailing_empty_line() {
    let files:std::collections::BTreeMap<String,String>=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = ''; Column { TextInput { key:'editor'; multiline:true; readOnly:true; value <-> state.query; } } }".into())]);
    let mut runtime = crate::Runtime::new();
    runtime
        .load_ui_files_json_native(
            &serde_json::to_string(&files).unwrap(),
            "App.ui",
            &serde_json::json!({"query":"A😀\r\né\n"}).to_string(),
        )
        .unwrap();
    let semantic = runtime.accessibility_tree();
    let editor = semantic.nodes.iter().find(|n| n.key == "editor").unwrap();
    let target = id(editor.id);
    let update = tree_update(&semantic);
    let parent = &update
        .nodes
        .iter()
        .find(|(key, _)| *key == target)
        .unwrap()
        .1;
    let children = parent.children();
    assert_eq!(children.len(), 3);
    let first = &update
        .nodes
        .iter()
        .find(|(key, _)| *key == children[0])
        .unwrap()
        .1;
    let second = &update
        .nodes
        .iter()
        .find(|(key, _)| *key == children[1])
        .unwrap()
        .1;
    assert_eq!(first.value(), Some("A😀\r\n"));
    assert_eq!(first.character_lengths(), &[1, 4, 2]);
    assert_eq!(second.character_lengths(), &[3, 1]);
    let selection = accesskit::TextSelection {
        anchor: accesskit::TextPosition {
            node: children[1],
            character_index: 1,
        },
        focus: accesskit::TextPosition {
            node: children[0],
            character_index: 1,
        },
    };
    assert!(apply_action_data(
        &mut runtime,
        target,
        Action::SetTextSelection,
        Some(&accesskit::ActionData::SetTextSelection(selection))
    ));
    assert_eq!(runtime.text_selected(), "😀\r\né");
    assert_eq!(runtime.text_selection_utf16(), vec![6, 1]);
    let roundtrip = tree_update(&runtime.accessibility_tree());
    assert_eq!(
        roundtrip
            .nodes
            .iter()
            .find(|(key, _)| *key == target)
            .unwrap()
            .1
            .text_selection(),
        Some(&selection)
    );
    let invalid = accesskit::TextSelection {
        anchor: accesskit::TextPosition {
            node: children[1],
            character_index: 999,
        },
        focus: selection.focus,
    };
    assert!(!apply_action_data(
        &mut runtime,
        target,
        Action::SetTextSelection,
        Some(&accesskit::ActionData::SetTextSelection(invalid))
    ));
    let end = accesskit::TextPosition {
        node: children[2],
        character_index: 0,
    };
    assert!(apply_action_data(
        &mut runtime,
        target,
        Action::SetTextSelection,
        Some(&accesskit::ActionData::SetTextSelection(
            accesskit::TextSelection {
                anchor: end,
                focus: end
            }
        ))
    ));
    assert_eq!(runtime.text_selection_utf16(), vec![7, 7]);
}
#[test]
fn oversized_grapheme_disables_unrepresentable_text_run_selection() {
    let value = format!("x{}", "\u{301}".repeat(300));
    assert!(text_runs(1, &value).is_none());
    let files:std::collections::BTreeMap<String,String>=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = ''; Column { TextInput { key:'editor'; value <-> state.query; } } }".into())]);
    let mut runtime = crate::Runtime::new();
    runtime
        .load_ui_files_json_native(
            &serde_json::to_string(&files).unwrap(),
            "App.ui",
            &serde_json::json!({"query":value}).to_string(),
        )
        .unwrap();
    let tree = runtime.accessibility_tree();
    let target = id(tree.nodes.iter().find(|n| n.key == "editor").unwrap().id);
    let update = tree_update(&tree);
    let node = &update
        .nodes
        .iter()
        .find(|(key, _)| *key == target)
        .unwrap()
        .1;
    assert!(!node.supports_action(Action::SetTextSelection));
    assert!(node.children().is_empty());
    assert!(node.supports_action(Action::SetValue));
}

#[test]
fn published_ids_reject_retired_scene_even_when_local_ids_and_keys_match() {
    let files=serde_json::json!({"App.ui":"component App { state query: String = 'seed'; Column { TextInput { key:'editor'; value <-> state.query; } } }"}).to_string();
    let mut runtime = crate::Runtime::new();
    runtime
        .load_ui_files_json_native(&files, "App.ui", "{}")
        .unwrap();
    let mut published = PublishedTree::default();
    let first = published.update(&runtime).unwrap();
    let target = first
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::TextInput)
        .unwrap()
        .0;
    let request = accesskit::ActionRequest {
        action: Action::SetValue,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: Some(accesskit::ActionData::Value("stale".into())),
    };
    let unchanged = published.update(&runtime).unwrap();
    assert!(unchanged.nodes.iter().any(|(id, _)| *id == target));
    runtime
        .load_ui_files_json_native(&files, "App.ui", "{\"query\":\"replacement\"}")
        .unwrap();
    assert!(
        !published.apply(&mut runtime, &request),
        "reject even before new tree publication"
    );
    let next = published.update(&runtime).unwrap();
    let new_target = next
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::TextInput)
        .unwrap()
        .0;
    assert_ne!(target, new_target);
    assert!(!published.apply(&mut runtime, &request));
    assert_eq!(runtime.text_value(0), "replacement");
    let fresh = accesskit::ActionRequest {
        target_node: new_target,
        ..request
    };
    assert!(published.apply(&mut runtime, &fresh));
    assert_eq!(runtime.text_value(0), "stale");
}
#[test]
fn published_text_positions_retire_when_text_changes_before_and_after_publication() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        "component App { Column { TextInput { key:'editor'; value:'seed'; } } }".into(),
    )]);
    let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let mut published = PublishedTree::default();
    let first = published.update(&runtime).unwrap();
    let (target, parent) = first
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::TextInput)
        .unwrap();
    let run = parent.children()[0];
    let position = accesskit::TextPosition {
        node: run,
        character_index: 1,
    };
    let request = accesskit::ActionRequest {
        action: Action::SetTextSelection,
        target_tree: TreeId::ROOT,
        target_node: *target,
        data: Some(accesskit::ActionData::SetTextSelection(
            accesskit::TextSelection {
                anchor: position,
                focus: position,
            },
        )),
    };
    assert!(published.apply(&mut runtime, &request));
    runtime.text_insert("X");
    assert!(
        !published.apply(&mut runtime, &request),
        "old positions cannot index unpublished new text"
    );
    let next = published.update(&runtime).unwrap();
    let (new_target, new_parent) = next
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::TextInput)
        .unwrap();
    assert_eq!(target, new_target);
    assert_ne!(run, new_parent.children()[0]);
    assert!(!published.apply(&mut runtime, &request));
    let position = accesskit::TextPosition {
        node: new_parent.children()[0],
        character_index: 2,
    };
    let fresh = accesskit::ActionRequest {
        data: Some(accesskit::ActionData::SetTextSelection(
            accesskit::TextSelection {
                anchor: position,
                focus: position,
            },
        )),
        ..request
    };
    assert!(published.apply(&mut runtime, &fresh));
    assert_eq!(runtime.text_selection_utf16(), vec![2, 2]);
}

#[test]
fn keyed_binding_updates_keep_parent_ids_but_retire_text_positions() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = 'seed'; Column { TextInput { key:'editor'; value <-> state.query; } } }".into())]);
    let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let scene = runtime.native_scene_identity();
    let mut published = PublishedTree::default();
    let first = published.update(&runtime).unwrap();
    let (target, parent) = first
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::TextInput)
        .unwrap();
    let old_run = parent.children()[0];
    let request = accesskit::ActionRequest {
        action: Action::SetValue,
        target_tree: TreeId::ROOT,
        target_node: *target,
        data: Some(accesskit::ActionData::Value("changed".into())),
    };
    assert!(published.apply(&mut runtime, &request));
    runtime.sync_ui_bindings_native().unwrap();
    assert_eq!(runtime.native_scene_identity(), scene);
    let next = published.update(&runtime).unwrap();
    let (new_target, new_parent) = next
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::TextInput)
        .unwrap();
    assert_eq!(target, new_target);
    assert_ne!(old_run, new_parent.children()[0]);
    assert!(published.apply(&mut runtime, &request));
}

#[test]
fn unchanged_publication_observes_text_disabled_and_source_identity_changes() {
    let files=serde_json::json!({"App.ui":"component App { Column { TextInput { key:'editor'; value:'seed'; } } }"}).to_string();
    let mut runtime = crate::Runtime::new();
    runtime
        .load_ui_files_json_native(&files, "App.ui", "{}")
        .unwrap();
    runtime.focus_control(0);
    let mut published = PublishedTree::default();
    assert!(published.update_if_changed(&runtime, 1.).unwrap().is_some());
    assert!(published.update_if_changed(&runtime, 1.).unwrap().is_none());
    runtime.text_preedit("marked");
    assert!(published.update_if_changed(&runtime, 1.).unwrap().is_none());
    runtime.text_insert("X");
    assert!(published.update_if_changed(&runtime, 1.).unwrap().is_some());
    runtime.set_binding_disabled(0, true).unwrap();
    assert!(published.update_if_changed(&runtime, 1.).unwrap().is_some());
    runtime
        .load_ui_files_json_native(&files, "App.ui", "{}")
        .unwrap();
    runtime.focus_control(0);
    assert!(published.update_if_changed(&runtime, 1.).unwrap().is_some());
    runtime
        .load_ui_files_json_native(&files, "App.ui", "{}")
        .unwrap();
    runtime.focus_control(0);
    assert!(
        published.update_if_changed(&runtime, 1.).unwrap().is_some(),
        "identical snapshot in replacement scene must retire IDs"
    );
    assert!(published.update_if_changed(&runtime, f64::NAN).is_err());
    assert!(published.update_if_changed(&runtime, 0.).is_err());
}

#[test]
fn slider_scroll_and_dialog_actions_route_and_publish_live_metadata() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state level: Number = 0.25; Column { Slider { key:'volume'; value <-> state.level; } } }".into())]);
    let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let semantic = runtime.accessibility_tree();
    let slider = semantic
        .nodes
        .iter()
        .find(|n| n.key == "volume")
        .unwrap()
        .id;
    let update = tree_update(&semantic);
    let slider_node = &update
        .nodes
        .iter()
        .find(|(key, _)| *key == id(slider))
        .unwrap()
        .1;
    assert_eq!(slider_node.role(), Role::Slider);
    assert_eq!(slider_node.min_numeric_value(), Some(0.));
    assert_eq!(slider_node.max_numeric_value(), Some(1.));
    assert!(slider_node.supports_action(Action::Increment));
    assert!(slider_node.supports_action(Action::Decrement));
    assert!(slider_node.supports_action(Action::SetValue));
    assert!(apply_action(&mut runtime, id(slider), Action::Increment));
    assert!((runtime.range_value(0) - 0.30).abs() < 1e-5);
    assert!(apply_action(&mut runtime, id(slider), Action::Decrement));
    assert!((runtime.range_value(0) - 0.25).abs() < 1e-5);
    assert!(apply_action_data(
        &mut runtime,
        id(slider),
        Action::SetValue,
        Some(&accesskit::ActionData::NumericValue(0.75))
    ));
    assert!((runtime.range_value(0) - 0.75).abs() < 1e-5);
    assert!(apply_action_data(
        &mut runtime,
        id(slider),
        Action::SetValue,
        Some(&accesskit::ActionData::Value("60%".into()))
    ));
    assert!((runtime.range_value(0) - 0.60).abs() < 1e-5);
    assert!(!apply_action_data(
        &mut runtime,
        id(slider),
        Action::SetValue,
        Some(&accesskit::ActionData::NumericValue(-1.))
    ));
    runtime.sync_ui_bindings_native().unwrap();
    let state: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
    assert!((state["level"].as_f64().unwrap() - 0.60).abs() < 1e-5);

    let layered = "component Demo { Frame { width:200; height:80; clip:true; Scroll { Button { key:'base'; width:180; height:240; clicked -> actions.base(); } } Modal { key:'dialog'; x:10; y:10; width:180; height:60; dismissed -> actions.cancel(); Button { key:'ok'; width:80; height:28; clicked -> actions.ok(); } } } }";
    let visual = "component Button { Rectangle { PointerArea { clicked -> events.clicked(); } } }";
    let mut modal_rt = crate::Runtime::from_sources(layered, visual).unwrap();
    let tree_open = modal_rt.accessibility_tree();
    let scroll_id = tree_open
        .nodes
        .iter()
        .find(|n| n.role == A11yRole::ScrollView)
        .unwrap()
        .id;
    let dialog_id = tree_open
        .nodes
        .iter()
        .find(|n| n.role == A11yRole::Dialog)
        .unwrap()
        .id;
    let update_open = tree_update(&tree_open);
    let dialog_node = &update_open
        .nodes
        .iter()
        .find(|(key, _)| *key == id(dialog_id))
        .unwrap()
        .1;
    assert_eq!(dialog_node.live(), Some(accesskit::Live::Polite));
    assert!(dialog_node.supports_action(Action::Collapse));
    assert!(
        !apply_action(&mut modal_rt, id(scroll_id), Action::ScrollDown),
        "active modal blocks background scroll"
    );
    assert!(apply_action(&mut modal_rt, id(dialog_id), Action::Collapse));
    assert_eq!(modal_rt.active_modal(), -1);
    assert_eq!(modal_rt.dismiss_count(), 1);
    assert!(apply_action(
        &mut modal_rt,
        id(scroll_id),
        Action::ScrollDown
    ));
    assert!(modal_rt.scroll_offset()[1] > 0.);
}
