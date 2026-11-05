//! Allocation budgets for warmed runtime paths. Counts are thread-local so the
//! test harness and concurrent tests do not affect these measurements.
use forma::{display_list::RenderScene, markup, Runtime};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAllocator;
thread_local! { static REQUESTED_BYTES: Cell<Option<usize>> = const { Cell::new(None) }; }
thread_local! { static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) }; }
fn record(bytes: usize) {
    let _=REQUESTED_BYTES.try_with(|count| {if let Some(value)=count.get(){count.set(Some(value+bytes));}});
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() { count.set(Some(value + 1)); }
    });
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        System.realloc(ptr, layout, size)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { System.dealloc(ptr, layout); }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn measured<T>(f: impl FnOnce() -> T) -> (T, usize) {
    ALLOCATIONS.with(|count| count.set(Some(0)));
    let result = f();
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    (result, allocations)
}
fn source(count: usize) -> String {
    let controls: String = (0..count).map(|i| format!("Button {{ key:'c{i}'; width:100; height:30; }}")).collect();
    format!("component Demo {{ Frame {{ width:200; height:200; {controls} }} }}")
}
const LABELED: &str = "component Button { Rectangle { Text { text: props.text; color:#102030; fontSize:12; } PointerArea { clicked -> events.clicked(); } } }";
const VISUAL: &str = "component Button { Rectangle { background:#102030; PointerArea { clicked -> events.clicked(); } } }";
const EDITOR: &str = "component Button { Rectangle { ContentInput { width:100; height:30; value:'Привет'; } PointerArea { clicked -> events.clicked(); } } }";
const RANGE: &str = "component Button { Rectangle { RangeInput { value:0.5; Minimum { ContentShape { points:'0 0 10 0 10 10'; color:#ffffff; } } Maximum { ContentShape { points:'0 0 80 0 80 10'; color:#ffffff; } } } PointerArea { clicked -> events.clicked(); } } }";

#[test]
fn retained_large_form_can_repair_a_pending_visual_error() {
    use forma::form_document::{Properties, Value};
    let files = std::collections::BTreeMap::from([("App.ui".into(),
        "component App { state query: String = 'ok'; state items: List = []; Column { width:260; height:8000; TextInput { width:200; height:32; value <-> state.query; } for item in state.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } Text { text:state.query == 'bad' ? [1] : 'ok'; } } }".into())]);
    let items = (0..96).map(|i| Value::Object(Properties::from([
        ("id".into(), Value::Text(format!("id{i}"))),
        ("title".into(), Value::Text(format!("title{i}"))),
    ]))).collect();
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Properties::from([("items".into(), Value::List(items))])).unwrap();
    runtime.set_binding_value(0, "bad", true).unwrap();
    assert!(runtime.sync_ui_bindings_native().is_err());
    runtime.set_binding_value(0, "ok", true).unwrap();
    let (rebuilt, allocations) = measured(|| runtime.sync_ui_bindings_native().unwrap());
    assert!(rebuilt);
    eprintln!("96-editor pending visual repair: {allocations} allocations");
    assert!(allocations < 450_000, "pending visual repair allocation regression: {allocations}");
    assert_eq!(runtime.text_value(0), "ok");
    assert_eq!(runtime.control_count(), 98);
    assert!(!runtime.sync_ui_bindings_native().unwrap());
}

#[test]
fn idle_scalar_binding_does_not_copy_unrelated_model_payload() {
    use forma::form_document::{Properties, Value};
    let files = std::collections::BTreeMap::from([("App.ui".into(),
        "component App { state query: String = 'seed'; Column { width:260; height:100; TextInput { width:200; height:32; value <-> state.query; } } }".into())]);
    let payload = Value::List((0..1024).map(|i| Value::Object(Properties::from([
        ("data".into(), Value::Text(format!("unrelated-{i}"))),
    ]))).collect());
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Properties::from([("payload".into(), payload)])).unwrap();
    assert!(!runtime.sync_ui_bindings_native().unwrap());
    let (rebuilt, allocations) = measured(|| runtime.sync_ui_bindings_native().unwrap());
    assert!(!rebuilt);
    eprintln!("idle scalar with 1024 unrelated records: {allocations} allocations");
    assert!(allocations < 256, "idle binding copied unrelated state: {allocations}");
    runtime.set_binding_value(0, "edited", true).unwrap();
    assert!(runtime.sync_ui_bindings_native().unwrap());
    let state = runtime.ui_state_properties();
    assert_eq!(state["query"], Value::Text("edited".into()));
    let Value::List(payload) = &state["payload"] else { panic!("payload") };
    assert_eq!(payload.len(), 1024);
}

