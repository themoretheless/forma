//! Phase attribution for `Runtime::from_sources` on the real Studio transport:
//! where the load time and allocations of one compile go, control by control.
//! Inputs come from `node scripts/transport-fixture.mjs`; one JSON object per case.
mod bench_support;
use bench_support::CountingAllocator;
use forma::{markup, template, Runtime};
use std::{hint::black_box, time::{Duration, Instant}};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

const SEPARATOR: &str = "\n@@@SPLIT@@@\n";

fn measure(label: &str, count: usize, ops: usize, mut f: impl FnMut()) {
    f();
    let mut elapsed = Duration::ZERO;
    let mut allocations = 0u64;
    let mut bytes = 0u64;
    for round in 0..ops {
        let before = ALLOCATOR.snapshot();
        let started = Instant::now();
        f();
        let delta = ALLOCATOR.snapshot().delta_since(before);
        if round > 0 {
            elapsed += started.elapsed();
            allocations += delta.allocations + delta.reallocations;
            bytes += delta.requested_bytes;
        }
    }
    let n = (ops - 1) as f64;
    println!("{{\"case\":\"{label}\",\"controls\":{count},\"ns_per_op\":{:.1},\"allocs_per_op\":{:.1},\"bytes_per_op\":{:.1}}}",
        elapsed.as_secs_f64() * 1e9 / n, allocations as f64 / n, bytes as f64 / n);
}

fn framed(component: &str) -> Vec<&str> {
    let mut rest = component.strip_prefix("FORMA-TEMPLATES-1\n").unwrap();
    let mut out = Vec::new();
    while !rest.is_empty() {
        let (length, body) = rest.split_once('\n').unwrap();
        let length: usize = length.parse().unwrap();
        out.push(&body[..length]);
        rest = &body[length..];
    }
    out
}

fn main() {
    let dir = std::env::var("FORMA_TRANSPORT_DIR")
        .unwrap_or_else(|_| format!("{}/../.forma/perf/template-load", env!("CARGO_MANIFEST_DIR")));
    for count in [32usize, 256] {
        let path = format!("{dir}/forma-transport-{count}.txt");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("missing {path}: run `node scripts/transport-fixture.mjs` first ({error})")
        });
        let (source, component) = text.split_once(SEPARATOR).unwrap();
        let templates = framed(component);
        let scene = markup::parse(source).unwrap();
        let spec = &scene.buttons[0];
        for t in &templates {
            drop(template::parse_cached(t, spec).unwrap());
        }
        let cached = template::parse_cached(templates[0], spec).unwrap();

        // Everything a compile pays per control, then its parts.
        measure("full_from_sources", count, 6, || {
            drop(Runtime::from_sources(black_box(source), black_box(component)).unwrap());
        });
        measure("markup_parse_document", count, 6, || {
            black_box(markup::parse(source).unwrap().buttons.len());
        });
        // The framed transport walk `runtime::templates` performs.
        measure("framing_scan", count, 6, || {
            black_box(framed(component).len());
        });
        // One of the deep ButtonSpec clones a control makes.
        measure("spec_clone_x1", count, 6, || {
            for _ in 0..count {
                black_box(spec.clone().width);
            }
        });
        // The default spec every leaf carries: its name-keyed maps are built from scratch.
        measure("spec_default_x_all", count, 6, || {
            for _ in 0..count {
                let spec = markup::ButtonSpec::default();
                black_box(spec.width + spec.height);
            }
        });
        // Leaf scene per control: frame metadata plus the spec it owns.
        measure("leaf_scene_build", count, 6, || {
            for _ in 0..count {
                let leaf = markup::Scene {
                    name: String::new(), width: scene.width, height: scene.height,
                    background: scene.background, overflow: String::new(),
                    clip: scene.clip, radius: scene.radius, scroll: scene.scroll,
                    padding: scene.padding, content_width: scene.content_width,
                    content_height: scene.content_height, gap: scene.gap,
                    button: markup::ButtonSpec::default(), buttons: Vec::new(),
                };
                black_box(leaf.width + leaf.button.width);
            }
        });
        // Cached template hand-out per control (warm cache: every call shares one Arc).
        measure("parse_cached_x_all", count, 6, || {
            for t in &templates {
                let value = template::parse_cached(t, spec).unwrap();
                black_box(value.props.width + value.radius);
            }
        });
        // What handing out a clone instead of a shared reference used to cost.
        measure("template_clone_x_all", count, 6, || {
            for _ in 0..count {
                let value = (*cached).clone();
                black_box(value.props.width + value.radius);
            }
        });
        measure("props_clone_x_all", count, 6, || {
            for _ in 0..count {
                black_box(cached.props.clone().width);
            }
        });
        // GPU payloads the browser requests after every compile. On a brand new model this is
        // where the display list is actually built, so it is keystroke cost that
        // `full_from_sources` never sees; the warm case isolates the pure clone.
        let model = Runtime::from_sources(source, component).unwrap();
        measure("gpu_transport_new_model", count, 6, || {
            let fresh = Runtime::from_sources(black_box(source), black_box(component)).unwrap();
            black_box(fresh.gpu_commands(1., false).len());
            black_box(fresh.gpu_edges(1., false).len());
            black_box(fresh.gpu_tiles(900, 600, 1., false).len());
        });
        measure("gpu_transport_warm", count, 6, || {
            black_box(model.gpu_commands(1., false).len());
            black_box(model.gpu_edges(1., false).len());
            black_box(model.gpu_tiles(900, 600, 1., false).len());
        });
        println!("{{\"case\":\"input_sizes\",\"controls\":{count},\"document_bytes\":{},\"transport_bytes\":{}}}",
            source.len(), component.len());
    }
}
