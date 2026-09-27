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
fn mixed_size_source(count: usize) -> String {
    // Alternating control sizes, so a capacity hint taken from the previous sibling is wrong
    // about as often as it is right.
    let controls: String = (0..count)
        .map(|i| format!("Button {{ key:'c{i}'; width:{}; height:{}; }}", if i % 2 == 0 { 40 } else { 160 }, if i % 2 == 0 { 20 } else { 60 }))
        .collect();
    format!("component Demo {{ Frame {{ width:200; height:200; {controls} }} }}")
}
const LABELED: &str = "component Button { Rectangle { Text { text: props.text; color:#102030; fontSize:12; } PointerArea { clicked -> events.clicked(); } } }";
const VISUAL: &str = "component Button { Rectangle { background:#102030; PointerArea { clicked -> events.clicked(); } } }";
const EDITOR: &str = "component Button { Rectangle { ContentInput { width:100; height:30; value:'Привет'; } PointerArea { clicked -> events.clicked(); } } }";
const RANGE: &str = "component Button { Rectangle { RangeInput { value:0.5; Minimum { ContentShape { points:'0 0 10 0 10 10'; color:#ffffff; } } Maximum { ContentShape { points:'0 0 80 0 80 10'; color:#ffffff; } } } PointerArea { clicked -> events.clicked(); } } }";

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
