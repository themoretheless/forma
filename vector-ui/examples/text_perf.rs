//! Text preparation benchmark using the public API available at e86a171.
//! Run the same file on each revision; compare geometry/pixel hashes as well as
//! time and allocation traffic. Cached snapshots and forced rebuilds are separate.
mod bench_support;
use bench_support::CountingAllocator;
use forma::{display_list::DisplayList, Button, Runtime};
use std::{hint::black_box, time::{Duration, Instant}};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

const LABEL: &str = "Forma Привет 0123456789 ffi ОяЖ";
const TEMPLATE: &str = "component Button { Rectangle { background:#102030; ContentText { x:0; y:0; width:600; height:45; text:props.text; fontSize:15.5; color:#ffffffad; } PointerArea { clicked -> events.clicked(); } } }";

fn hash_bytes(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf29ce484222325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3))
}

fn geometry_hash(list: &DisplayList) -> u64 {
    hash_bytes(list.commands.iter().chain(&list.edges).flat_map(|value| value.to_bits().to_le_bytes()))
}

fn measure<T>(case: &str, labels: usize, iterations: usize, mut run: impl FnMut(usize) -> T, hash: impl Fn(&T) -> u64) {
    let mut elapsed = Duration::ZERO;
    let (mut calls, mut bytes, mut live, mut checksum) = (0u64, 0u64, 0i128, 0u64);
    for i in 0..iterations {
        let before = ALLOCATOR.snapshot();
        let started = Instant::now();
        let value = black_box(run(i));
        elapsed += started.elapsed();
        let delta = ALLOCATOR.snapshot().delta_since(before);
        calls += delta.allocations + delta.reallocations;
        bytes += delta.requested_bytes;
        live += delta.live_bytes_change;
        checksum = checksum.rotate_left(1) ^ hash(&value);
        drop(value);
    }
    let count = iterations as f64;
    println!("{{\"case\":\"{case}\",\"labels\":{labels},\"iterations\":{iterations},\"ns_per_op\":{:.2},\"alloc_realloc_per_op\":{:.2},\"requested_bytes_per_op\":{:.2},\"live_bytes_change_per_op\":{:.2},\"geometry_hash\":\"{checksum:016x}\"}}", elapsed.as_secs_f64() * 1e9 / count, calls as f64 / count, bytes as f64 / count, live as f64 / count);
}

fn main() {
    let iterations = std::env::var("FORMA_TEXT_BENCH_ITERATIONS").ok().and_then(|value| value.parse().ok()).unwrap_or(100);
    // Face initialization is excluded from the steady-state preparation cases.
    black_box(forma::text_metrics(LABEL, 15.5));
    for labels in [1, 16, 64] {
        let content: String = (0..labels).map(|i| format!("ContentText {{ x:0; y:{}; width:600; height:45; text:'{LABEL}'; fontSize:15.5; color:#ffffffad; }}", i * 28)).collect();
        let source = format!("component Demo {{ Frame {{ width:320; height:160; Scroll {{ Button {{ width:640; height:{}; }} }} }} }}", labels * 28 + 180);
        let template = format!("component Button {{ Rectangle {{ background:#102030; {content} }} }}");
        let mut button = Button::from_sources(&source, &template).unwrap();
        // Bypass the persistent DisplayList cache to isolate contour preparation.
        measure("button_build", labels, iterations, |_| DisplayList::build(&button, 1.25, true), geometry_hash);
        black_box(button.vector_snapshot(1.25, true));
        measure("button_warm_snapshot", labels, iterations, |_| button.vector_snapshot(1.25, true), |list| geometry_hash(list));
        measure("button_scroll", labels, iterations, |i| {
            button.scroll(if i % 2 == 0 { 0.375 } else { -0.375 }, if i % 2 == 0 { 0.625 } else { -0.625 });
            button.vector_snapshot(1.25, true)
        }, |list| geometry_hash(list));

        let controls: String = (0..labels).map(|i| format!("Button {{ key:'t{i}'; width:640; height:45; text:'{LABEL}'; }}")).collect();
        let source = format!("component Demo {{ Frame {{ width:320; height:160; Scroll {{ {controls} }} }} }}");
        let mut runtime = Runtime::from_sources(&source, TEMPLATE).unwrap();
        black_box(runtime.vector_snapshot(1.25, true));
        measure("runtime_warm_snapshot", labels, iterations, |_| runtime.vector_snapshot(1.25, true), |list| geometry_hash(list));
        measure("runtime_dpi_rebuild", labels, iterations, |i| runtime.vector_snapshot(if i % 2 == 0 { 1. } else { 1.25 }, true), |list| geometry_hash(list));
        measure("runtime_scroll", labels, iterations, |i| {
            runtime.scroll(if i % 2 == 0 { 0.375 } else { -0.375 }, if i % 2 == 0 { 0.625 } else { -0.625 });
            runtime.vector_snapshot(1.25, true)
        }, |list| geometry_hash(list));

        // These correctness hashes are deliberately outside the timed loops.
        for scale in [1., 1.25, 2.] {
            let width = (320. * scale) as u32;
            let height = (160. * scale) as u32;
            let geometry = geometry_hash(&button.vector_snapshot(scale, true));
            let pixels = hash_bytes(button.pixels(width, height, scale));
            println!("{{\"case\":\"button_correctness\",\"labels\":{labels},\"scale\":{scale},\"geometry_hash\":\"{geometry:016x}\",\"pixel_hash\":\"{pixels:016x}\"}}");
        }
    }
}
