use super::*;
#[test]
fn disabling_editor_cancels_preedit_capture_and_stale_release() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = 'seed'; Column { width:260; height:120; TextInput { key:'editor'; width:200; height:32; value <-> state.query; } } }".into())]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let bounds = runtime.control_bounds(0);
    let (x, y) = (bounds[0] + 10., bounds[1] + 10.);
    runtime.pointer(x, y, 0);
    runtime.pointer(x, y, 1);
    assert_eq!(runtime.captured, Some(0));
    assert!(runtime.text_editing());
    runtime.text_preedit("未確定");
    assert!(!runtime.editors[0].as_ref().unwrap().preedit.is_empty());
    let clicks = runtime.control_clicks(0);
    runtime.set_binding_disabled(0, true).unwrap();
    assert!(runtime.captured.is_none());
    assert_eq!(runtime.focused_index(), -1);
    assert!(runtime.editors[0].as_ref().unwrap().preedit.is_empty());
    assert!(!runtime.controls[0].down);
    assert!(!runtime.text_insert("blocked"));
    runtime.set_binding_disabled(0, true).unwrap();
    runtime.set_binding_disabled(0, false).unwrap();
    runtime.pointer(x, y, 2);
    assert_eq!(
        runtime.control_clicks(0),
        clicks,
        "release from retired press cannot activate"
    );
    assert!(runtime.focus_control(0));
    runtime.text_key("End", false, false);
    runtime.text_insert("-back");
    runtime.sync_ui_bindings_native().unwrap();
    assert_eq!(runtime.text_value(0), "seed-back");
    let state: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
    assert_eq!(state["query"], "seed-back");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn live_disabled_binding_retires_editor_and_reenables_without_reload() {
    let files=std::collections::BTreeMap::from([
            ("App.ui".into(),"component App { state locked: Bool = false; state query: String = 'seed'; Column { width:260; height:160; TextInput { key:'editor'; disabled:state.locked; width:200; height:32; value <-> state.query; } Toggle { key:'lock'; checked <-> state.locked; } } }".into()),
            ("Toggle.component.ui".into(),"component Toggle { prop checked: Bool = false; width:100; height:24; Rectangle { PointerArea { clicked -> events.clicked(); } } }".into()),
        ]);
    for svet in [false, true] {
        let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        runtime.set_svet_document_scheduling(svet);
        assert!(runtime.focus_control(0));
        runtime.text_key("End", false, false);
        runtime.text_insert("-kept");
        runtime.text_preedit("未確定");
        runtime.activate_control(1);
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert!(runtime.control_disabled(0));
        assert!(!runtime.text_editing());
        assert!(!runtime.focus_control(0));
        assert!(!runtime.text_insert("-blocked"));
        assert_eq!(runtime.text_value(0), "seed-kept");
        let state: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
        assert_eq!(state["locked"], true);
        assert_eq!(state["query"], "seed-kept");
        runtime.activate_control(1);
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert!(!runtime.control_disabled(0));
        assert!(runtime.focus_control(0));
        runtime.text_key("End", false, false);
        runtime.text_insert("-back");
        runtime.sync_ui_bindings_native().unwrap();
        let state: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
        assert_eq!(state["locked"], false);
        assert_eq!(state["query"], "seed-kept-back");
        assert_eq!(runtime.svet_document_telemetry().is_some(), svet);
    }
}