#[test]
fn retained_idle_bindings_reuse_the_displayed_scene() {
    use forma::form_document::{Properties, Value};
    let files = std::collections::BTreeMap::from([("App.ui".into(),
        "component App { state items: List = []; Column { width:260; height:4096; for item in state.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".into())]);
    let items = (0..96).map(|i| Value::Object(Properties::from([
        ("id".into(), Value::Text(format!("id{i}"))),
        ("title".into(), Value::Text(format!("title{i}"))),
    ]))).collect();
    let state = Properties::from([("items".into(), Value::List(items))]);
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &state).unwrap();
    assert!(!runtime.sync_ui_bindings_native().unwrap());
    let (rebuilt, allocations) = measured(|| runtime.sync_ui_bindings_native().unwrap());
    assert!(!rebuilt);
    eprintln!("retained idle 96 editors: {allocations} allocations");
    assert!(allocations < 2_000, "idle bindings rescanned key collections or rebuilt a scene: {allocations}");
    runtime.set_svet_document_scheduling(true);
    assert!(!runtime.sync_ui_bindings_native().unwrap());
    let (rebuilt, svet_allocations) = measured(|| runtime.sync_ui_bindings_native().unwrap());
    assert!(!rebuilt);
    assert_eq!(svet_allocations, allocations, "idle scheduler copied model state or retracked the graph");
    runtime.set_binding_value(0, "edited", true).unwrap();
    assert!(runtime.sync_ui_bindings_native().unwrap());
    assert_eq!(runtime.text_value(0), "edited");
    assert!(!runtime.sync_ui_bindings_native().unwrap());
}

#[test]
fn warmed_pointer_tick_and_paint_buffers_do_not_allocate() {
    let mut runtime = Runtime::from_sources(&source(32), VISUAL).unwrap();
    let mut params = Vec::new();
    let mut paints = Vec::new();
    runtime.gpu_params_into(200, 200, 1., false, &mut params);
    runtime.gpu_paints_into(&mut paints);
    runtime.pointer(10., 10., 0);
    let (_, allocations) = measured(|| {
        for i in 0..64 {
            runtime.pointer(10., (i % 6) as f32 * 30. + 10., 0);
            runtime.tick(16.);
            runtime.gpu_params_into(200, 200, 1., false, &mut params);
            runtime.gpu_paints_into(&mut paints);
        }
    });
    assert_eq!(allocations, 0);
    assert_eq!(paints, runtime.gpu_paints());
    assert_eq!(params, runtime.gpu_params(200, 200, 1., false));
}

#[test]
fn idle_editors_and_ranges_do_not_recompose_each_tick() {
    for component in [EDITOR, RANGE] {
        let mut runtime = Runtime::from_sources(&source(16), component).unwrap();
        let revision = runtime.geometry_revision();
        let (_, allocations) = measured(|| {
            for _ in 0..64 { runtime.tick(16.); }
        });
        assert_eq!(allocations, 0);
        assert_eq!(runtime.geometry_revision(), revision);
    }
}

#[test]
fn scene_construction_allocations_scale_with_control_count() {
    let small_source = source(32);
    let large_source = source(128);
    let (small, small_allocations) = measured(|| Runtime::from_sources(&small_source, VISUAL).unwrap());
    let (large, large_allocations) = measured(|| Runtime::from_sources(&large_source, VISUAL).unwrap());
    assert_eq!(small.control_count(), 32);
    assert_eq!(large.control_count(), 128);
    assert!(large_allocations < small_allocations * 5,
        "4× more controls should remain linear: {small_allocations} → {large_allocations} allocations");
    assert!(large_allocations <= 128 * 10,
        "scene construction exceeded 10 allocations per control: {large_allocations} for 128 controls");
}

