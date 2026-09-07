//! Allocation budgets for warmed runtime paths. Counts are thread-local so the
//! test harness and concurrent tests do not affect these measurements.
use forma::{display_list::RenderScene, Runtime};
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
}
