//! Load cost of the real Studio transport (`FORMA-TEMPLATES-1`, icon-heavy).
//! Inputs are produced by `node scripts/transport-fixture.mjs`; one JSON object
//! per measured phase. Parse is timed apart from the rest of construction because
//! only the transport parse scales with repeated template bytes.
mod bench_support;
use bench_support::CountingAllocator;
use forma::display_list::DisplayList;
use forma::{markup::ButtonSpec, template, Runtime};
use std::{hint::black_box, time::{Duration, Instant}};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

const SEPARATOR: &str = "\n@@@SPLIT@@@\n";

fn transport(component: &str) -> Vec<&str> {
    // One control ships as a bare template; more controls use the framed form.
    let Some(rest) = component.strip_prefix("FORMA-TEMPLATES-1\n") else {
        return vec![component];
    };
    let mut rest = rest;
    let mut out = Vec::new();
    while !rest.is_empty() {
        let (length, body) = rest.split_once('\n').unwrap();
        let length = length.parse::<usize>().unwrap();
        out.push(&body[..length]);
        rest = &body[length..];
    }
    out
}

fn digest(list: &DisplayList) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for value in list.commands.iter().chain(list.edges.iter()) {
        for byte in value.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x1000_0000_01b3);
        }
    }
    hash
}

fn record(case: &str, count: usize, ops: usize, started: Duration, allocations: u64, bytes: u64, retained: i128) {
    let n = ops as f64;
    println!("{{\"case\":\"{case}\",\"controls\":{count},\"ns_per_op\":{:.2},\"alloc_realloc_per_op\":{:.2},\"requested_bytes_per_op\":{:.2},\"live_bytes_change_per_op\":{:.2}}}",
        started.as_secs_f64() * 1e9 / n, allocations as f64 / n, bytes as f64 / n, retained as f64 / n);
}