#[test]
fn document_markup_parse_pays_only_for_owned_property_values() {
    // Identifiers and colors borrow from the source, so a control costs its own
    // property names and string values rather than one allocation per token.
    let (small_scene, small_allocations) = measured(|| markup::parse(&source(32)).unwrap());
    let (large_scene, large_allocations) = measured(|| markup::parse(&source(128)).unwrap());
    assert_eq!((small_scene.buttons.len(), large_scene.buttons.len()), (32, 128));
    assert!(large_allocations < small_allocations * 5,
        "4× more controls should remain linear: {small_allocations} → {large_allocations} allocations");
    assert!(large_allocations <= 128 * 5,
        "document parse exceeded 5 allocations per control: {large_allocations} for 128 controls");
}

/// A scene whose labels are long enough that one control's edge list runs to hundreds of points,
/// so a buffer grown from empty is visible in the counts. `vary` makes consecutive controls
/// differ, which is the case a sibling capacity hint gets wrong.
fn labeled_source(count: usize, vary: bool) -> String {
    let short = "a".repeat(60);
    let long = format!("{}{}", "б".repeat(12), "в".repeat(60));
    let controls: String = (0..count)
        .map(|i| format!("Button {{ key:'c{i}'; text:'{}'; width:100; height:30; }}",
            if vary && i % 2 == 1 { &long } else { &short }))
        .collect();
    format!("component Demo {{ Frame {{ width:200; height:200; {controls} }} }}")
}

/// One keystroke's worth of GPU payloads from a brand new model: each control builds its own
/// display list, the scene merges them, and the tiler runs over the merge.
fn transport(controls: &str, component: &str) -> usize {
    let runtime = Runtime::from_sources(controls, component).unwrap();
    runtime.gpu_commands(1., false).len()
        + runtime.gpu_edges(1., false).len()
        + runtime.gpu_tiles(200, 200, 1., false).len()
}

#[test]
fn transport_sizes_control_buffers_instead_of_growing_them() {
    // Before the lists were sized from their siblings, every one of them doubled from empty, and
    // the cost scaled with the length of the control's geometry rather than with its count. The
    // plain scene already shows it (1 926 → 1 534 attempts); the labeled one, whose edge lists run
    // to hundreds of points, shows it clearly: 5 634 → 3 064 for the same 128 controls.
    let (_, plain_large) = measured(|| transport(&source(128), VISUAL));
    let (_, text_small) = measured(|| transport(&labeled_source(32, false), LABELED));
    let (_, text_large) = measured(|| transport(&labeled_source(128, false), LABELED));
    let (_, text_mixed) = measured(|| transport(&labeled_source(128, true), LABELED));
    assert!(text_large < text_small * 5,
        "4× more controls should remain linear: {text_small} → {text_large} allocations");
    assert!(text_large <= 128 * 28,
        "labeled transport exceeded 28 allocations per control: {text_large} for 128 controls");
    assert!(text_mixed <= 128 * 28,
        "mixed-size transport exceeded 28 allocations per control: {text_mixed} for 128 controls");
    assert!(plain_large <= 128 * 16,
        "plain transport exceeded 16 allocations per control: {plain_large} for 128 controls");
}

#[test]
fn warmed_lexical_idle_skips_replay_but_next_edit_still_commits() {
    use forma::form_document::{Properties, Value};
    let files = std::collections::BTreeMap::from([("App.ui".into(),
        "component App { state items: List = []; Column { width:260; height:4096; let visible = state.items; for item in visible key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".into())]);
    let items: Vec<Value> = (0..96).map(|i| Value::Object(Properties::from([
        ("id".into(), Value::Text(format!("id{i}"))),
        ("title".into(), Value::Text(format!("title{i}"))),
    ]))).collect();
    for svet in [false, true] {
        let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Properties::from([("items".into(), Value::List(items.clone()))])).unwrap();
        runtime.set_svet_document_scheduling(svet);
        assert!(!runtime.sync_ui_bindings_native().unwrap());
        let (_, allocations) = measured(|| {
            for _ in 0..8 { assert!(!runtime.sync_ui_bindings_native().unwrap()); }
        });
        assert_eq!(allocations, 0, "warmed lexical idle replayed owned model snapshots");
        runtime.set_binding_value(0, "edited", true).unwrap();
        assert!(runtime.sync_ui_bindings_native().unwrap());
        assert_eq!(runtime.text_value(0), "edited");
        let state = runtime.ui_state_properties();
        let Value::List(values) = &state["items"] else { panic!("items") };
        let Value::Object(first) = &values[0] else { panic!("first") };
        assert_eq!(first["title"], Value::Text("edited".into()));
        let (_, cold_allocations) = measured(|| assert!(!runtime.sync_ui_bindings_native().unwrap()));
        assert!(cold_allocations < 10_000, "cold lexical binding replay cloned the collection per control: {cold_allocations}");
        let (_, allocations) = measured(|| assert!(!runtime.sync_ui_bindings_native().unwrap()));
        assert_eq!(allocations, 0);
    }
}

