//! CPU raster comparison using only the public API available at e86a171.
//! Warm/animated native phases keep the host buffer alive. Allocation traffic,
//! retained Rust bytes and process RSS are distinct; digests exclude timing.
mod bench_support;
use bench_support::{CountingAllocator, ProcessSnapshot};
use forma::Runtime;
use std::{hint::black_box, time::Instant};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const SCALE: f32 = 1.25;
const TEMPLATE: &str = "component Button { Rectangle { radius:7; background:Brush { color:#324674cc; hover:#80aaffee; transition:180ms; }; Border { width:1.2; background:#d4e0ff99; } Reveal { color:#ffaa77; } Text { text:'Forma Я'; fontSize:13; color:#ffffff; } ContentShape { points:'-3 4 2 4 2 12 -3 12'; color:#dd507099; } PointerArea { clicked -> events.clicked(); } } }";

fn source(count: usize) -> String {
    let mut controls = String::new();
    for i in 0..count {
        use std::fmt::Write;
        write!(controls, "Button {{ key:'c{i}'; x:{}; y:{}; width:106; height:26; }}", 12+i%4*124, 10+i/4*34).unwrap();
    }
    format!("component Demo {{ Frame {{ width:512; height:288; padding:0; radius:12; background:#102030c0; {controls} }} }}")
}
fn digest(pixels: &[u32]) -> u64 {
    pixels.iter().fold(0xcbf29ce484222325, |h, p| p.to_le_bytes().into_iter().fold(h, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)))
}
fn measure(case: &str, count: usize, frames: usize, retained_base: u64, runtime: &mut Runtime, out: &mut [u32]) {
    let initial_digest = digest(out);
    ALLOCATOR.reset_peak();
    let start = ALLOCATOR.snapshot();
    let cpu_start = ProcessSnapshot::capture().unwrap();
    let time = Instant::now();
    for i in 0..frames {
        if case == "animation_native" {
            let control = i / 2 % count;
            if i % 2 == 0 {
                runtime.pointer((24+control%4*124) as f32, (20+control/4*34) as f32, 0);
            } else {
                runtime.pointer(-1., -1., 3);
            }
            runtime.tick(16.);
        }
        runtime.paint_native(black_box(out), WIDTH, HEIGHT, SCALE).unwrap();
        black_box(&out);
    }
    let elapsed = time.elapsed().as_secs_f64();
    let end = ALLOCATOR.snapshot();
    let cpu_end = ProcessSnapshot::capture().unwrap();
    let delta = end.delta_since(start);
    let n = frames as f64;
    println!("{{\"case\":\"{case}\",\"controls\":{count},\"frames\":{frames},\"width\":{WIDTH},\"height\":{HEIGHT},\"scale\":{SCALE},\"ns_per_frame\":{:.2},\"cpu_ns_per_frame\":{:.2},\"alloc_realloc_per_frame\":{:.2},\"requested_bytes_per_frame\":{:.2},\"rust_retained_scene_bytes\":{},\"rust_peak_bytes\":{},\"rss_bytes\":{},\"digest\":\"{:016x}\",\"changed\":{}}}",
        elapsed*1e9/n, (cpu_end.cpu_seconds-cpu_start.cpu_seconds)*1e9/n,
        (delta.allocations+delta.reallocations) as f64/n, delta.requested_bytes as f64/n,
        end.live_bytes.saturating_sub(retained_base), end.peak_live_bytes,
        cpu_end.resident_bytes, digest(out), digest(out)!=initial_digest);
}
fn main() {
    // Warm font initialization outside all scene measurements.
    let mut warm = Runtime::from_sources(&source(1), TEMPLATE).unwrap();
    let mut host = vec![0; (WIDTH*HEIGHT) as usize];
    warm.paint_native(&mut host, WIDTH, HEIGHT, SCALE).unwrap();
    warm.pointer(24., 20., 0);
    drop(warm);
    for count in [1, 8, 32] {
        let source = source(count);
        let retained_base = ALLOCATOR.snapshot().live_bytes;
        let mut runtime = Runtime::from_sources(&source, TEMPLATE).unwrap();
        host.fill(0);
        measure("cold_native", count, 1, retained_base, &mut runtime, &mut host);
        measure("warm_native", count, 30, retained_base, &mut runtime, &mut host);
        measure("animation_native", count, 48, retained_base, &mut runtime, &mut host);
    }
}
