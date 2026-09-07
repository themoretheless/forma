//! Retained Rust bytes released by the CPU -> GPU cache handoff, excluding
//! actual GPU resources. This measures the same release hook used by renderers.
mod bench_support;
use bench_support::CountingAllocator;
use forma::Runtime;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

fn digest(pixels: &[u32]) -> u64 {
    pixels.iter().fold(0xcbf29ce484222325, |h, p| p.to_le_bytes().into_iter()
        .fold(h, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)))
}

fn main() {
    const TEMPLATE: &str = "component Button { Rectangle { radius:7; background:#324674cc; Text { text:'Forma Я'; fontSize:13; color:#ffffff; } PointerArea { clicked -> events.clicked(); } } }";
    for count in [1, 8, 32] {
        let controls = (0..count).map(|i| format!("Button {{ x:{}; y:{}; width:106; height:26; }}", 12+i%4*124, 10+i/4*34)).collect::<String>();
        let source = format!("component Demo {{ Frame {{ width:512; height:288; background:#102030; {controls} }} }}");
        let model = Runtime::from_sources(&source, TEMPLATE).unwrap();
        // Geometry remains live across cache release, as it does with a GPU.
        let geometry = model.vector_snapshot(1.25, true);
        let mut host = vec![0; 640*360];
        model.paint_native(&mut host, 640, 360, 1.25).unwrap();
        let expected = digest(&host);
        let before = ALLOCATOR.snapshot();
        model.release_cpu_cache();
        let after = ALLOCATOR.snapshot();
        for _ in 0..100 { model.release_cpu_cache(); }
        let repeated = ALLOCATOR.snapshot().delta_since(after);
        assert_eq!(repeated.allocations + repeated.reallocations + repeated.deallocations, 0);
        assert!(std::sync::Arc::ptr_eq(&geometry, &model.vector_snapshot(1.25, true)));
        model.paint_native(&mut host, 640, 360, 1.25).unwrap();
        assert_eq!(digest(&host), expected, "CPU fallback must reproduce the same pixels");
        println!("{{\"controls\":{count},\"released_rust_bytes\":{},\"repeated_release_allocations\":0,\"fallback_digest\":\"{expected:016x}\"}}", before.live_bytes-after.live_bytes);
    }
}
