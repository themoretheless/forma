//! Public retained-document binding workload; prints raw timings as JSON.
use forma::{
    form_document::{Document, Properties, Value},
    Runtime,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::{Cell, RefCell},
    sync::atomic::{AtomicBool, Ordering},
};
use std::{collections::BTreeMap, time::Instant};
struct Counting;
thread_local! { static ALLOCS: Cell<Option<(usize, u64)>> = const { Cell::new(None) }; }
thread_local! { static LIVE_DELTA: Cell<Option<i128>> = const { Cell::new(None) }; }
fn live_delta(bytes: i128) {
    let _ = LIVE_DELTA.try_with(|cell| {
        if let Some(value) = cell.get() { cell.set(Some(value + bytes)); }
    });
}

static PROFILE_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
thread_local! {
    static PROFILE_PHASE: RefCell<String> = const { RefCell::new(String::new()) };
    static PROFILE_STACKS: RefCell<Vec<(String, usize, u64, String)>> = const { RefCell::new(Vec::new()) };
}
fn profile_phase(svet: bool, count: usize, phase: &str, sample: usize) {
    if PROFILE_ALLOCATIONS.load(Ordering::Relaxed) {
        PROFILE_PHASE.with(|value| *value.borrow_mut() = format!("svet={svet}/controls={count}/{phase}/{sample}"));
    }
}

