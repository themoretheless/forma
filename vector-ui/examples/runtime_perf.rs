//! CPU-only microbench, compatible with the runtime API before these changes.
//! One JSON object per case; allocation traffic is distinct from retained RAM.
mod bench_support;
use bench_support::CountingAllocator;
use forma::Runtime;
use std::{hint::black_box, time::{Duration, Instant}};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

const VISUAL: &str = "component Button { Rectangle { background:#102030; PointerArea { clicked -> events.clicked(); } } }";
const EDITOR: &str = "component Button { Rectangle { ContentInput { width:100; height:30; value:'Привет'; } PointerArea { clicked -> events.clicked(); } } }";
const RANGE: &str = "component Button { Rectangle { RangeInput { value:0.5; Minimum { ContentShape { points:'0 0 10 0 10 10'; color:#ffffff; } } Maximum { ContentShape { points:'0 0 80 0 80 10'; color:#ffffff; } } } PointerArea { clicked -> events.clicked(); } } }";

fn source(count: usize) -> String {
    let controls: String = (0..count).map(|i| format!("Button {{ key:'c{i}'; width:100; height:30; }}")).collect();
    format!("component Demo {{ Frame {{ width:200; height:200; {controls} }} }}")
}

fn measure<T>(case: &str, count: usize, iterations: usize, mut run: impl FnMut(usize) -> T) {
    let mut elapsed = Duration::ZERO;
    let (mut allocations, mut bytes, mut retained) = (0u64, 0u64, 0i128);
    for i in 0..iterations {
        let before = ALLOCATOR.snapshot();
        let started = Instant::now();
        let value = black_box(run(i));
        elapsed += started.elapsed();
        let delta = ALLOCATOR.snapshot().delta_since(before);
        allocations += delta.allocations + delta.reallocations;
        bytes += delta.requested_bytes;
        retained += delta.live_bytes_change;
        // Construction measurements exclude destruction of the completed model.
        drop(value);
    }
    let n = iterations as f64;
    println!("{{\"case\":\"{case}\",\"controls\":{count},\"iterations\":{iterations},\"ns_per_op\":{:.2},\"alloc_realloc_per_op\":{:.2},\"requested_bytes_per_op\":{:.2},\"live_bytes_change_per_op\":{:.2}}}",
        elapsed.as_secs_f64()*1e9/n, allocations as f64/n, bytes as f64/n, retained as f64/n);
}

fn main() {
    for count in [1, 64, 256] {
        let source = source(count);
        drop(Runtime::from_sources(&source, VISUAL).unwrap());
        measure("construct", count, 5, |_| Runtime::from_sources(black_box(&source), VISUAL).unwrap());
        for (name, template) in [("idle", VISUAL), ("idle_editors", EDITOR), ("idle_ranges", RANGE)] {
            let mut runtime = Runtime::from_sources(&source, template).unwrap();
            for _ in 0..20 { black_box(runtime.tick(16.)); }
            measure(name, count, 1000, |_| runtime.tick(black_box(16.)));
        }
        let mut runtime = Runtime::from_sources(&source, VISUAL).unwrap();
        for _ in 0..20 { runtime.pointer(10., 10., 0); }
        measure("pointer", count, 1000, |i| runtime.pointer(black_box(10.), black_box((i%6) as f32*30.+10.), 0));
    }
}
