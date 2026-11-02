use std::collections::BTreeMap;
use forma::{Runtime, form_document::{Document, Properties}};

#[test]
fn public_scene_artifact_retains_empty_scroll_extent() {
    let files = BTreeMap::from([("App.ui".into(),
        "component App { Column { width:360; height:400; Scroll { width:320; height:300; Column { width:100; height:512; } } } }".into())]);
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let artifact = doc.render_scene_artifact().unwrap();
    assert!(artifact.controls.is_empty());
    let mut runtime = Runtime::from_scene_artifact(&artifact).unwrap();
    assert_eq!(runtime.control_count(), 0);
    runtime.scroll(0., 10000.);
    assert_eq!(runtime.scroll_offset(), vec![0., 212.]);
    assert_eq!(runtime.viewport(), vec![20., 50., 320., 300.]);
    for svet in [false, true] {
        let mut form = forma::binding::Form::from_document(doc.clone()).unwrap();
        form.set_svet_bindings(svet);
        form.sync().unwrap();
        form.update(|runtime| runtime.scroll(0., 10000.)).unwrap();
        assert_eq!(form.runtime().scroll_offset(), vec![0., 212.]);
        assert_eq!(form.runtime().control_count(), 0);
    }
    for invalid in [[-1., 500.], [f64::NAN, 500.], [100., f64::INFINITY]] {
        let mut corrupted = artifact.clone(); corrupted.logical_content_end = Some(invalid);
        assert!(Runtime::from_scene_artifact(&corrupted).is_err());
    }
}

#[test]
fn ordinary_scene_artifact_and_extent_on_non_scroll_scene() {
    let files = BTreeMap::from([("App.ui".into(), "component App { Column { width:100; height:80; } }".into())]);
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let mut artifact = doc.render_scene_artifact().unwrap();
    assert_eq!(artifact.logical_content_end, None);
    assert!(!Runtime::from_scene_artifact(&artifact).unwrap().scrollable());
    artifact.logical_content_end = Some([100., 500.]);
    assert!(Runtime::from_scene_artifact(&artifact).is_err());
}

#[test]
fn scene_json_roundtrip_preserves_extents_and_typed_binding_values() {
    use forma::form_document::{SceneArtifact, SceneControl, Value, Expression};
    let files = BTreeMap::from([("App.ui".into(),
        "component App { Column { width:360; height:400; Scroll { width:320; height:300; Column { width:100; height:512; } } } }".into())]);
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let mut artifact = doc.render_scene_artifact().unwrap();
    let state = Properties::from([
        ("__binding_frames".into(), Value::List(vec![Value::Object(Properties::from([("item".into(), Value::Text("row".into()))]))])),
        ("expression".into(), Value::Expr("state.rows".into())),
        ("text".into(), Value::Text("state.rows".into())),
        ("ast".into(), Value::Expression(Box::new(Expression::Binary("+".into(), Value::Number(1.), Value::Number(2.))))),
    ]);
    artifact.controls.push(SceneControl { node: 7, key: "typed".into(), state: state.clone(),
        event_args: BTreeMap::from([("event".into(), vec![Value::Null, Value::Bool(true)])]) });
    let restored = SceneArtifact::from_json(&artifact.to_json().unwrap()).unwrap();
    assert_eq!(restored.controls[0].state, state);
    assert_eq!(restored.controls[0].event_args, artifact.controls[0].event_args);
    assert_eq!(restored.controls[0].node, 7);
    assert!(Runtime::from_scene_artifact(&restored).is_err(), "serialization-only mapping cannot authorize a nonexistent rendered control");
    let mut runtime = Runtime::from_scene_artifact(&doc.render_scene_artifact().unwrap()).unwrap();
    runtime.scroll(0., 10000.); assert_eq!(runtime.scroll_offset(), vec![0., 212.]);
    artifact.controls[0].state.insert("bad".into(), Value::Number(f64::NAN));
    assert!(artifact.to_json().is_err(), "nonfinite state cannot silently serialize to null");
}

#[test]
fn scene_json_refuses_unsupported_version_and_corrupt_metadata() {
    use forma::form_document::SceneArtifact;
    let files = BTreeMap::from([("App.ui".into(), "component App { Column { width:100; height:80; } }".into())]);
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let original: serde_json::Value = serde_json::from_str(&doc.render_scene_artifact().unwrap().to_json().unwrap()).unwrap();
    for (key, invalid) in [
        ("protocol", serde_json::json!("forma-scene-artifact/2")),
        ("logical_content_end", serde_json::json!([-1, 100])),
        ("logical_content_end", serde_json::json!([100])),
        ("logical_content_end", serde_json::json!(["100", 200])),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut value = original.clone(); value[key] = invalid;
        assert!(SceneArtifact::from_json(&value.to_string()).is_err());
    }
    let mut missing = original.clone(); missing.as_object_mut().unwrap().remove("logical_content_end");
    assert!(SceneArtifact::from_json(&missing.to_string()).is_err());
    assert!(SceneArtifact::from_json("{").is_err());
}