fn record(bytes: usize) {
    let _ = ALLOCS.try_with(|cell| {
        if let Some((n, b)) = cell.get() {
            let next = (n + 1, b + bytes as u64);
            cell.set(Some(next));
            if PROFILE_ALLOCATIONS.load(Ordering::Relaxed) && n % 65_536 == 0 {
                // Profiling must not recursively sample or count its own allocations.
                cell.set(None);
                PROFILE_STACKS.with(|samples| {
                    let mut samples = samples.borrow_mut();
                    if samples.len() < 1024 {
                        let phase = PROFILE_PHASE.with(|value| value.borrow().clone());
                        let stack = format!("{}", std::backtrace::Backtrace::force_capture());
                        samples.push((phase, next.0, next.1, stack));
                    }
                });
                cell.set(Some(next));
            }
        }
    });
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        record(l.size());
        live_delta(l.size() as i128);
        System.alloc(l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        record(l.size());
        live_delta(l.size() as i128);
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        record(n);
        live_delta(n as i128 - l.size() as i128);
        System.realloc(p, l, n)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        live_delta(-(l.size() as i128));
        System.dealloc(p, l)
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn measured<T>(f: impl FnOnce() -> T) -> (T, f64, usize, u64) {
    ALLOCS.with(|cell| cell.set(Some((0, 0))));
    let start = Instant::now();
    let result = f();
    let ms = start.elapsed().as_secs_f64() * 1000.;
    let (allocations, bytes) = ALLOCS.with(|cell| cell.replace(None).unwrap());
    (result, ms, allocations, bytes)
}
fn main() {
    if std::env::args().any(|arg| arg == "--artifact-retention") {
        artifact_retention();
        return;
    }
    if std::env::args().any(|arg| arg == "--retained") {
        let trace_output = std::env::var("FORMA_ALLOC_TRACE_OUTPUT").ok();
        PROFILE_ALLOCATIONS.store(trace_output.is_some(), Ordering::Relaxed);
        retained();
        if let Some(path) = trace_output {
            PROFILE_STACKS.with(|samples| {
                let samples = samples.borrow();
                let report = serde_json::json!({"sample_every_allocations":65536,"max_samples":1024,
                    "possibly_truncated":samples.len()==1024,"sample_fields":["phase","allocation_index","requested_bytes_so_far","backtrace"],
                    "scope":"Diagnostic stack sampling; timings are instrumented; not live/peak memory", "samples":*samples});
                std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
            });
        }
        return;
    }
    let files = BTreeMap::from([("App.ui".to_owned(), "component App { state items: List = []; Column { width:260; height:4096; for item in state.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".to_owned())]);
    let mut results = Vec::new();
    for count in [8usize, 32, 96] {
        let items = (0..count)
            .map(|i| {
                Value::Object(BTreeMap::from([
                    ("id".to_owned(), Value::Text(format!("id{i}"))),
                    ("title".to_owned(), Value::Text(format!("title{i}"))),
                ]))
            })
            .collect();
        let state = Properties::from([("items".to_owned(), Value::List(items))]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (source, templates, _) = doc.render_scene().unwrap();
        let mut runtime = Runtime::from_sources(&source, &templates).unwrap();
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
        let mut noop = Vec::new();
        let mut edit = Vec::new();
        let mut noop_alloc = Vec::new();
        let mut edit_alloc = Vec::new();
        let mut render_ms = Vec::new();
        let mut render_alloc = Vec::new();
        for sample in 0..7 {
            let (_, ms, n, bytes) = measured(|| {
                let (_, _, controls) = doc.render_scene().unwrap();
                assert_eq!(controls.len(), count);
            });
            render_ms.push(ms);
            render_alloc.push((n, bytes));
            let (changed, ms, n, bytes) =
                measured(|| doc.sync_runtime_bindings(&runtime, None).unwrap());
            assert_eq!(changed, 0);
            noop.push(ms);
            noop_alloc.push((n, bytes));
            runtime
                .set_binding_value(0, &format!("edited{sample}"), true)
                .unwrap();
            let (changed, ms, n, bytes) =
                measured(|| doc.sync_runtime_bindings(&runtime, None).unwrap());
            assert_eq!(changed, 1);
            edit.push(ms);
            edit_alloc.push((n, bytes));
            let Value::List(items) = &doc.states[0]["items"] else {
                panic!("items");
            };
            let Value::Object(first) = &items[0] else {
                panic!("item");
            };
            assert_eq!(first["title"], Value::Text(format!("edited{sample}")));
        }
        results.push(serde_json::json!({"controls":count,"noop_ms":noop,"one_edit_ms":edit,"noop_allocation_count_and_bytes":noop_alloc,"edit_allocation_count_and_bytes":edit_alloc,"render_scene_ms":render_ms,"render_scene_allocation_count_and_bytes":render_alloc}));
    }
    println!(
        "{}",
        serde_json::json!({"workload":"public Document::sync_runtime_bindings, 7 alternating noop/edit samples", "results":results})
    );
}

fn retained() {
    let source = "component App { state items: List = []; Column { width:260; height:16384; for item in state.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }";
    let frame = std::env::args().any(|arg| arg == "--frame");
    let source = if frame { source.replacen("Column {", "Frame {", 1) } else { source.to_owned() };
    let lexical = std::env::args().any(|arg| arg == "--lexical");
    let source = if lexical { source.replace("for item in state.items", "let visible = state.items; for item in visible") } else { source };
    let files = BTreeMap::from([("App.ui".to_owned(), source)]);
    let mut results = Vec::new();
    for svet in [false, true] {
        for count in [8usize, 32, 96, 256] {
            let items = (0..count).map(|i| Value::Object(Properties::from([
                ("id".into(), Value::Text(format!("id{i}"))),
                ("title".into(), Value::Text(format!("title{i}"))),
            ]))).collect();
            let state = Properties::from([("items".into(), Value::List(items))]);
            let mut runtime = Runtime::from_ui_files(&files, "App.ui", &state).unwrap();
            runtime.set_svet_document_scheduling(svet);
            assert_eq!(runtime.control_count(), count);
            assert!(!runtime.sync_ui_bindings_native().unwrap());
            let mut samples = Vec::new();
            for sample in 0..7 {
                profile_phase(svet, count, "idle", sample);
                let (rebuilt, idle_ms, idle_count, idle_bytes) = measured(|| runtime.sync_ui_bindings_native().unwrap());
                assert!(!rebuilt);
                let value = format!("edited{sample}");
                runtime.set_binding_value(0, &value, true).unwrap();
                profile_phase(svet, count, "edit", sample);
                let (rebuilt, edit_ms, edit_count, edit_bytes) = measured(|| runtime.sync_ui_bindings_native().unwrap());
                assert!(rebuilt);
                assert_eq!(runtime.text_value(0), value);
                let state = runtime.ui_state_properties();
                let Value::List(items) = &state["items"] else { panic!("items") };
                let Value::Object(first) = &items[0] else { panic!("item") };
                assert_eq!(first["title"], Value::Text(value));
                assert_eq!(items.len(), count);
                if svet {
                    let telemetry = runtime.svet_document_telemetry().unwrap();
                    assert_eq!(telemetry.batch_depth, 0);
                    assert_eq!(telemetry.tracking_depth, 0);
                }
                samples.push(serde_json::json!({"idle_ms":idle_ms,"idle_allocations":idle_count,"idle_requested_bytes":idle_bytes,"edit_ms":edit_ms,"edit_allocations":edit_count,"edit_requested_bytes":edit_bytes}));
            }
            results.push(serde_json::json!({"svet":svet,"controls":count,"samples":samples}));
        }
    }
    println!("{}", serde_json::json!({"workload":"retained Runtime sync, 7 alternating idle/edit samples; edit includes scene rendering and Runtime replacement, excludes painting/presentation", "layout":if frame {"Frame"} else {"Column"}, "lexical_alias":lexical, "results":results}));
}

fn artifact_retention() {
    let files = BTreeMap::from([("App.ui".to_owned(), "component App { state items: List = []; Column { width:260; height:16384; let visible = state.items; for item in visible key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".to_owned())]);
    let mut rows = Vec::new();
    for count in [8usize, 32, 96, 256] {
        let items = (0..count).map(|i| Value::Object(Properties::from([
            ("id".into(), Value::Text(format!("id{i}"))),
            ("title".into(), Value::Text(format!("title{i}"))),
        ]))).collect();
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::from([("items".into(), Value::List(items))])).unwrap();
        drop(doc.render_scene_artifact().unwrap());
        let mut samples = Vec::new();
        for _ in 0..2 {
            LIVE_DELTA.with(|cell| cell.set(Some(0)));
            let (artifact, ms, allocations, requested) = measured(|| doc.render_scene_artifact().unwrap());
            assert_eq!(artifact.controls.len(), count);
            let retained = LIVE_DELTA.with(|cell| cell.get().unwrap());
            drop(artifact);
            let after_drop = LIVE_DELTA.with(|cell| cell.replace(None).unwrap());
            assert_eq!(after_drop, 0, "warmed artifact lifecycle must return allocator delta to zero");
            samples.push(serde_json::json!({"render_ms":ms,"allocations":allocations,"requested_bytes":requested,
                "retained_allocator_bytes":retained,"allocator_delta_after_drop":after_drop}));
        }
        rows.push(serde_json::json!({"controls":count,"samples":samples}));
    }
    println!("{}", serde_json::json!({"workload":"Warmed owned SceneArtifact with retained Let collection alias",
        "scope":"Net live Rust allocator Layout bytes relative to pre-render baseline, validated by zero delta after Drop; excludes Document,allocator overhead,RSS,GPU", "rows":rows}));
}