#[test]
fn nested_layout_render_does_not_snapshot_root_payload_per_container() {
    use forma::form_document::{Document, Properties, Value};
    let mut body="TextInput { key:'editor'; width:100; height:32; value <-> state.query; }".to_owned();
    for _ in 0..24 { body=format!("Column {{ width:200; height:100; {body} }}"); }
    let files=std::collections::BTreeMap::from([("App.ui".into(),format!("component App {{ state query: String = 'seed'; {body} }}"))]);
    let payload=Value::List((0..256).map(|i|Value::Object(Properties::from([("data".into(),Value::Text(format!("payload-{i}")))]))).collect());
    let doc=Document::from_ui_files(&files,"App.ui",&Properties::from([("payload".into(),payload)])).unwrap();
    let (artifact, allocations)=measured(||doc.render_scene_artifact().unwrap());
    eprintln!("24 nested layouts with 256 unrelated records: {allocations} allocations");
    assert_eq!(artifact.controls.len(),1);
    assert!(allocations<10_000,"layout-only nodes copied root snapshots: {allocations}");
    let Value::List(payload)=&artifact.controls[0].state["payload"] else {panic!("public control state preserved")};
    assert_eq!(payload.len(),256);
}

#[test]
fn component_contract_check_borrows_large_resolved_properties() {
    use forma::form_document::{Document,Properties,Value};
    let files=std::collections::BTreeMap::from([
        ("App.ui".into(),"component App { state payload: List = []; Column { width:200; height:100; Card { payload:state.payload; } } }".into()),
        ("Card.ui".into(),"component Card { prop payload: List = []; width:100; height:32; Rectangle { background:#123456; PointerArea { clicked -> events.clicked(); } } }".into()),
    ]);
    let payload=Value::List((0..256).map(|i|Value::Object(Properties::from([("data".into(),Value::Text(format!("payload-{i}")))]))).collect());
    let doc=Document::from_ui_files(&files,"App.ui",&Properties::from([("payload".into(),payload)])).unwrap();
    let (artifact,allocations)=measured(||doc.render_scene_artifact().unwrap());
    assert_eq!(artifact.controls.len(),1);
    eprintln!("large resolved component property: {allocations} allocations");
    assert!(allocations<4_700,"contract validation copied resolved payload: {allocations}");
}

#[test]
fn control_local_state_moves_into_snapshot_after_expansion() {
    use forma::form_document::{Document,Properties,Value};
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = 'seed'; Column { width:220; height:100; TextInput { key:'editor'; width:180; height:32; value <-> state.query; } } }".into())]);
    let mut doc=Document::from_ui_files(&files,"App.ui",&Default::default()).unwrap();
    let id=doc.render_scene_artifact().unwrap().controls[0].node;
    doc.states.resize_with(id+1,Properties::new);
    doc.states[id].insert("local_payload".into(),Value::List((0..256).map(|i|Value::Object(Properties::from([("data".into(),Value::Text(format!("local-{i}")))]))).collect()));
    let before=doc.states.clone();
    let (artifact,allocations)=measured(||doc.render_scene_artifact().unwrap());
    let Value::List(payload)=&artifact.controls[0].state["local_payload"] else {panic!("local state snapshot")};assert_eq!(payload.len(),256);
    assert_eq!(artifact.controls[0].state["query"],Value::Text("seed".into()));
    eprintln!("owned control-local state snapshot: {allocations} allocations");
    assert!(allocations<1_400,"local scope copied into snapshot: {allocations}");
    assert_eq!(doc.states,before,"render must not consume Document state");
    let repeated=doc.render_scene_artifact().unwrap();
    assert_eq!(artifact.to_json().unwrap(),repeated.to_json().unwrap(),"repeated render preserves snapshot bytes");
}