fn main() {
    let dir = std::env::var("FORMA_TRANSPORT_DIR")
        .unwrap_or_else(|_| format!("{}/../.forma/perf/template-load", env!("CARGO_MANIFEST_DIR")));
    for count in [1usize, 32, 256] {
        let path = format!("{dir}/forma-transport-{count}.txt");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("missing {path}: run `node scripts/transport-fixture.mjs` first ({error})")
        });
        let (source, component) = text.split_once(SEPARATOR).unwrap();
        let templates = transport(component);
        assert_eq!(templates.len(), count);
        let template_bytes: usize = templates.iter().map(|t| t.len()).sum();

        // Parse only: one lexer + template build per control template.
        for t in &templates {
            drop(template::parse(t, &ButtonSpec::default()).unwrap());
        }
        let props = ButtonSpec::default();
        let mut elapsed = Duration::ZERO;
        for round in 0..4 {
            let before = ALLOCATOR.snapshot();
            let started = Instant::now();
            for t in &templates {
                black_box(template::parse(black_box(t), &props).unwrap());
            }
            if round == 0 {
                continue; // warm-up: code paths and allocator arenas
            }
            elapsed += started.elapsed();
            if round == 1 {
                let delta = ALLOCATOR.snapshot().delta_since(before);
                record("parse", count, count, started.elapsed(), delta.allocations + delta.reallocations, delta.requested_bytes, delta.live_bytes_change);
            }
        }
        record("parse_total", count, 3 * count, elapsed, 0, 0, 0);

        // Ceiling of a content-keyed parse cache: on a keystroke only one control's
        // template text changes, so 255 of 256 loads would clone a cached Template.
        {
            let cached = template::parse(templates[0], &props).unwrap();
            let mut elapsed = std::time::Duration::ZERO;
            for round in 0..7 {
                let before = ALLOCATOR.snapshot();
                let started = Instant::now();
                for _ in 0..count {
                    black_box(cached.clone());
                }
                if round > 0 {
                    elapsed += started.elapsed();
                    if round == 1 {
                        let delta = ALLOCATOR.snapshot().delta_since(before);
                        record("clone_cached", count, count, started.elapsed(), delta.allocations + delta.reallocations, delta.requested_bytes, delta.live_bytes_change);
                    }
                }
            }
            record("clone_cached_total", count, 5 * count, elapsed, 0, 0, 0);
            let started = Instant::now();
            for _ in 0..count {
                black_box(template::parse(templates[count - 1], &props).unwrap());
            }
            record("parse_repeat", count, count, started.elapsed(), 0, 0, 0);
        }

        // How much of a parse is the numeric payload itself?
        {
            // Every numeric payload in the first template: what a perfect
            // number parser could still not remove.
            let template = templates[0];
            let mut numbers: Vec<&str> = Vec::new();
            let mut rest = template;
            while let Some(at) = rest.find("points:") {
                rest = &rest[at..];
                let open = rest.find('\'').unwrap();
                let close = rest[open + 1..].find('\'').unwrap();
                numbers.extend(rest[open + 1..open + 1 + close].split_whitespace());
                rest = &rest[open + 1 + close..];
            }
            let payload: usize = numbers.iter().map(|n| n.len()).sum();
            let mut sum = 0f32;
            let mut elapsed = Duration::ZERO;
            for round in 0..21 {
                let started = Instant::now();
                for word in &numbers {
                    sum += black_box(word.parse::<f32>().unwrap_or(black_box(1.)));
                }
                if round > 0 {
                    elapsed += started.elapsed();
                }
            }
            record("numbers", count, 20, elapsed, 0, 0, 0);
            println!("{{\"case\":\"numbers_info\",\"controls\":{count},\"numbers_per_template\":{},\"digits\":{payload},\"sum\":{sum}}}", numbers.len());
        }

        // Whole load: markup parse + per-control template link and geometry.
        drop(Runtime::from_sources(source, component).unwrap());
        for round in 0..3 {
            let before = ALLOCATOR.snapshot();
            let started = Instant::now();
            let runtime = Runtime::from_sources(black_box(source), black_box(component)).unwrap();
            let delta = ALLOCATOR.snapshot().delta_since(before);
            if round > 0 {
                record("load", count, 1, started.elapsed(), delta.allocations + delta.reallocations, delta.requested_bytes, delta.live_bytes_change);
            }
            assert_eq!(runtime.control_count(), count);
            drop(runtime);
        }

        // A cache hit end to end: key hash, full key compare, then Template clone.
        {
            template::flush_parse_cache();
            for t in &templates { drop(template::parse_cached(t, &props).unwrap()); }
            let mut elapsed = Duration::ZERO;
            for round in 0..5 {
                let before = ALLOCATOR.snapshot();
                let started = Instant::now();
                for t in &templates {
                    black_box(template::parse_cached(black_box(t), &props).unwrap());
                }
                let delta = ALLOCATOR.snapshot().delta_since(before);
                if round == 1 {
                    record("parse_cached", count, count, started.elapsed(), delta.allocations + delta.reallocations, delta.requested_bytes, delta.live_bytes_change);
                }
                elapsed += started.elapsed();
            }
            record("parse_cached_total", count, 4 * count, elapsed, 0, 0, 0);
            let stats = template::parse_cache_stats();
            println!("{{\"case\":\"cache\",\"controls\":{count},\"entries\":{},\"bytes\":{},\"hits\":{},\"misses\":{}}}",
                stats.entries, stats.bytes, stats.hits, stats.misses);
        }

        // Cold load of the same document: what the cache costs when nothing hits.
        {
            template::flush_parse_cache();
            let started = Instant::now();
            let runtime = Runtime::from_sources(black_box(source), black_box(component)).unwrap();
            record("load_cold", count, 1, started.elapsed(), 0, 0, 0);
            assert_eq!(runtime.control_count(), count);
        }

        // Geometry equivalence guard for the parse changes above.
        let runtime = Runtime::from_sources(source, component).unwrap();
        let list = runtime.vector_snapshot(1.25, true);
        println!("{{\"case\":\"digest\",\"controls\":{count},\"template_bytes\":{template_bytes},\"commands\":{},\"edges\":{},\"hash\":{}}}",
            list.commands.len(), list.edges.len(), digest(&list) & 0xffff_ffff_ffff_ffff);
    }
}