#[test]
fn scene_json_load_is_atomic_and_revises_cached_geometry() {
    let files = BTreeMap::from([("App.ui".into(),
        "component App { Column { width:360; height:400; Scroll { width:320; height:300; Column { width:100; height:512; } } } }".into())]);
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let json = doc.render_scene_artifact().unwrap().to_json().unwrap();
    let mut runtime = Runtime::from_sources("component Empty { Frame { width:100; height:80; } }", "FORMA-TEMPLATES-1\n").unwrap();
    let revision = runtime.layout_revision();
    runtime.load_scene_artifact_native(&json).unwrap();
    assert_ne!(runtime.layout_revision(), revision);
    runtime.scroll(0., 10000.);
    let revision = runtime.layout_revision();
    for invalid in ["{", &json.replace("forma-scene-artifact/1", "forma-scene-artifact/99")] {
        assert!(runtime.load_scene_artifact_native(invalid).is_err());
        assert_eq!(runtime.layout_revision(), revision);
        assert_eq!(runtime.scroll_offset(), vec![0., 212.]);
        assert_eq!(runtime.viewport(), vec![20., 50., 320., 300.]);
    }
}


#[test]
fn scene_mapping_count_key_and_order_must_match_the_rendered_scene() {
    let files = BTreeMap::from([("App.ui".into(),
        "component App { Column { width:240; height:160; TextInput { key:'a'; width:100; height:32; } TextInput { key:'b'; width:100; height:32; } } }".into())]);
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let artifact = doc.render_scene_artifact().unwrap();
    let mut runtime = Runtime::from_scene_artifact(&artifact).unwrap();
    assert_eq!(runtime.control_count(), 2);
    let revision = runtime.layout_revision();
    for corruption in [0, 1, 2, 3] {
        let mut bad = artifact.clone();
        match corruption {
            0 => { bad.controls.pop(); },
            1 => bad.controls.swap(0, 1),
            2 => bad.controls[0].key = "replacement".into(),
            _ => bad.controls.push(bad.controls[0].clone()),
        }
        assert!(runtime.load_scene_artifact_native(&bad.to_json().unwrap()).is_err());
        assert_eq!(runtime.layout_revision(), revision);
        assert_eq!(runtime.control_count(), 2);
        assert_eq!(runtime.control_key(0), artifact.controls[0].key);
        assert_eq!(runtime.control_key(1), artifact.controls[1].key);
    }
}

#[test]
fn oversized_empty_scroll_layout_is_reachable_from_origin_on_both_axes() {
    for kind in ["Row", "Column", "Stack", "Frame", "Grid"] {
        let source = format!("component App {{ Column {{ width:360; height:400; Scroll {{ width:320; height:300; {kind} {{ width:512; height:600; }} }} }} }}");
        let files = BTreeMap::from([("App.ui".into(), source)]);
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let artifact = doc.render_scene_artifact().unwrap();
        let mut runtime = Runtime::from_scene_artifact(&artifact).unwrap();
        assert_eq!(runtime.scroll_offset(), vec![0., 0.]);
        runtime.scroll(10000., 10000.);
        assert_eq!(runtime.scroll_offset(), vec![192., 300.], "{kind}");
        runtime.scroll(-10000., -10000.);
        assert_eq!(runtime.scroll_offset(), vec![0., 0.]);
    }
}


#[test]
fn oversized_scroll_control_preserves_reachable_leading_edge() {
    let files = BTreeMap::from([("App.ui".into(),
        "component App { Column { width:360; height:400; Scroll { width:320; height:300; TextInput { key:'wide'; width:512; height:600; } } } }".into())]);
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let mut runtime = Runtime::from_scene_artifact(&doc.render_scene_artifact().unwrap()).unwrap();
    assert_eq!(runtime.control_bounds(0), vec![20., 50., 512., 600.]);
    runtime.scroll(10000., 10000.);
    assert_eq!(runtime.scroll_offset(), vec![192., 300.]);
    runtime.scroll(-10000., -10000.);
    assert_eq!(runtime.control_bounds(0), vec![20., 50., 512., 600.]);
}