#[test]
fn focused_leaf_scratch_does_not_allocate_a_full_canvas_per_caret_frame() {
    use forma::form_document::Properties;
    let files=std::collections::BTreeMap::from([("App.ui".into(),include_str!("fixtures/svet-native-ime/App.ui").into())]);
    let mut runtime=Runtime::from_ui_files(&files,"App.ui",&Properties::new()).unwrap();
    runtime.set_binding_value(1,"你好 мир🙂",true).unwrap();runtime.sync_ui_bindings_native().unwrap();
    assert!(runtime.focus_control(1));
    let mut out=vec![0;1600*1200];
    for _ in 0..10 {runtime.tick(1000./60.);runtime.paint_native(&mut out,1600,1200,2.).unwrap();}
    let before=runtime.raster_stats();
    REQUESTED_BYTES.with(|count|count.set(Some(0)));
    runtime.tick(1000./60.);runtime.sync_ui_bindings_native().unwrap();
    runtime.paint_native(&mut out,1600,1200,2.).unwrap();
    let bytes=REQUESTED_BYTES.with(|count|count.replace(None).unwrap());
    let after=runtime.raster_stats();assert_eq!(after[0],before[0]+1,"fixture must rebuild the focused leaf");
    eprintln!("focused leaf on1600x1200 canvas: {bytes} requested bytes");
    assert!(bytes<2_000_000,"caret frame rebuilt full-canvas scratch: {bytes}");
}

#[test]
fn empty_dense_content_does_not_allocate_an_rgba_scratch_layer() {
    let model=forma::Button::from_sources(
        "component App { Frame { width:800; height:600; Button { width:160; height:80; } } }",
        "component Button { Rectangle { background:#abcdef; } }",
    ).unwrap();
    REQUESTED_BYTES.with(|count|count.set(Some(0)));
    let pixels=model.content_pixels(800,600,1.);
    let bytes=REQUESTED_BYTES.with(|count|count.replace(None).unwrap());
    assert_eq!(pixels.len(),800*600*4);
    assert!(pixels.chunks_exact(4).any(|p|p==[0xab,0xcd,0xef,255]));
    eprintln!("empty dense content: {bytes} requested bytes");
    assert!(bytes<5_000_000,"empty content allocated full RGBA scratch: {bytes}");
}

#[test]
fn dense_nested_content_scratch_is_bounded_by_content_support() {
    let model=forma::Button::from_sources(
        "component App { Frame { width:800; height:600; Button { x:30; y:40; width:160; height:80; } } }",
        "component Button { Rectangle { background:#abcdef; ContentClip { x:3; y:4; width:100; height:60; radius:3; } ContentClip { x:8; y:9; width:40; height:30; radius:2; } ContentShape { points:'10 10 25 10 25 25 10 25'; color:#ff000080; } ContentClipEnd {} ContentClipEnd {} } }",
    ).unwrap();
    REQUESTED_BYTES.with(|count|count.set(Some(0)));
    let pixels=model.content_pixels(800,600,1.);
    let bytes=REQUESTED_BYTES.with(|count|count.replace(None).unwrap());
    assert_eq!(pixels.len(),800*600*4);
    assert!(pixels.chunks_exact(4).any(|p|p==[0xab,0xcd,0xef,255]));
    assert!(pixels.chunks_exact(4).any(|p|p[0]>0xab&&p[3]==255));
    eprintln!("dense nested content: {bytes} requested bytes");
    assert!(bytes<4_000_000,"nested content allocated canvas RGBA layers: {bytes}");
}

