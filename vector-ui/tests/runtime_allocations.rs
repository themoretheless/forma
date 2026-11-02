//! Allocation budgets for warmed runtime paths. Counts are thread-local so the
//! test harness and concurrent tests do not affect these measurements.
use forma::{display_list::RenderScene, markup, Runtime};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAllocator;
thread_local! { static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) }; }
fn record() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() { count.set(Some(value + 1)); }
    });
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record();
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
