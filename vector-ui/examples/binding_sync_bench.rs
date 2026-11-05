//! Public retained-document binding workload; prints raw timings as JSON.
use forma::{
    form_document::{Document, Properties, Value},
    Runtime,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::{Cell, RefCell},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::{collections::BTreeMap, time::Instant};
struct Counting;
thread_local! { static ALLOCS: Cell<Option<(usize, u64)>> = const { Cell::new(None) }; }
thread_local! { static LIVE_PEAK: Cell<Option<i128>> = const { Cell::new(None) }; }
thread_local! { static LIVE_DELTA: Cell<Option<i128>> = const { Cell::new(None) }; }
fn live_delta(bytes: i128) {
    let _ = LIVE_DELTA.try_with(|cell| {
        if let Some(value) = cell.get() {
            let next = value + bytes;
            cell.set(Some(next));
            let _ = LIVE_PEAK.try_with(|peak| { if let Some(max) = peak.get() { peak.set(Some(max.max(next))); } });
        }
    });
}

static PROFILE_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static PROFILE_LARGE_MIN_BYTES: AtomicUsize = AtomicUsize::new(usize::MAX);
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
            if PROFILE_ALLOCATIONS.load(Ordering::Relaxed) && (n % 65_536 == 0 || bytes >= PROFILE_LARGE_MIN_BYTES.load(Ordering::Relaxed)) {
                // Profiling must not recursively sample or count its own allocations.
                cell.set(None);
                PROFILE_STACKS.with(|samples| {
                    let mut samples = samples.borrow_mut();
                    if samples.len() < 1024 {
                        let phase = PROFILE_PHASE.with(|value| value.borrow().clone());
                        let stack = format!("allocation request: {bytes} bytes\n{}", std::backtrace::Backtrace::force_capture());
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
    if std::env::args().any(|arg|arg=="--cpu-lifecycle") {cpu_lifecycle_probe();return;}
    if std::env::args().any(|arg|arg=="--cpu-frame") {cpu_frame_probe();return;}

    if std::env::args().any(|arg| arg == "--runtime-allocation-trace") {
        runtime_allocation_trace();
        return;
    }
    if std::env::args().any(|arg| arg == "--runtime-retention") {
        runtime_retention();
        return;
    }
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
                Value::Object(Properties::from([
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
    let payload_records=std::env::args().find_map(|arg|arg.strip_prefix("--payload-records=").map(|value|value.parse::<usize>().expect("payload record count"))).unwrap_or(0);
    let frame = std::env::args().any(|arg| arg == "--frame");
    let source = if frame { source.replacen("Column {", "Frame {", 1) } else { source.to_owned() };
    let lexical = std::env::args().any(|arg| arg == "--lexical");
    let source = if lexical { source.replace("for item in state.items", "let visible = state.items; for item in visible") } else { source };
    let nested_depth = if std::env::args().any(|arg| arg == "--nested-layout") { 12 } else { 0 };
    let source = if nested_depth > 0 {
        let containers = "Column { width:260; height:16384; ".repeat(nested_depth + 1);
        let alias = if lexical { "let visible = state.items;" } else { "" };
        let collection = if lexical { "visible" } else { "state.items" };
        format!("component App {{ state items: List = []; {containers}{alias} for item in {collection} key item.id {{ TextInput {{ width:200; height:32; value <-> item.title; }} }} {} }}", "} ".repeat(nested_depth + 1))
    } else { source };
    let source=if payload_records>0 {source.replace("state items: List = [];", "state items: List = []; state payload: List = [];")} else {source};
    let files = BTreeMap::from([("App.ui".to_owned(), source)]);
    let mut results = Vec::new();
    for svet in [false, true] {
        for count in [8usize, 32, 96, 256] {
            let items = (0..count).map(|i| Value::Object(Properties::from([
                ("id".into(), Value::Text(format!("id{i}"))),
                ("title".into(), Value::Text(format!("title{i}"))),
            ]))).collect();
            let mut state = Properties::from([("items".into(), Value::List(items))]);
            if payload_records>0 {
                state.insert("payload".into(),Value::List((0..payload_records).map(|i|Value::Text(format!("unrelated-{i}-{}","payload".repeat(8)))).collect()));
            }
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
    println!("{}", serde_json::json!({"workload":"retained Runtime sync, 7 alternating idle/edit samples; edit includes scene rendering and Runtime replacement, excludes painting/presentation", "layout":if frame {"Frame"} else {"Column"}, "lexical_alias":lexical, "nested_depth":nested_depth,"payload_records":payload_records, "results":results}));
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


fn runtime_retention() {
    let files = BTreeMap::from([("App.ui".to_owned(), "component App { state items: List = []; Column { width:260; height:16384; let visible = state.items; for item in visible key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".to_owned())]);
    let mut rows = Vec::new();
    for svet in [false, true] {
        for count in [8usize, 32, 96, 256] {
            let run = || {
                let state = Properties::from([("items".into(), Value::List((0..count).map(|i| Value::Object(Properties::from([
                    ("id".into(), Value::Text(format!("id{i}"))),
                    ("title".into(), Value::Text(format!("title{i}"))),
                ]))).collect()))]);
                let mut runtime = Runtime::from_ui_files(&files,"App.ui",&state).unwrap();
                runtime.set_svet_document_scheduling(svet);
                drop(state);
                assert_eq!(runtime.control_count(), count);
                assert!(!runtime.sync_ui_bindings_native().unwrap());
                drop(runtime.pixels(300,400,1.25));
                runtime
            };
            // Warm caches through the complete lifecycle, including longer edited labels.
            for _ in 0..2 {
                let mut warm = run();
                for edit in 0..3 {
                    warm.set_binding_value(0,&format!("retention-edit-{edit}"),true).unwrap();
                    assert!(warm.sync_ui_bindings_native().unwrap());
                    drop(warm.pixels(300,400,1.25));
                }
                drop(warm);
            }
            let mut samples = Vec::new();
            for _ in 0..2 {
                LIVE_DELTA.with(|v|v.set(Some(0)));
                LIVE_PEAK.with(|v|v.set(Some(0)));
                let (mut runtime, initial_ms, initial_allocations, initial_requested_bytes) = measured(run);
                let initial_live = LIVE_DELTA.with(|v|v.get().unwrap());
                let mut edits = Vec::new();
                for edit in 0..3 {
                    let value = format!("retention-edit-{edit}");
                    let (_, ms, allocations, requested_bytes) = measured(|| {
                        runtime.set_binding_value(0,&value,true).unwrap();
                        assert!(runtime.sync_ui_bindings_native().unwrap());
                        assert_eq!(runtime.text_value(0),value);
                        drop(runtime.pixels(300,400,1.25));
                    });
                    // Collecting diagnostics must not contaminate allocator live/peak accounting.
                    let live = LIVE_DELTA.with(|v|v.replace(None).unwrap());
                    let peak = LIVE_PEAK.with(|v|v.replace(None).unwrap());
                    edits.push(serde_json::json!({"ms":ms,"allocations":allocations,"requested_bytes":requested_bytes,"live_bytes":live,"peak_bytes":peak}));
                    LIVE_DELTA.with(|v|v.set(Some(live)));
                    LIVE_PEAK.with(|v|v.set(Some(peak)));
                }
                drop(runtime);
                let after_drop = LIVE_DELTA.with(|v|v.replace(None).unwrap());
                let peak = LIVE_PEAK.with(|v|v.replace(None).unwrap());
                assert_eq!(after_drop,0,"warmed Runtime lifecycle leaked allocator delta: svet={svet} controls={count}");
                samples.push(serde_json::json!({"initial_ms":initial_ms,"initial_allocations":initial_allocations,"initial_requested_bytes":initial_requested_bytes,"initial_live_bytes":initial_live,"lifetime_peak_bytes":peak,"allocator_delta_after_drop":after_drop,"edits":edits}));
            }
            rows.push(serde_json::json!({"svet":svet,"controls":count,"samples":samples}));
        }
    }
    println!("{}",serde_json::json!({"scope":"Warmed Runtime construction +3 binding edit/rebuild/CPU frames at1.25scale; includes created model and runtime, excludes caller source files, warmed thread caches, allocator overhead, RSS/GPU; Layout bytes measured on one thread","rows":rows}));
}


fn runtime_allocation_trace() {
    let files = BTreeMap::from([("App.ui".to_owned(), "component App { state items: List = []; Column { width:260; height:16384; let visible = state.items; for item in visible key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".to_owned())]);
    let state = Properties::from([("items".into(), Value::List((0..256).map(|i| Value::Object(Properties::from([
        ("id".into(),Value::Text(format!("id{i}"))),
        ("title".into(),Value::Text(format!("title{i}"))),
    ]))).collect()))]);
    let mut warm = Runtime::from_ui_files(&files,"App.ui",&state).unwrap();
    warm.sync_ui_bindings_native().unwrap();
    drop(warm.pixels(300,400,1.25)); drop(warm);
    PROFILE_LARGE_MIN_BYTES.store(1_048_576,Ordering::Relaxed);
    PROFILE_ALLOCATIONS.store(true,Ordering::Relaxed);
    profile_phase(false,256,"initial-load",0);
    let (mut runtime,_,load_allocations,load_bytes)=measured(||Runtime::from_ui_files(&files,"App.ui",&state).unwrap());
    profile_phase(false,256,"first-sync",0);
    let (_,_,sync_allocations,sync_bytes)=measured(||runtime.sync_ui_bindings_native().unwrap());
    profile_phase(false,256,"cpu-frame",0);
    let (_,_,frame_allocations,frame_bytes)=measured(||drop(runtime.pixels(300,400,1.25)));
    PROFILE_ALLOCATIONS.store(false,Ordering::Relaxed);
    PROFILE_STACKS.with(|samples| {
        let samples=samples.borrow();
        println!("{}",serde_json::json!({"scope":"Diagnostic release allocator stacks: first/every65536 allocations plus requests>=1MiB, max1024 samples; no timing/live/RSS claim","phases":[{"name":"load","allocations":load_allocations,"requested_bytes":load_bytes},{"name":"sync","allocations":sync_allocations,"requested_bytes":sync_bytes},{"name":"frame","allocations":frame_allocations,"requested_bytes":frame_bytes}],"sample_fields":["phase","allocation_index","requested_bytes_so_far","backtrace_with_current_request_size"],"samples":*samples}));
    });
}


fn cpu_frame_probe() {
    let files=BTreeMap::from([("App.ui".into(),include_str!("../tests/fixtures/svet-native-ime/App.ui").into())]);
    let mut rows=Vec::new();
    for scale in [1_f32,1.25,2.] {
        for focused in [false,true] {
            let mut runtime=Runtime::from_ui_files(&files,"App.ui",&Properties::new()).unwrap();
            assert_eq!(runtime.control_count(),4);
            runtime.set_binding_value(1,"你好 мир🙂",true).unwrap();
            runtime.sync_ui_bindings_native().unwrap();
            if focused {assert!(runtime.focus_control(1));} else {runtime.focus(false);}
            let width=(640.*scale) as u32;let height=(360.*scale) as u32;
            let mut out=vec![0_u32;(width*height) as usize];
            for _ in 0..10 {runtime.tick(1000./60.);runtime.sync_ui_bindings_native().unwrap();runtime.paint_native(&mut out,width,height,scale).unwrap();}
            let mut samples=Vec::new();
            for _ in 0..30 {
                let before=runtime.raster_stats();
                let (_,ms,allocations,requested_bytes)=measured(|| {
                    runtime.tick(1000./60.);runtime.sync_ui_bindings_native().unwrap();
                    runtime.paint_native(&mut out,width,height,scale).unwrap();
                    std::hint::black_box(&out);
                });
                let after=runtime.raster_stats();
                samples.push(serde_json::json!({"ms":ms,"allocations":allocations,"requested_bytes":requested_bytes,"leaf_cache_builds":after[0]-before[0],"leaf_cache_paints":after[1]-before[1]}));
            }
            assert_eq!(runtime.text_value(1),"你好 мир🙂");
            rows.push(serde_json::json!({"scale":scale,"focused":focused,"width":width,"height":height,"samples":samples}));
        }
    }
    println!("{}",serde_json::json!({"scope":"Single-process release CPU tick+retained binding sync+paint_native, 30 frames after10 warmup frames, simulated60Hz dt; no OS present/probe-journal cost; allocator-instrumented timings, not app latency","rows":rows}));
}

/// Sustained allocator accounting, including repeated binding replacement and
/// CPU cache release/reconstruction. Diagnostics are outside the tracked heap.
fn cpu_lifecycle_probe() {
    let files = BTreeMap::from([("App.ui".to_owned(), "component App { state items: List = []; Column { width:260; height:16384; let visible = state.items; for item in visible key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".to_owned())]);
    let mut rows = Vec::new();
    for svet in [false, true] {
        for count in [8usize, 32, 96, 256] {
            let run = || {
                let state = Properties::from([("items".into(), Value::List((0..count).map(|i| Value::Object(Properties::from([
                    ("id".into(), Value::Text(format!("id{i}"))),
                    ("title".into(), Value::Text(format!("title{i}"))),
                ]))).collect()))]);
                let mut runtime = Runtime::from_ui_files(&files,"App.ui",&state).unwrap();
                runtime.set_svet_document_scheduling(svet);
                drop(state);
                let mut rgba = vec![0; 300*400*4];
                let mut native = vec![0; 300*400];
                let mut live_by_edit=[0i128;32];
                for edit in 0..32 {
                    let index = if edit % 2 == 0 {0} else {count-1};
                    let value = format!("edit-{edit:04} — 你好 мир🙂");
                    runtime.set_binding_value(index,&value,true).unwrap();
                    assert!(runtime.sync_ui_bindings_native().unwrap());
                    assert_eq!(runtime.text_value(index),value);
                    runtime.tick(16.);
                    if edit % 4 == 0 { runtime.release_cpu_cache(); }
                    runtime.paint_rgba(&mut rgba,300,400,1.25,true).unwrap();
                    runtime.paint_native(&mut native,300,400,1.25).unwrap();
                    // Independent public owned-output equivalence; its temporary
                    // allocation is included in peak/requested, then released.
                    assert_eq!(runtime.pixels(300,400,1.25),rgba);
                    for (pixel, &xrgb) in rgba.chunks_exact(4).zip(&native) {
                        let a=pixel[3] as u32;
                        let c=|i:usize,bg:u32|(pixel[i] as u32*a+bg*(255-a)+127)/255;
                        assert_eq!(xrgb,c(0,17)<<16|c(1,19)<<8|c(2,25));
                    }
                    drop(value);
                    live_by_edit[edit]=LIVE_DELTA.with(|v|v.get().unwrap_or(0));
                }
                let model=runtime.ui_state_properties();
                let Value::List(items)=&model["items"] else {panic!("items")};
                assert_eq!(items.len(),count);
                for (i,item) in items.iter().enumerate() {
                    let Value::Object(item)=item else {panic!("row")};
                    assert_eq!(item["id"],Value::Text(format!("id{i}")));
                    let expected=if i==0 {"edit-0030 — 你好 мир🙂".to_owned()}
                        else if i==count-1 {"edit-0031 — 你好 мир🙂".to_owned()}
                        else {format!("title{i}")};
                    assert_eq!(item["title"],Value::Text(expected));
                }
                drop(model);
                (runtime,rgba,native,live_by_edit)
            };
            drop(run());
            let mut samples=Vec::new();
            for _ in 0..3 {
                LIVE_DELTA.with(|v|v.set(Some(0)));
                LIVE_PEAK.with(|v|v.set(Some(0)));
                let (owned,ms,allocations,requested_bytes)=measured(run);
                let live_by_edit=owned.3;
                let retained=LIVE_DELTA.with(|v|v.get().unwrap());
                drop(owned);
                let after_drop=LIVE_DELTA.with(|v|v.replace(None).unwrap());
                let peak=LIVE_PEAK.with(|v|v.replace(None).unwrap());
                assert_eq!(after_drop,0,"lifecycle allocator delta did not return to baseline");
                samples.push(serde_json::json!({"ms":ms,"allocations":allocations,"requested_bytes":requested_bytes,"live_bytes_by_edit":live_by_edit,"retained_bytes":retained,"peak_bytes":peak,"after_drop_delta":after_drop}));
            }
            rows.push(serde_json::json!({"svet":svet,"controls":count,"cycles_per_lifecycle":32,"samples":samples}));
        }
    }
    println!("{}",serde_json::json!({"scope":"One warm lifecycle then3 measured lifecycles per scheduler/size,32 alternating first/last edits and CPU RGBA/XRGB frames, cache release every4 edits; exact model, owned pixels and XRGB conversion verified","accounting":"Single-thread net allocator Layout bytes; includes model/runtime and caller output buffers; diagnostics excluded; not RSS, GPU memory, OS latency or long-duration leak proof","rows":rows}));
}