#[test]
fn for_scope_overlays_do_not_copy_unrelated_root_lists_per_control() {
    use forma::form_document::{Document,Properties,Value};
    let files:std::collections::BTreeMap<String,String>=std::collections::BTreeMap::from([("App.ui".into(),
        "component App { state rows: List = []; state payload: List = []; Column { for row in state.rows key row.id { TextInput { width:180; height:32; value <-> row.title; } } } }".into())]);
    let rows=Value::List((0..96).map(|i|Value::Object(Properties::from([
        ("id".into(),Value::Text(format!("row-{i}"))),("title".into(),Value::Text(format!("title-{i}"))),
    ]))).collect());
    let payload=Value::List((0..1024).map(|i|Value::Text(format!("unrelated-{i}-{}","payload".repeat(8)))).collect());
    let mut worst=0;
    for mode in ["direct","lexical","shadow","captured"] {
        let mut files=files.clone();
        if mode=="lexical" {
            let source=files.get_mut("App.ui").unwrap();
            *source=source.replace("for row in state.rows", "let cargo = state.payload; let visible = state.rows; for row in visible");
        }
        if mode=="shadow" {
            let source=files.get_mut("App.ui").unwrap();
            *source=source.replace("{ TextInput", "{ let payload = [1,2]; TextInput");
        }
        let mut doc=Document::from_ui_files(&files,"App.ui",&Properties::from([("rows".into(),rows.clone()),("payload".into(),payload.clone())])).unwrap();
        let mut local_node=None;
        if mode=="captured" {
            let node=doc.render_scene_artifact().unwrap().controls[0].node-1;
            doc.states.resize_with(node+1,Properties::new);
            doc.states[node].insert("local_payload".into(),payload.clone());
            local_node=Some(node);
        }
        let before=doc.state_json().unwrap();
        REQUESTED_BYTES.with(|count|count.set(Some(0)));
        let mut artifact=doc.render_scene_artifact().unwrap();
        let bytes=REQUESTED_BYTES.with(|count|count.replace(None).unwrap());
        eprintln!("96 For scopes with1024 unrelated text records, mode={mode}: {bytes} requested bytes");
        assert_eq!(artifact.controls.len(),96);
        assert_eq!(doc.state_json().unwrap(),before);
        for control in &artifact.controls {
            let Value::List(payload)=&control.state["payload"] else {panic!("public root payload")};
            assert_eq!(payload.len(),if mode=="shadow" {2} else {1024});
            if mode=="lexical" {assert_eq!(control.state["cargo"],control.state["payload"]);}
        }
        if mode=="shadow" {
            let control=artifact.controls.remove(0);
            REQUESTED_BYTES.with(|count|count.set(Some(0)));
            let owned:std::collections::BTreeMap<_,_>=control.state.into_iter().collect();
            let materialized=REQUESTED_BYTES.with(|count|count.replace(None).unwrap());
            eprintln!("owned masked scope: {materialized} requested bytes");
            assert_eq!(owned["payload"],Value::List(vec![Value::Number(1.),Value::Number(2.)]));
            assert!(materialized<20_000,"owning a masked scope copied hidden root payload: {materialized}");
        }
        if mode=="captured" {
            fn locals(state:&Properties)->&Properties {
                let Value::List(frames)=&state["__binding_frames"] else {panic!()};
                let Value::Object(frame)=&frames[0] else {panic!()};
                let Value::Object(locals)=&frame["__locals"] else {panic!()};locals
            }
            let Value::List(first)=&locals(&artifact.controls[0].state)["local_payload"] else {panic!()};
            let pointer=first.as_ptr();
            for control in &artifact.controls {
                let Value::List(values)=&locals(&control.state)["local_payload"] else {panic!()};
                assert_eq!(values.len(),1024);assert_eq!(values.as_ptr(),pointer,"captured locals must share immutable storage");
            }
            let Value::List(frames)=artifact.controls[0].state.get_mut("__binding_frames").unwrap() else {panic!()};
            let Value::Object(frame)=&mut frames[0] else {panic!()};
            let Value::Object(scope)=frame.get_mut("__locals").unwrap() else {panic!()};
            let Value::List(values)=scope.get_mut("local_payload").unwrap() else {panic!()};values.push(Value::Text("snapshot-only".into()));
            let Value::List(other)=&locals(&artifact.controls[1].state)["local_payload"] else {panic!()};assert_eq!(other.len(),1024);
            let Value::List(original)=&doc.states[local_node.unwrap()]["local_payload"] else {panic!()};assert_eq!(original.len(),1024);
            assert_eq!(doc.state_json().unwrap(),before);
        }
        worst=worst.max(bytes);
    }
    assert!(worst<5_000_000,"For overlays copied root payload per control: {worst}");
}