#[test]
fn builtin_text_input_disabled_property_controls_input_and_accessibility() {
    let files=std::collections::BTreeMap::from([("App.ui".into(), "component App { state locked: Bool = true; state a: String = 'locked'; state b: String = 'open'; Column { width:260; height:120; TextInput { key:'a'; disabled:state.locked; width:200; height:32; value <-> state.a; } TextInput { key:'b'; width:200; height:32; value <-> state.b; } } }".into())]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert!(runtime.control_disabled(0));
    assert!(!runtime.focus_control(0));
    assert!(!runtime.text_insert("blocked"));
    runtime.focus(true);
    assert_eq!(runtime.focused_index(), 1);
    runtime.text_key("End", false, false);
    runtime.text_insert("!");
    runtime.sync_ui_bindings_native().unwrap();
    let state: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
    assert_eq!(state["a"], "locked");
    assert_eq!(state["b"], "open!");
    let a11y = runtime.accessibility_tree();
    let locked = a11y.nodes.iter().find(|node| node.key == "a").unwrap();
    assert!(locked.disabled);
    assert!(locked.actions.is_empty());
    runtime
        .load_ui_files_json_native(
            &serde_json::to_string(&files).unwrap(),
            "App.ui",
            r#"{"locked":false,"a":"locked","b":"open!"}"#,
        )
        .unwrap();
    assert!(!runtime.control_disabled(0));
    assert!(runtime.focus_control(0));
    runtime.text_key("End", false, false);
    runtime.text_insert("-enabled");
    runtime.sync_ui_bindings_native().unwrap();
    let state: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
    assert_eq!(state["a"], "locked-enabled");
    assert_eq!(state["b"], "open!");
    let a11y = runtime.accessibility_tree();
    let enabled = a11y.nodes.iter().find(|node| node.key == "a").unwrap();
    assert!(!enabled.disabled);
    assert!(enabled.actions.contains(&"set_value"));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_project_reload_refuses_atomically_and_keeps_scheduler_choice() {
    let project = |value: &str| {
        serde_json::to_string(&std::collections::BTreeMap::from([(
            "App.ui", format!("component App {{ state query: String = '{value}'; Column {{ width:220; height:100; TextInput {{ key:'editor'; width:200; height:32; value <-> state.query; }} }} }}"),
        )])).unwrap()
    };
    for svet in [false, true] {
        let mut runtime = Runtime::new();
        runtime.set_svet_document_scheduling(svet);
        runtime
            .load_ui_files_json_native(&project("old"), "App.ui", "{}")
            .unwrap();
        assert_eq!(runtime.svet_document_telemetry().is_some(), svet);
        assert!(runtime.focus_control(0));
        runtime.text_insert("!");
        runtime.sync_ui_bindings_native().unwrap();
        let state = runtime.ui_state_json();
        let text = runtime.text_value(0);
        let versions = (
            runtime.revision,
            runtime.geometry_revision,
            runtime.layout_revision,
        );
        for (files, entry, initial) in [
            ("{", "App.ui", "{}"),
            ("{}", "missing.ui", "{}"),
            (
                r#"{"App.ui":"component App { Missing {} }"}"#,
                "App.ui",
                "{}",
            ),
            (project("next").as_str(), "App.ui", "{"),
        ] {
            assert!(
                runtime
                    .load_ui_files_json_native(files, entry, initial)
                    .is_err()
            );
            assert_eq!(runtime.ui_state_json(), state);
            assert_eq!(runtime.text_value(0), text);
            assert_eq!(runtime.focused_index(), 0);
            assert_eq!(
                (
                    runtime.revision,
                    runtime.geometry_revision,
                    runtime.layout_revision
                ),
                versions
            );
            assert_eq!(runtime.svet_document_telemetry().is_some(), svet);
        }
        runtime.text_insert("?");
        runtime.sync_ui_bindings_native().unwrap();
        assert!(runtime.ui_state_json().contains('?'));
        let before = (
            runtime.revision,
            runtime.geometry_revision,
            runtime.layout_revision,
        );
        runtime
            .load_ui_files_json_native(&project("next"), "App.ui", "{}")
            .unwrap();
        assert_eq!(runtime.text_value(0), "next");
        assert_eq!(runtime.svet_document_telemetry().is_some(), svet);
        assert_eq!(
            (
                runtime.revision,
                runtime.geometry_revision,
                runtime.layout_revision
            ),
            (
                before.0.wrapping_add(1),
                before.1.wrapping_add(1),
                before.2.wrapping_add(1)
            )
        );
    }
}

#[test]
fn native_source_api_refusals_preserve_scene_and_can_recover() {
    let mut runtime = model();
    assert!(runtime.focus_control(0));
    let versions = (
        runtime.revision,
        runtime.geometry_revision,
        runtime.layout_revision,
    );
    let pixels = runtime.pixels(160, 160, 1.);
    for (source, template) in [("component Bad {", VISUAL), (TWO, "component Button {")] {
        assert!(runtime.load_component_native(source, template).is_err());
        assert_eq!(
            (
                runtime.revision,
                runtime.geometry_revision,
                runtime.layout_revision
            ),
            versions
        );
        assert_eq!(runtime.focused_index(), 0);
        assert_eq!(runtime.control_count(), 2);
        assert_eq!(runtime.pixels(160, 160, 1.), pixels);
    }
    assert!(runtime.load_native("invalid markup").is_err());
    runtime.load_component_native(TWO, VISUAL).unwrap();
    assert_eq!(
        (
            runtime.revision,
            runtime.geometry_revision,
            runtime.layout_revision
        ),
        (
            versions.0.wrapping_add(1),
            versions.1.wrapping_add(1),
            versions.2.wrapping_add(1)
        )
    );
    runtime.load_native(crate::EXAMPLE).unwrap();
    assert!(runtime.control_count() > 0);
    let files = serde_json::to_string(&std::collections::BTreeMap::from([(
        "App.ui",
        include_str!("../../tests/fixtures/svet-native-ime/App.ui"),
    )]))
    .unwrap();
    for (project, entry, state) in [
        ("{", "App.ui", "{}"),
        ("{}", "missing.ui", "{}"),
        (files.as_str(), "App.ui", "{"),
    ] {
        assert!(Runtime::compile_ui_scene_artifact_native(project, entry, state).is_err());
    }
    let json = Runtime::compile_ui_scene_artifact_native(&files, "App.ui", "{}").unwrap();
    runtime.load_scene_artifact_native(&json).unwrap();
    assert_eq!(runtime.text_value(1), "seed-A");
    assert_eq!(runtime.text_value(2), "seed-B");
}

const TWO: &str = "component Demo { Frame { width:120; height:120; padding:10; gap:10; clip:true; Button { key:'first'; width:80; height:35; text:'Первая'; clicked -> actions.first(); } Button { key:'second'; width:80; height:35; text:'Вторая'; clicked -> actions.second(); } } }";
const VISUAL: &str = "component Button { Rectangle { radius:4; background:Brush { color:#ff0000; hover:#00ff00; pressed:#0000ff; transition:0ms; }; Border { width:2; background:Brush { color:#ffffff; focus:#ffff00; }; } PointerArea { clicked -> events.clicked(); } } }";
fn model() -> Runtime {
    Runtime::from_sources(TWO, VISUAL).unwrap()
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svet_retained_document_updates_editor_and_dependent_text() {
    let files = std::collections::BTreeMap::from([("App.ui".to_string(),
            "component App { state query: String = 'seed'; Column { width:260; height:100; TextInput { key:'edit'; width:200; height:32; value <-> state.query; } Text { key:'summary'; text:'Value: ${state.query}'; } } }".to_string())]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    runtime.set_svet_document_scheduling(true);
    let initial = runtime.svet_document_telemetry().unwrap();
    assert_eq!(initial.nodes, 3);
    assert_eq!(initial.edges, 2);
    for suffix in ["-one", "-two"] {
        assert!(runtime.focus_control(0));
        runtime.text_key("End", false, false);
        assert!(runtime.text_insert(suffix));
        let value = runtime.text_value(0);
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(
            runtime.ui_state_properties()["query"],
            crate::form_document::Value::Text(value.clone())
        );
        assert_eq!(runtime.text_value(0), value);
        assert_eq!(runtime.focused_index(), 0);
        assert!(runtime.controls.iter().any(|control| control.template.content.iter().any(|content|
                matches!(content, crate::template::Content::Text { text, .. } if text.text == format!("Value: {value}")))));
        assert!(!runtime.sync_ui_bindings_native().unwrap());
        let telemetry = runtime.svet_document_telemetry().unwrap();
        assert_eq!(telemetry.nodes, initial.nodes);
        assert_eq!(telemetry.slots, initial.slots);
    }
    // Separate windows/runtimes have independent owned graphs.
    let mut other = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    other.set_svet_document_scheduling(true);
    assert_eq!(other.text_value(0), "seed");
    assert!(!other.sync_ui_bindings_native().unwrap());
    runtime.set_svet_document_scheduling(false);
    assert!(runtime.svet_document_telemetry().is_none());
    runtime.text_insert("-legacy");
    assert!(runtime.sync_ui_bindings_native().unwrap());
    runtime.set_svet_document_scheduling(true);
    assert!(!runtime.sync_ui_bindings_native().unwrap());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retained_document_recovers_after_render_error_without_losing_input() {
    for (svet, switch, model_fix) in [(false, false), (true, false), (false, true), (true, true)]
        .into_iter()
        .flat_map(|(svet, switch)| [(svet, switch, false), (svet, switch, true)])
    {
        let mut files = std::collections::BTreeMap::from([("App.ui".to_string(),
                "component App { state query: String = 'seed'; state agreed: Bool = false; Column { width:260; height:100; TextInput { key:'editor'; width:200; height:32; value <-> state.query; } Toggle { key:'toggle'; checked <-> state.agreed; } Text { key:'summary'; text: state.query == 'bad' ? [1] : state.query; } } }".to_string())]);
        files.insert("Toggle.component.ui".into(), "component Toggle { prop checked: Bool = false; width:100; height:24; Rectangle { PointerArea { clicked -> events.clicked(); } } }".into());
        let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        runtime.set_svet_document_scheduling(svet);
        let initial_graph = runtime
            .svet_document_telemetry()
            .map(|t| (t.nodes, t.edges, t.slots));
        runtime.set_binding_value(0, "bad", true).unwrap();
        assert!(runtime.sync_ui_bindings_native().is_err());
        assert_eq!(
            runtime.ui_state_properties()["query"],
            crate::form_document::Value::Text("bad".into())
        );
        assert!(runtime.ui_render_pending);
        // Multiple activations while the template is broken must toggle
        // the current model, rather than the last rendered false value.
        for checked in [true, false] {
            runtime.activate_control(1);
            assert!(runtime.sync_ui_bindings_native().is_err());
            assert_eq!(
                runtime.ui_state_properties()["agreed"],
                crate::form_document::Value::Bool(checked)
            );
        }
        if switch {
            runtime.set_svet_document_scheduling(!svet);
        }
        if model_fix {
            runtime.ui_document.as_mut().unwrap().states[0].insert(
                "query".into(),
                crate::form_document::Value::Text("recovered".into()),
            );
        } else {
            runtime.set_binding_value(0, "recovered", true).unwrap();
        }
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(
            runtime.ui_state_properties()["query"],
            crate::form_document::Value::Text("recovered".into())
        );
        assert_eq!(runtime.text_value(0), "recovered");
        assert!(!runtime.ui_render_pending);
        assert!(!runtime.sync_ui_bindings_native().unwrap());
        if !switch {
            assert_eq!(
                runtime
                    .svet_document_telemetry()
                    .map(|t| (t.nodes, t.edges, t.slots)),
                initial_graph
            );
        }
        if let Some(t) = runtime.svet_document_telemetry() {
            assert_eq!(t.tracking_depth, 0);
            assert_eq!(t.batch_depth, 0);
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn failed_binding_transaction_retries_activation_exactly_once() {
    use crate::form_document::{Properties, Value};
    for (svet, count) in [
        (false, 1),
        (false, 2),
        (false, 3),
        (true, 1),
        (true, 2),
        (true, 3),
    ] {
        let files = std::collections::BTreeMap::from([
                ("App.ui".to_string(), "component App { state agreed: Bool = false; state query: String = 'seed'; Column { width:260; height:100; Toggle { checked <-> state.agreed; } TextInput { width:200; height:32; value <-> state.query; } } }".to_string()),
                ("Toggle.component.ui".to_string(), "component Toggle { prop checked: Bool = false; width:100; height:24; Rectangle { PointerArea { clicked -> events.clicked(); } } }".to_string()),
            ]);
        let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        runtime.set_svet_document_scheduling(svet);
        let document = runtime.ui_document.as_mut().unwrap();
        let Some(Value::Object(bindings)) = document.root.children[1].props.get_mut("__bindings")
        else {
            panic!("editor bindings");
        };
        bindings.insert("value".into(), Value::List(vec![]));
        for _ in 0..count {
            runtime.activate_control(0);
        }
        for _ in 0..2 {
            assert!(runtime.sync_ui_bindings_native().is_err());
            assert_eq!(runtime.ui_state_properties()["agreed"], Value::Bool(false));
            assert_eq!(runtime.last_event, Some(0));
        }
        assert_eq!(runtime.pending_ui_binding_activations(), count);
        let document = runtime.ui_document.as_mut().unwrap();
        let Some(Value::Object(bindings)) = document.root.children[1].props.get_mut("__bindings")
        else {
            panic!("editor bindings");
        };
        bindings.insert("value".into(), Value::Text("state.query".into()));
        let rendered = runtime.sync_ui_bindings_native().unwrap();
        assert_eq!(rendered, !svet || count % 2 == 1);
        assert_eq!(
            runtime.ui_state_properties()["agreed"],
            Value::Bool(count % 2 == 1)
        );
        assert_eq!(runtime.last_event, None);
        assert!(!runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(
            runtime.ui_state_properties()["agreed"],
            Value::Bool(count % 2 == 1)
        );
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn activation_queue_runtime(svet: bool) -> Runtime {
    let files = std::collections::BTreeMap::from([
            ("App.ui".to_string(), "component App { state flag: Bool = false; state query: String = 'seed'; Column { width:260; height:140; Toggle { checked <-> state.flag; } Toggle { selected <-> state.flag; } TextInput { width:200; height:32; value <-> state.query; } } }".to_string()),
            ("Toggle.component.ui".to_string(), "component Toggle { prop checked: Bool = false; prop selected: Bool = false; width:100; height:24; Rectangle { PointerArea { clicked -> events.clicked(); } } }".to_string()),
        ]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    runtime.set_svet_document_scheduling(svet);
    runtime
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn buffered_binding_activations_preserve_order_across_controls() {
    use crate::form_document::Value;
    for svet in [false, true] {
        for (order, expected) in [([0, 1], true), ([1, 0], false)] {
            let mut runtime = activation_queue_runtime(svet);
            let document = runtime.ui_document.as_mut().unwrap();
            let Some(Value::Object(bindings)) =
                document.root.children[2].props.get_mut("__bindings")
            else {
                panic!("editor bindings");
            };
            bindings.insert("value".into(), Value::List(vec![]));
            for index in order {
                runtime.activate_control(index);
                assert!(runtime.sync_ui_bindings_native().is_err());
            }
            assert_eq!(runtime.pending_ui_binding_activations(), 2);
            let document = runtime.ui_document.as_mut().unwrap();
            let Some(Value::Object(bindings)) =
                document.root.children[2].props.get_mut("__bindings")
            else {
                panic!("editor bindings");
            };
            bindings.insert("value".into(), Value::Text("state.query".into()));
            runtime.sync_ui_bindings_native().unwrap();
            assert_eq!(runtime.ui_state_properties()["flag"], Value::Bool(expected));
            assert_eq!(runtime.pending_ui_binding_activations(), 0);
            assert!(!runtime.sync_ui_bindings_native().unwrap());
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn activation_queue_overflow_is_bounded_and_requires_explicit_cancellation() {
    use crate::form_document::Value;
    let mut runtime = activation_queue_runtime(true);
    for _ in 0..=MAX_UI_ACTIVATIONS {
        runtime.activate_control(0);
    }
    assert_eq!(runtime.pending_ui_binding_activations(), MAX_UI_ACTIVATIONS);
    for _ in 0..2 {
        assert!(
            runtime
                .sync_ui_bindings_native()
                .unwrap_err()
                .contains("activation limit")
        );
        assert_eq!(runtime.ui_state_properties()["flag"], Value::Bool(false));
    }
    assert_eq!(runtime.cancel_ui_binding_activations(), MAX_UI_ACTIVATIONS);
    assert_eq!(runtime.pending_ui_binding_activations(), 0);
    assert!(!runtime.sync_ui_bindings_native().unwrap());
    runtime.activate_control(0);
    assert!(runtime.sync_ui_bindings_native().unwrap());
    assert_eq!(runtime.ui_state_properties()["flag"], Value::Bool(true));
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn keyboard_pointer_and_direct_activation_share_the_ordered_queue() {
    use crate::form_document::Value;
    let mut runtime = activation_queue_runtime(true);
    assert!(runtime.focus_control(0));
    runtime.key_event(1, true, false);
    runtime.key_event(1, false, false);
    let bounds = runtime.control_bounds(1);
    let x = bounds[0] + bounds[2] * 0.5;
    let y = bounds[1] + bounds[3] * 0.5;
    runtime.pointer(x, y, 1);
    runtime.pointer(x, y, 2);
    runtime.activate_control(0);
    assert_eq!(runtime.ui_activations.events, vec![0, 1, 0]);
    runtime.sync_ui_bindings_native().unwrap();
    assert_eq!(runtime.ui_state_properties()["flag"], Value::Bool(false));
    assert_eq!(runtime.pending_ui_binding_activations(), 0);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retained_collection_reorder_during_render_failure_keeps_event_identity() {
    use crate::form_document::{Value, props};
    for svet in [false, true] {
        let files = std::collections::BTreeMap::from([
                ("App.ui".to_string(), "component App { prop prefix: String = 'row/'; state items: List = []; state broken: Bool = false; Column { width:260; height:140; for item in state.items key props.prefix + item.id { Toggle { checked <-> item.done; } } Text { text:state.broken ? [1] : 'ok'; } } }".to_string()),
                ("Toggle.component.ui".to_string(), "component Toggle { prop checked: Bool = false; width:100; height:24; Rectangle { PointerArea { clicked -> events.clicked(); } } }".to_string()),
            ]);
        let item = |id: &str| {
            Value::Object(props(vec![
                ("id", Value::Text(id.into())),
                ("done", Value::Bool(false)),
            ]))
        };
        let state = props(vec![("items", Value::List(vec![item("a"), item("b")]))]);
        let mut runtime = Runtime::from_ui_files(&files, "App.ui", &state).unwrap();
        runtime.set_svet_document_scheduling(svet);
        let a_key = runtime.control_key(0);
        let b_key = runtime.control_key(1);
        runtime.activate_control(0);
        let doc = runtime.ui_document.as_mut().unwrap();
        doc.states[0].insert("items".into(), Value::List(vec![item("b"), item("a")]));
        doc.states[0].insert("broken".into(), Value::Bool(true));
        for _ in 0..2 {
            assert!(runtime.sync_ui_bindings_native().is_err());
        }
        assert_eq!(
            runtime.pending_ui_binding_activations(),
            0,
            "accepted toggle must not replay after render failure"
        );
        let state = runtime.ui_state_properties();
        let Value::List(items) = &state["items"] else {
            panic!("items");
        };
        let Value::Object(b) = &items[0] else {
            panic!("b");
        };
        let Value::Object(a) = &items[1] else {
            panic!("a");
        };
        assert_eq!(b["done"], Value::Bool(false));
        assert_eq!(a["done"], Value::Bool(true));
        runtime.ui_document.as_mut().unwrap().states[0].insert("broken".into(), Value::Bool(false));
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(runtime.control_key(0), b_key);
        assert_eq!(runtime.control_key(1), a_key);
        assert!(!runtime.sync_ui_bindings_native().unwrap());
    }
}
#[test]
fn retained_scroll_resize_revises_viewport_and_clamps_offset_when_empty() {
    use crate::form_document::{Properties, Value};
    let files = std::collections::BTreeMap::from([("App.ui".into(),
            "component App { state contentHeight: Number = 512; state viewWidth: Number = 320; state viewHeight: Number = 300; state items: List = []; Column { width:360; height:400; Scroll { width:state.viewWidth; height:state.viewHeight; Column { width:100; height:state.contentHeight; for item in state.items key item.id { TextInput { width:100; height:32; value <-> item.title; } } } } } }".into())]);
    let items = (0..16)
        .map(|i| {
            Value::Object(Properties::from([
                ("id".into(), Value::Text(format!("id{i}"))),
                ("title".into(), Value::Text("Row".into())),
            ]))
        })
        .collect();
    let state = Properties::from([("items".into(), Value::List(items))]);
    for svet in [false, true] {
        let mut runtime = Runtime::from_ui_files(&files, "App.ui", &state).unwrap();
        #[cfg(not(target_arch = "wasm32"))]
        runtime.set_svet_document_scheduling(svet);
        let _ = svet;
        runtime.scroll(0., 10000.);
        assert_eq!(runtime.scroll_offset()[1], 212.);
        let bounds = runtime.control_bounds(0);
        let revision = runtime.layout_revision();
        runtime.ui_document.as_mut().unwrap().states[0]
            .insert("viewWidth".into(), Value::Number(160.));
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(runtime.viewport(), vec![100., 50., 160., 300.]);
        assert_eq!(runtime.control_bounds(0), bounds);
        assert_ne!(
            runtime.layout_revision(),
            revision,
            "viewport-only resize must invalidate layout"
        );
        assert_eq!(runtime.scroll_offset()[1], 212.);
        runtime.ui_document.as_mut().unwrap().states[0]
            .insert("viewHeight".into(), Value::Number(400.));
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(runtime.scroll_offset()[1], 112.);
        runtime.ui_document.as_mut().unwrap().states[0].insert("items".into(), Value::List(vec![]));
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(runtime.control_count(), 0);
        assert_eq!(
            runtime.scroll_offset(),
            vec![0., 112.],
            "fixed empty layout still occupies content space"
        );
        let revision = runtime.layout_revision();
        runtime.ui_document.as_mut().unwrap().states[0]
            .insert("contentHeight".into(), Value::Number(200.));
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(runtime.scroll_offset(), vec![0., 0.]);
        assert_ne!(
            runtime.layout_revision(),
            revision,
            "logical extent changes invalidate layout even without controls"
        );
        assert!(!runtime.sync_ui_bindings_native().unwrap());
    }
}
#[test]
fn match_switch_render_error_does_not_apply_old_branch_input() {
    use crate::form_document::{Properties, Value};
    let files = std::collections::BTreeMap::from([("App.ui".into(),
            "component App { state mode: String = 'a'; state broken: Bool = false; state query: String = 'seed'; state other: String = 'Other'; Column { width:260; height:200; match state.mode { 'a' => { TextInput { width:200; height:32; value <-> state.query; } } _ => { TextInput { width:200; height:32; value <-> state.other; } Text { text:state.broken ? [1] : 'ok'; } } } } }".into())]);
    for svet in [false, true] {
        let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        #[cfg(not(target_arch = "wasm32"))]
        runtime.set_svet_document_scheduling(svet);
        let _ = svet;
        runtime
            .set_binding_value(0, "Obsolete input", true)
            .unwrap();
        let state = &mut runtime.ui_document.as_mut().unwrap().states[0];
        state.insert("mode".into(), Value::Text("b".into()));
        state.insert("broken".into(), Value::Bool(true));
        assert!(runtime.sync_ui_bindings_native().is_err());
        assert_eq!(
            runtime.ui_state_properties()["query"],
            Value::Text("seed".into())
        );
        assert_eq!(
            runtime.ui_state_properties()["other"],
            Value::Text("Other".into())
        );
        runtime.ui_document.as_mut().unwrap().states[0].insert("broken".into(), Value::Bool(false));
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(runtime.text_value(0), "Other");
        assert_eq!(
            runtime.ui_state_properties()["query"],
            Value::Text("seed".into())
        );
    }
}
#[test]
fn retained_model_change_rebuilds_without_widget_input() {
    use crate::form_document::{Properties, Value};
    let files = std::collections::BTreeMap::from([("App.ui".into(),
            "component App { state query: String = 'seed'; Column { width:260; height:100; TextInput { width:200; height:32; value <-> state.query; } } }".into())]);
    for svet in [false, true] {
        let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        #[cfg(not(target_arch = "wasm32"))]
        runtime.set_svet_document_scheduling(svet);
        let _ = svet;
        assert!(!runtime.sync_ui_bindings_native().unwrap());
        runtime.ui_document.as_mut().unwrap().states[0]
            .insert("query".into(), Value::Text("model".into()));
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(runtime.text_value(0), "model");
        assert!(!runtime.sync_ui_bindings_native().unwrap());
    }
}
fn pushed_state_runtime() -> Runtime {
    let files = std::collections::BTreeMap::from([("App.ui".to_string(),
        "component App { state title: String = 'Old'; state note: String = 'draft'; state user: Object = { name: 'Ada' }; state items: List = []; Column { width:360; height:400; Scroll { width:320; height:300; Column { width:200; height:900; Text { key:'title'; text: state.title; } Text { key:'user'; text: state.user.name; } TextInput { key:'note'; width:200; height:32; value <-> state.note; } for item in state.items key item.id { Text { text: item.label; } } } } } }".to_string())]);
    Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap()
}
fn static_labels(runtime: &Runtime) -> Vec<String> {
    runtime
        .accessibility_tree()
        .nodes
        .into_iter()
        .filter(|node| node.role == A11yRole::StaticText)
        .map(|node| node.label)
        .collect()
}
fn state_json(runtime: &Runtime) -> serde_json::Value {
    serde_json::from_str(&runtime.ui_state_json()).unwrap()
}
#[test]
fn pushed_state_updates_text_and_collections() {
    use crate::form_document::{Properties, Value};
    for svet in [false, true] {
        let mut runtime = pushed_state_runtime();
        #[cfg(not(target_arch = "wasm32"))]
        runtime.set_svet_document_scheduling(svet);
        let _ = svet;
        assert_eq!(static_labels(&runtime), ["Old", "Ada"]);
        assert!(runtime.set_ui_state_json_native(r#"{"title":"New","items":[{"id":"a","label":"First"},{"id":"b","label":"Second"}]}"#).unwrap());
        assert_eq!(static_labels(&runtime), ["New", "Ada", "First", "Second"]);
        // Fields that were not named, including the screen's own, are kept.
        assert_eq!(state_json(&runtime)["note"], "draft");
        assert_eq!(runtime.text_value(2), "draft");
        assert!(
            runtime
                .patch_ui_state_json_native(
                    r#"{"user.name":"Grace","items.1.label":"Other","extra.depth.flag":true}"#
                )
                .unwrap()
        );
        assert_eq!(static_labels(&runtime), ["New", "Grace", "First", "Other"]);
        assert_eq!(state_json(&runtime)["extra"]["depth"]["flag"], true);
        let state = Properties::from([
            (
                "items".to_string(),
                Value::List(vec![Value::Object(Properties::from([
                    ("id".to_string(), Value::Text("b".into())),
                    ("label".to_string(), Value::Text("Only".into())),
                ]))]),
            ),
            ("note".to_string(), Value::Text("pushed".into())),
        ]);
        assert!(runtime.set_ui_state_properties(&state).unwrap());
        assert_eq!(static_labels(&runtime), ["New", "Grace", "Only"]);
        assert_eq!(runtime.text_value(2), "pushed");
        assert!(!runtime.sync_ui_bindings_native().unwrap());
    }
}
#[test]
fn pushed_state_keeps_focus_caret_undo_and_scroll() {
    for synchronized in [true, false] {
        let mut runtime = pushed_state_runtime();
        runtime.scroll(0., 40.);
        assert!(runtime.focus_control(2));
        runtime.text_key("End", false, false);
        runtime.text_insert("-x");
        runtime.text_key("ArrowLeft", false, false);
        let selection = runtime.text_selection_utf16();
        if synchronized {
            assert!(runtime.sync_ui_bindings_native().unwrap());
        }
        let revision = runtime.revision;
        assert!(
            runtime
                .set_ui_state_json_native(r#"{"title":"New","items":[{"id":"a","label":"First"}]}"#)
                .unwrap()
        );
        assert_ne!(runtime.revision, revision);
        assert_eq!(static_labels(&runtime), ["New", "Ada", "First"]);
        assert_eq!(runtime.focused_index(), 2);
        assert_eq!(runtime.text_value(2), "draft-x");
        assert_eq!(
            state_json(&runtime)["note"],
            "draft-x",
            "typed text reaches the model with the push"
        );
        assert_eq!(runtime.text_selection_utf16(), selection);
        assert_eq!(runtime.scroll_offset(), vec![0., 40.]);
        assert!(runtime.text_key("z", false, true));
        assert_eq!(runtime.text_value(2), "draft");
    }
}
#[test]
fn refused_state_push_changes_nothing() {
    let mut empty = Runtime::new();
    assert!(empty.set_ui_state_json_native("{}").is_err());
    assert!(empty.patch_ui_state_json_native("{}").is_err());
    for svet in [false, true] {
        let mut runtime = pushed_state_runtime();
        #[cfg(not(target_arch = "wasm32"))]
        runtime.set_svet_document_scheduling(svet);
        let _ = svet;
        assert!(
            runtime
                .set_ui_state_json_native(r#"{"items":[{"id":"a","label":"First"}]}"#)
                .unwrap()
        );
        // Input that is not synchronized yet must survive every refusal.
        assert!(runtime.focus_control(2));
        runtime.text_key("End", false, false);
        runtime.text_insert("-y");
        let (state, labels, revision) = (
            state_json(&runtime),
            static_labels(&runtime),
            runtime.revision,
        );
        let deep = format!("{{\"{}\":1}}", vec!["a"; 65].join("."));
        let nested = format!("{{\"title\":{}1{}}}", "[".repeat(200), "]".repeat(200));
        let rows = |count: usize| {
            serde_json::json!({"title": "Lost", "items": (0..count).map(|i| serde_json::json!({"id": format!("r{i}"), "label": "Row"})).collect::<Vec<_>>()}).to_string()
        };
        for refused in [
            "{",
            "",
            "[1]",
            "\"title\"",
            "{\"title\":\"A\"} tail",
            nested.as_str(),
            // A text field cannot display a list, so the scene refuses the value.
            r#"{"title":[1]}"#,
            // More controls and more scene nodes than one scene admits.
            rows(1_100).as_str(),
            rows(20_000).as_str(),
        ] {
            assert!(
                runtime.set_ui_state_json_native(refused).is_err(),
                "{:.40}",
                refused
            );
            assert_eq!(state_json(&runtime), state);
        }
        for refused in [
            "{",
            "[]",
            r#"{"":1}"#,
            r#"{"user..name":1}"#,
            r#"{".title":1}"#,
            r#"{"title.":1}"#,
            deep.as_str(),
            r#"{"title.length":1}"#,
            r#"{"items.1.label":"x"}"#,
            r#"{"items.x.label":"x"}"#,
            r#"{"items.0.label.text":"x"}"#,
            // Every path of a patch is applied, or none of them.
            r#"{"title":"Lost","items.9":1}"#,
            r#"{"title":"Lost","user.name":[1]}"#,
        ] {
            assert!(
                runtime.patch_ui_state_json_native(refused).is_err(),
                "{:.40}",
                refused
            );
            assert_eq!(state_json(&runtime), state);
        }
        assert_eq!(static_labels(&runtime), labels);
        assert_eq!(runtime.revision, revision);
        assert_eq!(runtime.text_value(2), "draft-y");
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(state_json(&runtime)["note"], "draft-y");
        assert!(!runtime.sync_ui_bindings_native().unwrap());
    }
}
#[test]
fn unchanged_state_push_does_not_rebuild() {
    for svet in [false, true] {
        let mut runtime = pushed_state_runtime();
        #[cfg(not(target_arch = "wasm32"))]
        runtime.set_svet_document_scheduling(svet);
        let _ = svet;
        assert!(
            runtime
                .set_ui_state_json_native(r#"{"title":"New","items":[{"id":"a","label":"First"}]}"#)
                .unwrap()
        );
        let (revision, geometry, layout) = (
            runtime.revision,
            runtime.geometry_revision,
            runtime.layout_revision,
        );
        #[cfg(not(target_arch = "wasm32"))]
        let identity = runtime.native_instance_id;
        assert!(
            !runtime
                .set_ui_state_json_native(r#"{"title":"New","items":[{"id":"a","label":"First"}]}"#)
                .unwrap()
        );
        assert!(!runtime.set_ui_state_json_native("{}").unwrap());
        assert!(
            !runtime
                .patch_ui_state_json_native(r#"{"user.name":"Ada","items.0.label":"First"}"#)
                .unwrap()
        );
        assert!(
            !runtime
                .set_ui_state_properties(&runtime.ui_state_properties())
                .unwrap()
        );
        assert_eq!(
            (
                runtime.revision,
                runtime.geometry_revision,
                runtime.layout_revision
            ),
            (revision, geometry, layout)
        );
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(runtime.native_instance_id, identity);
    }
}
const EDITOR: &str = "component Button { Rectangle { ContentInput { width:80; height:30; value:'seed'; } PointerArea { clicked -> events.clicked(); } } }";
#[test]
fn editor_refresh_tracks_mutations_focus_and_preedit_without_touching_siblings() {
    let mut r = Runtime::from_sources(TWO, EDITOR).unwrap();
    let untouched = r.controls[1].vector_snapshot(1., false);
    r.focus_control(0);
    r.set_reduced_motion(true);
    let focused = r.vector_snapshot(1., false);
    let revision = r.geometry_revision();
    r.tick(16.);
    assert_eq!(r.geometry_revision(), revision);
    assert!(Arc::ptr_eq(&focused, &r.vector_snapshot(1., false)));
    r.text_preedit("я");
    let preedit = r.geometry_revision();
    assert!(preedit > revision);
    r.text_preedit("я");
    assert_eq!(r.geometry_revision(), preedit);
    r.text_insert("🙂");
    assert!(r.text_value(0).contains('🙂'));
    assert!(r.geometry_revision() > preedit);
    assert!(
        r.controls[0].template.content.iter().any(
            |c| matches!(c,crate::template::Content::Text{text,..} if text.text.contains('🙂'))
        )
    );
    r.text_key("z", false, true);
    assert_eq!(r.text_value(0), "seed");
    r.text_key("y", false, true);
    assert!(r.text_value(0).contains('🙂'));
    assert!(Arc::ptr_eq(
        &untouched,
        &r.controls[1].vector_snapshot(1., false)
    ));
    r.text_preedit("pending");
    r.focus(false);
    assert!(r.editors[0].as_ref().unwrap().preedit.is_empty());
    assert!(!r.is_animating());
}
#[test]
fn editor_reconciliation_preserves_committed_history_but_drops_preedit() {
    let mut previous = Runtime::from_sources(TWO, EDITOR).unwrap();
    assert!(previous.focus_control(0));
    previous.text_key("End", false, false);
    previous.text_insert("—committed");
    let committed = previous.text_value(0);
    previous.text_key("a", false, true);
    let selection = previous.text_selected();
    previous.text_preedit("未確定");
    assert_eq!(previous.editors[0].as_ref().unwrap().preedit, "未確定");
    let swapped = TWO
        .replace("'first'", "'temporary'")
        .replace("'second'", "'first'")
        .replace("'temporary'", "'second'");
    let mut next = Runtime::from_sources(&swapped, EDITOR).unwrap();
    next.preserve_interaction(&previous);
    assert_eq!(next.focused_index(), 1);
    assert_eq!(next.text_value(1), committed);
    assert_eq!(next.text_selected(), selection);
    assert!(
        next.editors
            .iter()
            .flatten()
            .all(|editor| editor.preedit.is_empty())
    );
    assert_eq!(next.text_value(0), "seed");
    assert!(next.text_key("z", false, true));
    assert_eq!(next.text_value(1), "seed");
    assert_eq!(next.text_value(0), "seed");
    // Reconciliation is read-only with respect to the previous live scene.
    assert_eq!(previous.text_value(0), committed);
    assert_eq!(previous.editors[0].as_ref().unwrap().preedit, "未確定");
    assert_eq!(previous.text_selected(), selection);
}
#[test]
fn editor_reconciliation_uses_the_same_unique_keys_as_control_state() {
    let mut previous = Runtime::from_sources(TWO, EDITOR).unwrap();
    previous.focus_control(0);
    previous.text_insert("edited");
    let edited = previous.text_value(0);
    let swapped = TWO
        .replace("'first'", "'temporary'")
        .replace("'second'", "'first'")
        .replace("'temporary'", "'second'");
    let mut next = Runtime::from_sources(&swapped, EDITOR).unwrap();
    next.preserve_interaction(&previous);
    assert_eq!(next.text_value(1), edited);
    assert_eq!(next.text_value(0), "seed");
    assert!(
        next.controls[1]
            .template
            .content
            .iter()
            .any(|c| matches!(c,crate::template::Content::Text{text,..} if text.text==edited))
    );
    let unnamed = TWO.replace("key:'first';", "").replace("key:'second';", "");
    let mut next = Runtime::from_sources(&unnamed, EDITOR).unwrap();
    next.preserve_interaction(&previous);
    assert_eq!(next.text_value(0), "seed");
    assert_eq!(next.text_value(1), "seed");
    let mut previous = Runtime::from_sources(&unnamed, EDITOR).unwrap();
    previous.focus_control(0);
    previous.text_insert("ambiguous");
    let mut next = Runtime::from_sources(&unnamed, EDITOR).unwrap();
    next.preserve_interaction(&previous);
    assert_eq!(next.text_value(0), "seed");
    assert_eq!(next.text_value(1), "seed");
}
#[test]
fn sibling_states_capture_and_events_are_independent() {
    let mut r = model();
    assert_eq!(r.control_count(), 2);
    assert_eq!(r.nodes()[0].children, vec![1, 2]);
    assert_eq!(r.nodes()[2].parent, Some(0));
    assert_eq!(r.control_bounds(1), vec![10., 55., 80., 35.]);
    r.pointer(30., 25., 0);
    assert!(r.control_hovered(0));
    assert!(!r.control_hovered(1));
    assert_eq!(&r.gpu_paints()[..4], &[0., 1., 0., 1.]);
    assert_eq!(&r.gpu_paints()[8..12], &[1., 0., 0., 1.]);
    r.pointer(30., 25., 1);
    assert_eq!(r.focused_index(), 0);
    r.pointer(30., 65., 0);
    r.pointer(30., 65., 2);
    assert_eq!(r.clicks(), 0);
    assert!(r.control_hovered(1));
    assert!(!r.control_hovered(0));
    r.pointer(30., 65., 1);
    r.pointer(30., 65., 2);
    assert_eq!(r.control_clicks(0), 0);
    assert_eq!(r.control_clicks(1), 1);
    assert_eq!(r.event_index(), 1);
    assert_eq!(r.action(), "actions.second");
    assert!(r.control_focused(1));
    assert!(!r.control_focused(0));
    r.pointer(30., 25., 1);
    r.pointer(-1., -1., 3);
    r.pointer(30., 25., 2);
    assert_eq!(r.clicks(), 1);
}
#[test]
fn keyboard_focus_skips_disabled_and_blur_cancels() {
    let mut r = Runtime::from_sources(
        &TWO.replace("key:'first';", "key:'first'; disabled:true;"),
        VISUAL,
    )
    .unwrap();
    r.focus(true);
    assert_eq!(r.focused_index(), 1);
    assert!(!r.focus_next(false));
    assert!(!r.focus_next(true));
    r.key_event(1, true, false);
    r.key_event(1, true, true);
    r.key_event(1, false, false);
    assert_eq!(r.control_clicks(1), 1);
    r.key_event(2, true, false);
    r.focus(false);
    r.key_event(2, false, false);
    assert_eq!(r.clicks(), 1);
    let mut r = model();
    r.focus(true);
    r.key_event(1, true, false);
    assert!(r.focus_next(false));
    r.key_event(1, false, false);
    assert_eq!(r.clicks(), 0);
    r.key_event(2, true, false);
    r.key_event(2, false, false);
    assert_eq!(r.event_index(), 1);
    assert!(r.focus_next(true));
    assert_eq!(r.focused_index(), 0);
}
#[test]
fn reconciliation_uses_keys_and_never_resumes_an_unfinished_gesture() {
    let mut previous = model();
    previous.activate_control(0);
    previous.activate_control(0);
    previous.activate_control(1);
    assert!(previous.focus_control(1));
    previous.key_event(1, true, false);
    let swapped = TWO
        .replace("'first'", "'temporary'")
        .replace("'second'", "'first'")
        .replace("'temporary'", "'second'");
    let mut next = Runtime::from_sources(&swapped, VISUAL).unwrap();
    next.preserve_interaction(&previous);
    assert_eq!(next.focused_index(), 0);
    assert_eq!(next.control_clicks(0), 1);
    assert_eq!(next.control_clicks(1), 2);
    next.key_event(1, false, false);
    next.pointer(30., 25., 2);
    assert_eq!(next.clicks(), 3);
    assert!(!next.focus_control(99));
    assert_eq!(next.focused_index(), 0);

    let mut disabled = Runtime::from_sources(
        &TWO.replace("key:'second';", "key:'second'; disabled:true;"),
        VISUAL,
    )
    .unwrap();
    disabled.preserve_interaction(&previous);
    assert_eq!(disabled.focused_index(), -1);
    assert_eq!(disabled.control_clicks(1), 1);
    assert!(!disabled.focus_control(1));
    let mut passive = Runtime::from_sources(TWO, "component Button { Rectangle {} }").unwrap();
    passive.preserve_interaction(&previous);
    assert_eq!(passive.focused_index(), -1);
    assert!(!passive.focus_control(1));

    let scroll = TWO
        .replace("height:120;", "height:70;")
        .replace("Button { key:'first'", "Scroll { Button { key:'first'")
        .replace("} } }", "} } } }");
    let mut previous = Runtime::from_sources(&scroll, VISUAL).unwrap();
    previous.scroll(0., 1000.);
    assert_eq!(previous.scroll_offset(), vec![0., 30.]);
    let mut next =
        Runtime::from_sources(&scroll.replace("height:70;", "height:95;"), VISUAL).unwrap();
    next.preserve_interaction(&previous);
    assert_eq!(next.scroll_offset(), vec![0., 5.]);
}
#[test]
fn reveal_proximity_updates_sibling_paints_without_hover_or_geometry_changes() {
    let template = VISUAL.replace("Border {", "Reveal { color:#ff8800; } Border {");
    let mut r = Runtime::from_sources(TWO, &template).unwrap();
    let geometry = r.vector_snapshot(1., false);
    let slots: Vec<_> = geometry
        .commands
        .chunks_exact(STRIDE)
        .filter(|c| c[0] == 6.)
        .map(|c| c[14])
        .collect();
    assert_eq!(slots, vec![4., 6.]);
    let old = r.gpu_paints();
    r.reveal_pointer(5., 50., true);
    assert_eq!(r.hit_index(5., 50.), -1);
    assert!(!r.control_hovered(0) && !r.control_hovered(1));
    assert_eq!(r.focused_index(), -1);
    let paints = r.gpu_paints();
    assert_eq!(&old[..16], &paints[..16]);
    assert_ne!(&old[16..24], &paints[16..24]);
    assert_ne!(&old[24..32], &paints[24..32]);
    assert!(Arc::ptr_eq(&geometry, &r.vector_snapshot(1., false)));
    r.pointer(5., 50., 3);
    assert_eq!(r.gpu_paints(), old);
    r.pointer(5., 50., 0);
    assert_eq!(
        r.gpu_paints(),
        paints,
        "native pointer uses the same proximity state"
    );
    assert_eq!(r.clicks(), 0);
}
#[test]
fn passive_controls_are_reported_but_never_focused_or_activated() {
    let passive = VISUAL.replace("PointerArea { clicked -> events.clicked(); }", "");
    let packed = format!(
        "FORMA-TEMPLATES-1\n{}\n{}{}\n{}",
        passive.len(),
        passive,
        VISUAL.len(),
        VISUAL
    );
    let mut r = Runtime::from_sources(TWO, &packed).unwrap();
    assert!(!r.control_interactive(0));
    assert!(r.control_interactive(1));
    assert!(!r.control_interactive(2));
    assert!(!r.control_disabled(0));
    // Passive geometry still participates in designer hit testing.
    assert_eq!(r.hit_index(30., 25.), 0);
    r.focus(true);
    assert_eq!(r.focused_index(), 1);
    assert!(!r.focus_next(true));
    r.key_event(1, true, false);
    r.key_event(1, false, false);
    assert_eq!(r.control_clicks(1), 1);
    r.pointer(30., 25., 1);
    r.pointer(30., 25., 2);
    r.activate_control(0);
    assert_eq!(r.focused_index(), -1);
    assert_eq!(r.control_clicks(0), 0);
    assert_eq!(r.clicks(), 1);
    assert!(r.focus_next(false));
    assert_eq!(r.focused_index(), 1);

    let mut r = Runtime::from_sources(TWO, &passive).unwrap();
    r.focus(true);
    assert_eq!(r.focused_index(), -1);
    assert!(!r.focus_next(false));
    assert!(!r.focus_next(true));
    r.activate();
    r.key_event(2, true, false);
    r.key_event(2, false, false);
    assert_eq!(r.clicks(), 0);

    let mut r = Runtime::from_sources(
        &TWO.replace("key:'second';", "key:'second'; disabled:true;"),
        &packed,
    )
    .unwrap();
    assert!(
        r.control_interactive(1),
        "disabled buttons retain their semantics"
    );
    assert!(r.control_disabled(1));
    assert!(!r.focus_next(false));
    r.activate();
    assert_eq!(r.clicks(), 0);
}
#[test]
fn paint_slots_do_not_rebuild_geometry_and_cpu_pixels_are_independent() {
    let mut r = model();
    let a = r.vector_snapshot(1., true);
    let pixels = r.pixels(120, 120, 1.);
    let shapes: Vec<_> = a
        .commands
        .chunks_exact(STRIDE)
        .filter(|c| c[0] == 4.)
        .collect();
    assert_eq!(&shapes[0][14..16], &[0., 1.]);
    assert_eq!(&shapes[1][14..16], &[2., 3.]);
    r.pointer(30., 65., 0);
    let b = r.vector_snapshot(1., true);
    assert!(Arc::ptr_eq(&a, &b));
    let hovered = r.pixels(120, 120, 1.);
    let pixel = |x: usize, y: usize| (y * 120 + x) * 4;
    assert_eq!(
        &pixels[pixel(30, 25)..pixel(30, 25) + 4],
        &hovered[pixel(30, 25)..pixel(30, 25) + 4]
    );
    assert_eq!(
        &hovered[pixel(30, 65)..pixel(30, 65) + 4],
        &[0, 255, 0, 255]
    );
    assert!(!Arc::ptr_eq(&a, &r.vector_snapshot(1.25, true)));
}
#[test]
fn reverse_painter_order_blocks_click_through_disabled_controls() {
    let source = TWO.replace("key:'second';", "key:'second'; x:10; y:10; disabled:true;");
    let mut r = Runtime::from_sources(&source, VISUAL).unwrap();
    assert_eq!(r.hit_index(30., 25.), 1);
    r.pointer(30., 25., 1);
    r.pointer(30., 25., 2);
    assert_eq!(r.clicks(), 0);
    assert_eq!(r.focused_index(), -1);
}
#[test]
fn scroll_and_rounded_clip_apply_to_all_children() {
    let source = TWO
        .replace("height:120;", "height:70; radius:12;")
        .replace("Button { key:'first'", "Scroll { Button { key:'first'")
        .replace("} } }", "} } } }");
    let mut r = Runtime::from_sources(&source, VISUAL).unwrap();
    assert_eq!(r.nodes()[1].kind, NodeKind::Scroll);
    assert_eq!(r.nodes()[1].children, vec![2, 3]);
    assert_eq!(r.hit_index(30., 65.), -1);
    let before = r.vector_snapshot(1., true);
    let tiles = r.gpu_tiles(120, 70, 1., true);
    r.scroll(0., 1000.);
    assert_eq!(r.scroll_offset(), vec![0., 30.]);
    assert_eq!(r.hit_index(30., 40.), 1);
    assert_eq!(r.hit_index(5., 40.), -1);
    // Scroll is a uniform: shared geometry survives, tiles/params move.
    assert!(Arc::ptr_eq(&before, &r.vector_snapshot(1., true)));
    assert_ne!(tiles, r.gpu_tiles(120, 70, 1., true));
    assert_eq!(&r.gpu_params(120, 70, 1., true)[12..14], &[0., 30.]);
    assert!(
        before
            .commands
            .chunks_exact(STRIDE)
            .filter(|c| c[0] == 4.)
            .all(|c| c[crate::display_list::SCROLL_MODE] == crate::display_list::SCROLL_CONTENT)
    );
    assert_eq!(r.pixels(120, 70, 1.)[3], 0);
}
#[test]
fn template_framing_is_strict_and_unicode_safe() {
    let other = VISUAL.replace("#ff0000", "#00ffff");
    let packed = format!(
        "FORMA-TEMPLATES-1\n{}\n{}{}\n{}",
        VISUAL.len(),
        VISUAL,
        other.len(),
        other
    );
    let r = Runtime::from_sources(TWO, &packed).unwrap();
    assert_eq!(&r.gpu_paints()[8..12], &[0., 1., 1., 1.]);
    for invalid in [
        "FORMA-TEMPLATES-1\n",
        "FORMA-TEMPLATES-1\n2\nяx",
        "FORMA-TEMPLATES-1\n999\nx",
    ] {
        assert!(Runtime::from_sources(TWO, invalid).is_err());
    }
    assert!(Runtime::from_sources(&TWO.replace("key:'second'", "key:'first'"), VISUAL).is_err());
    assert!(Button::from_sources(TWO, VISUAL).is_err());
}
const LAYERED: &str = "component Demo { Frame { width:200; height:160; padding:10; gap:10; clip:true; Button { key:'base'; width:80; height:30; clicked -> actions.base(); } Modal { key:'ask'; x:20; y:40; width:160; height:80; backdrop:#00000080; dismissed -> actions.cancel(); Button { key:'yes'; width:60; height:24; clicked -> actions.yes(); } Button { key:'no'; width:60; height:24; clicked -> actions.no(); } } } }";
#[test]
fn open_layer_blocks_the_base_scene_traps_focus_and_dismisses_on_escape_or_backdrop() {
    let mut r = Runtime::from_sources(LAYERED, VISUAL).unwrap();
    assert_eq!(
        (r.control_count(), r.modal_count(), r.active_modal()),
        (3, 1, 0)
    );
    assert_eq!(
        r.nodes()
            .iter()
            .filter(|n| n.kind == NodeKind::Modal)
            .count(),
        1
    );
    assert_eq!(r.nodes()[2].children, vec![3, 4]);
    assert_eq!(
        (r.control_modal(0), r.control_modal(1), r.control_modal(2)),
        (-1, 0, 0)
    );
    assert!(r.control_blocked(0) && !r.control_blocked(1));
    assert_eq!(r.control_bounds(2), vec![20., 74., 60., 24.]);
    assert_eq!(
        r.hit_index(30., 20.),
        -1,
        "base controls are covered by the backdrop"
    );
    assert_eq!(r.hit_index(30., 50.), 1);
    r.pointer(30., 20., 0);
    assert!(!r.control_hovered(0));
    // A press on the surface beside its controls neither clicks nor dismisses.
    r.pointer(150., 100., 1);
    r.pointer(150., 100., 2);
    assert_eq!((r.active_modal(), r.dismiss_count(), r.clicks()), (0, 0, 0));
    r.focus(true);
    assert_eq!(r.focused_index(), 1);
    assert!(r.focus_next(false));
    assert_eq!(r.focused_index(), 2);
    assert!(
        r.focus_next(false),
        "Tab wraps inside the layer instead of leaving the scene"
    );
    assert_eq!(r.focused_index(), 1);
    assert!(r.focus_next(true));
    assert_eq!(r.focused_index(), 2);
    r.key_event(2, true, false);
    r.key_event(2, false, false);
    assert_eq!(
        (r.control_clicks(2), r.action().as_str()),
        (1, "actions.no")
    );
    r.activate_control(0);
    assert_eq!(
        r.control_clicks(0),
        0,
        "hosts cannot activate a covered control"
    );
    r.key_event(3, true, true);
    assert_eq!(r.active_modal(), 0, "key repeat never dismisses");
    r.key_event(3, true, false);
    assert_eq!(
        (r.active_modal(), r.dismiss_count(), r.dismissed_index()),
        (-1, 1, 0)
    );
    assert_eq!(r.modal_action(0), "actions.cancel");
    assert!(!r.modal_is_open(0) && r.control_blocked(1) && !r.control_blocked(0));
    assert_eq!(r.focused_index(), -1);
    assert_eq!(r.hit_index(30., 20.), 0);
    assert_eq!(r.hit_index(30., 50.), -1, "closed layers are hidden");
    r.key_event(3, true, false);
    assert_eq!(r.dismiss_count(), 1);
    assert!(r.focus_control(0));
    assert!(r.open_modal(0));
    assert!(!r.open_modal(0));
    assert_eq!(r.focused_index(), 1, "opening moves focus into the layer");
    assert!(!r.control_hovered(0));
    r.pointer(5., 5., 1);
    assert_eq!((r.active_modal(), r.dismiss_count()), (-1, 2));
    assert_eq!(
        r.focused_index(),
        0,
        "closing restores the previously focused control"
    );
    r.pointer(5., 5., 2);
    assert_eq!(r.clicks(), 1);

    let fixed = LAYERED.replace("backdrop:#00000080;", "backdrop:#00000080; dismiss:false;");
    let mut r = Runtime::from_sources(&fixed, VISUAL).unwrap();
    assert!(!r.modal_dismissible(0));
    r.key_event(3, true, false);
    r.pointer(5., 5., 1);
    r.pointer(5., 5., 2);
    assert_eq!((r.active_modal(), r.dismiss_count()), (0, 0));
    assert!(r.close_modal(0));
    assert!(!r.close_modal(0));
    assert_eq!((r.active_modal(), r.dismiss_count()), (-1, 0));
}
#[test]
fn layers_paint_a_backdrop_above_the_base_scene_and_vanish_when_closed() {
    let mut r = Runtime::from_sources(LAYERED, VISUAL).unwrap();
    let open = r.vector_snapshot(1., true);
    let count = |list: &DisplayList, kind: f32| {
        list.commands
            .chunks_exact(STRIDE)
            .filter(|c| c[0] == kind)
            .count()
    };
    let backdrop = open
        .commands
        .chunks_exact(STRIDE)
        .find(|c| c[0] == 0. && c[8] == 0. && c[11] < 1.)
        .expect("backdrop fill");
    assert_eq!(&backdrop[4..8], &[0., 0., 200., 160.]);
    assert_eq!(count(&open, 4.), 3);
    let base_shape = open
        .commands
        .chunks_exact(STRIDE)
        .position(|c| c[0] == 4.)
        .unwrap();
    let backdrop_at = open
        .commands
        .chunks_exact(STRIDE)
        .position(|c| c[0] == 0. && c[11] < 1.)
        .unwrap();
    assert!(
        backdrop_at > base_shape,
        "the backdrop covers the base scene"
    );
    let pixels = r.pixels(200, 160, 1.);
    let at = |p: &[u8], x: usize, y: usize| p[(y * 200 + x) * 4..][..4].to_vec();
    let mut expected = [24, 30, 42, 255];
    crate::raster_cache::over(&mut expected, &[0, 0, 0, 128]);
    assert_eq!(at(&pixels, 150, 100), expected.to_vec());
    assert!(r.close_modal(0));
    let closed = r.vector_snapshot(1., true);
    assert!(!Arc::ptr_eq(&open, &closed));
    assert_eq!(count(&closed, 4.), 1);
    assert_eq!(at(&r.pixels(200, 160, 1.), 150, 100), vec![24, 30, 42, 255]);
    assert_eq!(
        at(&r.pixels(200, 160, 1.), 30, 50),
        vec![24, 30, 42, 255],
        "hidden layer controls are not painted"
    );
    assert!(r.open_modal(0));
    r.focus(false);
    assert_eq!(r.pixels(200, 160, 1.), pixels);
}
#[test]
fn layer_state_survives_reevaluation_under_the_same_key_and_declared_state() {
    let mut previous = Runtime::from_sources(LAYERED, VISUAL).unwrap();
    previous.key_event(3, true, false);
    assert!(!previous.modal_is_open(0));
    let mut next = Runtime::from_sources(LAYERED, VISUAL).unwrap();
    next.preserve_interaction(&previous);
    assert!(
        !next.modal_is_open(0),
        "a dismissed layer stays closed while its source is unchanged"
    );
    assert_eq!(next.dismiss_count(), 1);
    let closed_source = LAYERED.replace("backdrop:#00000080;", "backdrop:#00000080; open:false;");
    let mut declared = Runtime::from_sources(&closed_source, VISUAL).unwrap();
    assert!(!declared.modal_is_open(0));
    declared.focus_control(0);
    assert!(declared.open_modal(0));
    let mut next = Runtime::from_sources(&closed_source, VISUAL).unwrap();
    next.preserve_interaction(&declared);
    assert!(next.modal_is_open(0));
    assert_eq!(next.focused_index(), 1);
    assert!(next.close_modal(0));
    assert_eq!(
        next.focused_index(),
        0,
        "the focus-restore target follows control keys"
    );
    let mut reopened = Runtime::from_sources(LAYERED, VISUAL).unwrap();
    reopened.preserve_interaction(&previous);
    let mut flipped = Runtime::from_sources(&closed_source, VISUAL).unwrap();
    flipped.preserve_interaction(&reopened);
    assert!(
        !flipped.modal_is_open(0),
        "a changed declaration wins over runtime state"
    );
}
#[test]
fn disabled_focus_navigation_and_late_key_release_remain_isolated() {
    let mut runtime = model();
    assert!(runtime.focus_control(0));
    runtime.key_event(1, true, false);
    runtime.set_binding_disabled(0, true).unwrap();
    assert_eq!(runtime.focused_index(), -1);
    runtime.set_binding_disabled(0, false).unwrap();
    assert!(runtime.focus_control(1));
    runtime.key_event(1, false, false);
    assert_eq!(runtime.control_clicks(1), 0);
    assert!(runtime.focus_control(0));
    runtime.key_event(1, false, false);
    assert_eq!(runtime.control_clicks(0), 0);
    runtime.key_event(1, true, false);
    runtime.key_event(1, false, false);
    assert_eq!(runtime.control_clicks(0), 1);
    runtime.set_binding_disabled(0, true).unwrap();
    assert!(runtime.focus_next(false));
    assert_eq!(runtime.focused_index(), 1);
    runtime.focus(false);
    assert!(runtime.focus_next(true));
    assert_eq!(runtime.focused_index(), 1);
    runtime.set_binding_disabled(1, true).unwrap();
    assert!(!runtime.focus_next(false));
    assert!(!runtime.focus_next(true));
    runtime.focus(true);
    assert_eq!(runtime.focused_index(), -1);

    let mut modal = Runtime::from_sources(LAYERED, VISUAL).unwrap();
    modal.set_binding_disabled(1, true).unwrap();
    modal.set_binding_disabled(2, true).unwrap();
    assert!(
        !modal.focus_control(0),
        "background cannot receive focus through an empty modal"
    );
    for reverse in [false, true] {
        assert!(!modal.focus_next(reverse));
        assert_eq!(modal.focused_index(), -1);
    }
    modal.set_binding_disabled(2, false).unwrap();
    for reverse in [false, true, false] {
        assert!(modal.focus_next(reverse));
        assert_eq!(modal.focused_index(), 2);
    }
    modal.close_modal(0);
    assert!(modal.focus_control(0));
}

#[test]
fn scrolling_moves_only_the_base_scene_under_a_layer() {
    let source = "component Demo { Frame { width:200; height:60; padding:10; clip:true; Scroll { Button { key:'base'; width:80; height:200; clicked -> actions.base(); } } Modal { key:'ask'; x:20; y:40; width:160; height:80; Button { key:'yes'; width:60; height:24; clicked -> actions.yes(); } } } }";
    let mut r = Runtime::from_sources(source, VISUAL).unwrap();
    assert!(r.scrollable());
    assert_eq!(r.render_height(), 60.);
    r.close_modal(0);
    r.scroll(0., 1000.);
    assert!(r.scroll_offset()[1] > 0.);
    assert_eq!(r.control_bounds(1), vec![20., 40., 60., 24.]);
    assert!(r.control_bounds(0)[1] < 10.);
    r.open_modal(0);
    assert_eq!(
        r.hit_index(30., 50.),
        1,
        "layer controls ignore the scroll viewport"
    );
    let offset = r.scroll_offset();
    let base = r.control_bounds(0);
    let layer = r.control_bounds(1);
    r.pointer(30., 50., 1);
    r.scroll(0., -20.);
    assert_eq!(
        r.scroll_offset(),
        offset,
        "active modal blocks background scrolling"
    );
    assert_eq!(r.control_bounds(0), base);
    assert_eq!(r.control_bounds(1), layer);
    r.pointer(30., 50., 2);
    assert_eq!(
        r.control_clicks(1),
        1,
        "blocked scroll must not cancel modal press"
    );
    r.close_modal(0);
    r.scroll(0., -20.);
    assert!(r.scroll_offset()[1] < offset[1]);
}
#[test]
fn layout_uses_component_defaults_and_single_scene_is_compatible() {
    let r = Runtime::from_sources(
        "component Demo { Frame { gap:8; Button {} Button {} } }",
        "component Button { height:20; Rectangle {} }",
    )
    .unwrap();
    assert_eq!(r.control_bounds(1)[1], 28.);
    let old = Button::new();
    let new = Runtime::new();
    assert_eq!(old.pixels(400, 200, 1.), new.pixels(400, 200, 1.));
    assert_eq!(old.gpu_commands(1., true), new.gpu_commands(1., true));
    assert_eq!(old.gpu_paints(), new.gpu_paints());
}

#[test]
fn accessibility_tree_exports_semantic_roles_focus_and_modal_blocking() {
    let mut r = Runtime::from_sources(LAYERED, VISUAL).unwrap();
    r.focus_control(1);
    let tree = r.accessibility_tree();
    assert_eq!(tree.root_id, 0);
    assert_eq!(tree.active_modal_id, Some(2));
    assert_eq!(tree.focused_id, Some(3));
    assert_eq!(tree.nodes[0].role, A11yRole::Window);
    assert_eq!(tree.nodes[1].role, A11yRole::Button);
    assert!(tree.nodes[1].modal_blocked);
    assert!(tree.nodes[1].actions.is_empty());
    assert_eq!(tree.nodes[2].role, A11yRole::Dialog);
    assert_eq!(tree.nodes[3].role, A11yRole::Button);
    assert!(tree.nodes[3].focused);
    assert!(tree.nodes[3].actions.contains(&"click"));
    let json = r.accessibility_tree_json();
    assert!(json.contains("\"role\":\"Dialog\""));
    assert!(json.contains("\"focusedId\":3"));
}
#[test]
fn pushed_text_runs_restyle_a_text_and_label_it_for_accessibility() {
    use crate::form_document::{Properties, TextRun, TextRuns};
    let files = std::collections::BTreeMap::from([("App.ui".to_string(),
        "component App { state snippet: List = []; Column { width:360; height:200; Text { key:'hit'; width: 300; runs: state.snippet; } } }".to_string())]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let runs = TextRuns::new()
        .text("found in ")
        .bold("wiki.md")
        .run(TextRun::new(" open").color("#6fa8ff").link("open:42"));
    assert_eq!(runs.len(), 3);
    let state = Properties::from([("snippet".to_string(), runs.into_value())]);
    assert!(runtime.set_ui_state_properties(&state).unwrap());
    assert_eq!(static_labels(&runtime), ["found in wiki.md open"]);
    let b = runtime.control_bounds(0);
    let right = b[0]
        + crate::text::measure_text("found in ", 16.)
        + crate::text::measure_text_in(crate::text::FontFamily::SansBold, "wiki.md", 16.)
        + 10.;
    let y = b[1] + b[3] / 2.;
    assert_eq!(runtime.text_link_at(right, y).as_deref(), Some("open:42"));
    runtime.pointer(right, y, 1);
    runtime.pointer(right, y, 2);
    let events = runtime.take_text_link_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].link, "open:42");
    assert_eq!(events[0].key, "hit");
}

// Base: a menu button at the top left, a second button and a tooltip anchor at the bottom right.
// Layers: a two-item menu, a tooltip and a card popup, all closed and declared without backdrop.
const POPUPS: &str = "component Demo { Frame { width:300; height:200; padding:10; gap:10; clip:true; Button { key:'file'; x:10; y:10; width:80; height:30; clicked -> actions.file(); } Button { key:'other'; x:120; y:10; width:80; height:30; clicked -> actions.other(); } Button { key:'help'; x:240; y:160; width:50; height:30; clicked -> actions.help(); } Modal { key:'menu'; x:0; y:0; width:100; height:70; open:false; dismissed -> actions.closed(); Button { key:'open'; x:0; y:5; width:100; height:30; clicked -> actions.open(); } Button { key:'save'; x:0; y:35; width:100; height:30; clicked -> actions.save(); } } Modal { key:'tip'; x:0; y:0; width:120; height:40; open:false; Button { key:'tiptext'; x:0; y:0; width:120; height:40; } } Modal { key:'card'; x:0; y:0; width:200; height:150; open:false; Button { key:'inside'; x:0; y:0; width:200; height:40; clicked -> actions.inside(); } } } }";

fn popups() -> Runtime {
    let mut r = Runtime::from_sources(POPUPS, VISUAL).unwrap();
    r.set_popup("menu", PopupSpec::new("file", PopupKind::Menu))
        .unwrap();
    r.set_popup(
        "tip",
        PopupSpec::new("help", PopupKind::Tooltip).delay_ms(400.),
    )
    .unwrap();
    r
}

#[test]
fn popup_placement_flips_and_clamps_to_the_frame() {
    use super::popup::place;
    let frame = [300., 200.];
    // Room below: placed below, aligned to the anchor's left edge.
    assert_eq!(
        place(
            [10., 10., 80., 30.],
            [100., 70.],
            frame,
            Placement::Below,
            4.
        ),
        ([10., 44.], Placement::Below)
    );
    // No room below but room above: flips.
    assert_eq!(
        place(
            [240., 160., 50., 30.],
            [120., 40.],
            frame,
            Placement::Below,
            4.
        ),
        ([180., 116.], Placement::Above)
    );
    // Right side overflows, left fits: flips; the vertical start is clamped.
    assert_eq!(
        place(
            [250., 180., 40., 20.],
            [100., 50.],
            frame,
            Placement::Right,
            2.
        ),
        ([148., 150.], Placement::Left)
    );
    // Fits neither side: keeps the preferred side and clamps inside the frame.
    assert_eq!(
        place(
            [10., 90., 80., 20.],
            [100., 150.],
            frame,
            Placement::Below,
            4.
        ),
        ([10., 50.], Placement::Below)
    );
    // Larger than the frame: pinned to the origin.
    assert_eq!(
        place(
            [10., 10., 80., 20.],
            [400., 300.],
            frame,
            Placement::Below,
            4.
        )
        .0,
        [0., 0.]
    );
}

#[test]
fn popup_layers_open_anchored_without_backdrop_or_blocking() {
    let mut r = popups();
    assert_eq!(r.active_modal(), -1);
    assert!(r.open_popup("menu"));
    assert_eq!(r.popup_bounds("menu"), Some([10., 44., 100., 70.]));
    assert_eq!(r.popup_placement("menu"), Some(Placement::Below));
    // The menu's items moved with the surface.
    assert_eq!(r.control_bounds(3), vec![10., 49., 100., 30.]);
    // Not a blocking modal: no active layer, the base keeps hits, hover and clicks.
    assert_eq!(r.active_modal(), -1);
    assert!(!r.control_blocked(1));
    assert_eq!(r.hit_index(150., 20.), 1);
    assert_eq!(r.hit_index(50., 60.), 3, "the open menu is above the base");
    // A press on another base control dismisses the menu and still clicks that control.
    r.pointer(150., 20., 1);
    r.pointer(150., 20., 2);
    assert!(!r.popup_open("menu"));
    assert_eq!(r.control_clicks(1), 1);
    assert_eq!((r.dismiss_count(), r.dismissed_index()), (1, 0));
    // Pressing the anchor itself does not dismiss, so the app can toggle on `clicked`.
    assert!(r.open_popup("menu"));
    r.pointer(20., 20., 1);
    assert!(r.popup_open("menu"));
    r.pointer(20., 20., 2);
    assert_eq!(r.control_clicks(0), 1);
    assert!(r.toggle_popup("menu") && !r.popup_open("menu"));
    // A press inside the popup does not dismiss it.
    assert!(r.open_popup("menu"));
    r.pointer(50., 90., 1);
    assert!(r.popup_open("menu"));
}

#[test]
fn popup_escape_closes_a_plain_popup_without_moving_focus() {
    let mut r = popups();
    r.set_popup(
        "card",
        PopupSpec::new("other", PopupKind::Popup).placement(Placement::Right),
    )
    .unwrap();
    assert!(r.open_popup("card"));
    // Right of `other` (ends at 200) has no room for 200 px; left of it neither: clamped.
    assert_eq!(r.popup_placement("card"), Some(Placement::Right));
    assert_eq!(r.popup_bounds("card"), Some([100., 10., 200., 150.]));
    // Opening a plain popup does not move focus.
    r.focus_control(0);
    assert_eq!(r.focused_index(), 0);
    r.key_event(3, true, false);
    assert!(!r.popup_open("card"));
    assert_eq!(r.focused_index(), 0);
    // Escape with nothing open is still harmless.
    r.key_event(3, true, false);
    assert_eq!(r.dismiss_count(), 1);
}

#[test]
fn menu_keyboard_navigation_activation_and_focus_return() {
    let mut r = popups();
    assert!(r.focus_control(0));
    // Down on the focused anchor opens the menu and focuses its first item.
    r.key_event(5, true, false);
    assert!(r.popup_open("menu"));
    assert_eq!(r.focused_index(), 3);
    r.key_event(5, true, false);
    assert_eq!(r.focused_index(), 4);
    r.key_event(5, true, false);
    assert_eq!(r.focused_index(), 3, "Down wraps");
    r.key_event(4, true, false);
    assert_eq!(r.focused_index(), 4, "Up wraps");
    // Enter activates the item, closes the menu and returns focus to the anchor.
    r.key_event(2, true, false);
    r.key_event(2, false, false);
    assert_eq!(r.control_clicks(4), 1);
    assert_eq!(r.action(), "actions.save");
    assert!(!r.popup_open("menu"));
    assert_eq!(r.focused_index(), 0);
    // Escape closes and returns focus too.
    r.key_event(5, true, false);
    assert_eq!(r.focused_index(), 3);
    r.key_event(3, true, false);
    assert!(!r.popup_open("menu"));
    assert_eq!(r.focused_index(), 0);
    // A pointer click on an item closes the menu as well.
    assert!(r.open_popup("menu"));
    r.pointer(50., 60., 1);
    r.pointer(50., 60., 2);
    assert_eq!(r.control_clicks(3), 1);
    assert!(!r.popup_open("menu"));
    // Arrow keys without a menu change nothing.
    r.focus_control(1);
    r.key_event(5, true, false);
    assert_eq!(r.focused_index(), 1);
    assert!(!r.popup_open("menu"));
}

#[test]
fn tooltip_opens_after_the_hover_delay_and_never_takes_input() {
    let mut r = popups();
    assert!(!r.is_animating());
    r.pointer(260., 170., 0);
    assert!(!r.popup_open("tip"));
    assert!(r.is_animating(), "the pending delay keeps the host ticking");
    r.tick(300.);
    assert!(!r.popup_open("tip"));
    r.tick(150.);
    assert!(r.popup_open("tip"));
    assert!(!r.is_animating());
    // Above the anchor (no room below), clamped to the right edge of the frame.
    assert_eq!(r.popup_bounds("tip"), Some([180., 116., 120., 40.]));
    assert_eq!(r.popup_placement("tip"), Some(Placement::Above));
    // The tooltip is not hit: the pointer reaches whatever lies under it.
    assert_eq!(r.hit_index(200., 130.), -1);
    assert_eq!(r.focused_index(), -1);
    // Leaving the anchor closes it; leaving before the delay cancels it.
    r.pointer(150., 20., 0);
    assert!(!r.popup_open("tip"));
    r.pointer(260., 170., 0);
    r.tick(200.);
    r.pointer(150., 20., 0);
    r.tick(500.);
    assert!(!r.popup_open("tip"));
    // A press closes an open tooltip; Escape does too.
    r.pointer(260., 170., 0);
    r.tick(400.);
    assert!(r.popup_open("tip"));
    r.pointer(260., 170., 1);
    assert!(!r.popup_open("tip"));
    r.pointer(260., 170., 2);
    r.pointer(250., 175., 0);
    r.tick(400.);
    assert!(r.popup_open("tip"));
    r.key_event(3, true, false);
    assert!(!r.popup_open("tip"));
}

#[test]
fn popup_configuration_errors_and_modal_paths() {
    let mut r = Runtime::from_sources(POPUPS, VISUAL).unwrap();
    assert!(
        r.set_popup("nope", PopupSpec::new("file", PopupKind::Menu))
            .is_err()
    );
    assert!(
        r.set_popup("menu", PopupSpec::new("nope", PopupKind::Menu))
            .is_err()
    );
    assert!(
        r.set_popup("menu", PopupSpec::new("save", PopupKind::Menu))
            .is_err()
    );
    assert!(
        r.set_popup(
            "menu",
            PopupSpec::new("file", PopupKind::Menu).delay_ms(f32::NAN)
        )
        .is_err()
    );
    // Before configuration the layers are ordinary modals.
    assert!(r.open_modal(0));
    assert_eq!(r.active_modal(), 0);
    assert!(r.close_modal(0));
    r.set_popup("menu", PopupSpec::new("file", PopupKind::Menu))
        .unwrap();
    // The modal entry points drive the popup without blocking the scene.
    assert!(r.open_modal(0));
    assert_eq!(r.active_modal(), -1);
    assert!(r.popup_open("menu"));
    assert!(r.dismiss_modal(0));
    assert!(!r.popup_open("menu"));
}

#[test]
fn open_popup_follows_its_anchor_when_the_base_scrolls() {
    let source = "component Demo { Frame { width:200; height:100; padding:10; clip:true; Scroll { Button { key:'base'; width:80; height:30; } Button { key:'tall'; width:80; height:300; } } Modal { key:'pop'; x:0; y:0; width:60; height:20; open:false; Button { key:'item'; width:60; height:20; } } } }";
    let mut r = Runtime::from_sources(source, VISUAL).unwrap();
    r.set_popup("pop", PopupSpec::new("base", PopupKind::Popup).offset(0.))
        .unwrap();
    assert!(r.open_popup("pop"));
    let [x, y, ..] = r.popup_bounds("pop").unwrap();
    assert_eq!((x, y), (10., 40.));
    // The popup does not block scrolling of the base.
    r.scroll(0., 15.);
    assert_eq!(r.scroll_offset(), vec![0., 15.]);
    assert_eq!(r.popup_bounds("pop").unwrap()[1], 25.);
    assert_eq!(r.control_bounds(2)[1], 25.);
}