#[test]
fn caller_owned_rgba_reuses_output_and_survives_cache_release() {
    for count in [1, 8] {
        let runtime = Runtime::from_sources(&source(count), VISUAL).unwrap();
        for scale in [1., 1.25, 2.] {
            for background in [false, true] {
                let expected = if background { runtime.pixels(240, 220, scale) }
                    else { runtime.content_pixels(240, 220, scale) };
                let mut out = vec![0; expected.len()];
                runtime.paint_rgba(&mut out, 240, 220, scale, background).unwrap();
                assert_eq!(out, expected);
                let (_, allocations) = measured(|| {
                    for _ in 0..8 {
                        out.fill(99);
                        runtime.paint_rgba(&mut out, 240, 220, scale, background).unwrap();
                    }
                });
                assert_eq!(allocations, 0, "warmed RGBA output allocated");
                let (_, owned_allocations) = measured(|| {
                    std::hint::black_box(if background { runtime.pixels(240, 220, scale) }
                        else { runtime.content_pixels(240, 220, scale) });
                });
                assert_eq!(owned_allocations, 1, "owned output baseline must allocate its Vec");
                eprintln!("RGBA count={count} scale={scale} background={background}: owned={owned_allocations}, reused={allocations}; output bytes={}",out.len());
                assert_eq!(out, expected);
                runtime.release_cpu_cache();
                runtime.paint_rgba(&mut out, 240, 220, scale, background).unwrap();
                assert_eq!(out, expected);
                for dpi in [0., -1., f32::NAN, f32::INFINITY] {
                    out.fill(99);
                    assert!(runtime.paint_rgba(&mut out, 240, 220, dpi, background).is_err());
                    assert!(out.iter().all(|x| *x == 99));
                }
                assert!(runtime.paint_rgba(&mut out[..4], 240, 220, scale, background).is_err());
            }
        }
    }
}

#[test]
fn accessibility_snapshot_serialization_budget_and_exact_value_oracle(){
 for count in [1,32,256]{
  let mut runtime=Runtime::from_sources(&source(count),EDITOR).unwrap();runtime.focus_control(0);runtime.text_key("ArrowLeft",true,false);runtime.accessibility_tree_json();
  let (json,allocations)=measured(||runtime.accessibility_tree_json());
  let tree=runtime.accessibility_tree();let nodes:Vec<_>=tree.nodes.iter().map(|node|serde_json::json!({"id":node.id,"parent":node.parent,"children":node.children,"role":node.role.as_str(),"key":node.key,"label":node.label,"value":node.value,"bounds":node.bounds,"focused":node.focused,"disabled":node.disabled,"readOnly":node.read_only,"multiline":node.multiline,"selectionUtf16":node.selection_utf16,"modalBlocked":node.modal_blocked,"actions":node.actions})).collect();
  let expected=serde_json::json!({"rootId":tree.root_id,"focusedId":tree.focused_id,"activeModalId":tree.active_modal_id,"nodes":nodes});
  assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap(),expected);
  eprintln!("A11Y_SERIALIZATION controls={count} allocations={allocations} bytes={}",json.len());
  assert!(allocations<=7*count+32,"intermediate semantic JSON allocation regression: {allocations}");
 }
}

#[cfg(feature="native")]
#[test]
fn unchanged_accesskit_publication_skips_translation_allocations() {
 for count in [1,32,256] {
  let mut runtime=Runtime::from_sources(&source(count),EDITOR).unwrap();runtime.focus_control(0);
  let mut baseline=forma::native_accessibility::PublishedTree::default();baseline.update(&runtime).unwrap();
  let (_,full)=measured(||baseline.update(&runtime).unwrap());
  let mut cached=forma::native_accessibility::PublishedTree::default();assert!(cached.update_if_changed(&runtime,2.).unwrap().is_some());
  let (update,unchanged)=measured(||cached.update_if_changed(&runtime,2.).unwrap());assert!(update.is_none());
  eprintln!("ACCESSKIT_PUBLICATION controls={count} full={full} unchanged={unchanged}");
  assert!(unchanged<full,"unchanged tree must avoid translation allocations");
  runtime.tick(100.);assert!(cached.update_if_changed(&runtime,2.).unwrap().is_none());
  runtime.text_key("ArrowLeft",true,false);assert!(cached.update_if_changed(&runtime,2.).unwrap().is_some());
  assert!(cached.update_if_changed(&runtime,1.).unwrap().is_some(),"DPR change must publish");
  cached.invalidate();assert!(cached.update_if_changed(&runtime,1.).unwrap().is_some(),"reactivation requires full tree");
 }
}
