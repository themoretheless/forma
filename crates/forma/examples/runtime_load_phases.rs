//! Phase attribution for `Runtime::from_sources` on the real Studio transport:
//! where the load time and allocations of one compile go, control by control.
//! Inputs come from `node scripts/transport-fixture.mjs`; one JSON object per case.
mod bench_support;
use bench_support::CountingAllocator;
use forma::{DirtyFlags, Runtime, form_document::Document, markup, template};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

const SEPARATOR: &str = "\n@@@SPLIT@@@\n";

fn measure(label: &str, count: usize, ops: usize, mut f: impl FnMut()) {
    f();
    let mut elapsed = Duration::ZERO;
    let mut allocations = 0u64;
    let mut reallocations = 0u64;
    let mut bytes = 0u64;
    for round in 0..ops {
        let before = ALLOCATOR.snapshot();
        let started = Instant::now();
        f();
        let delta = ALLOCATOR.snapshot().delta_since(before);
        if round > 0 {
            elapsed += started.elapsed();
            allocations += delta.allocations;
            reallocations += delta.reallocations;
            bytes += delta.requested_bytes;
        }
    }
    let n = (ops - 1) as f64;
    // `allocs_per_op` keeps its historical meaning of allocation attempts, so the separate
    // realloc count is the one that shows a buffer grown by doubling instead of sized at once.
    println!(
        "{{\"case\":\"{label}\",\"controls\":{count},\"ns_per_op\":{:.1},\"allocs_per_op\":{:.1},\"reallocs_per_op\":{:.1},\"bytes_per_op\":{:.1}}}",
        elapsed.as_secs_f64() * 1e9 / n,
        (allocations + reallocations) as f64 / n,
        reallocations as f64 / n,
        bytes as f64 / n
    );
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

fn reframe_first_template(component: &str, edited_first: &str) -> String {
    let rest = component.strip_prefix("FORMA-TEMPLATES-1\n").unwrap();
    let (length, body) = rest.split_once('\n').unwrap();
    let length: usize = length.parse().unwrap();
    let tail = &body[length..];
    format!(
        "FORMA-TEMPLATES-1\n{}\n{}{}",
        edited_first.len(),
        edited_first,
        tail
    )
}

fn build_edited_ir_pair(
    count: usize,
) -> (
    forma::form_document::SceneIr,
    forma::form_document::SceneIr,
    forma::form_document::SceneIr,
) {
    use std::collections::BTreeMap;
    let controls = (0..count)
        .map(|i| {
            format!(
                "Button {{ key: 'b{i}'; width: 180; height: 50; text: {}; background: {}; }}",
                if i == 0 { "state.firstLabel" } else { "'Item'" },
                if i == 0 { "state.firstBg" } else { "#8ca5ff" }
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let app_ui = format!(
        "component App {{ state firstLabel: String = 'Item 0'; state firstBg: String = '#8ca5ff'; Frame {{ width: 900; height: 600; {controls} }} }}"
    );
    let files = BTreeMap::from([
        (
            "components/Button.ui".to_owned(),
            forma::BUTTON_COMPONENT.to_owned(),
        ),
        ("App.ui".to_owned(), app_ui),
    ]);
    let mut doc = Document::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let base_ir = doc.render_scene_ir().unwrap();

    doc.states[0].insert(
        "firstBg".into(),
        forma::form_document::Value::Text("#ff6688".into()),
    );
    let paint_ir = doc.render_scene_ir().unwrap();

    doc.states[0].insert(
        "firstLabel".into(),
        forma::form_document::Value::Text("Edited 0".into()),
    );
    doc.states[0].insert(
        "firstBg".into(),
        forma::form_document::Value::Text("#8ca5ff".into()),
    );
    let text_ir = doc.render_scene_ir().unwrap();
    (base_ir, paint_ir, text_ir)
}

fn main() {
    let dir = std::env::var("FORMA_TRANSPORT_DIR").unwrap_or_else(|_| {
        format!(
            "{}/../../.forma/perf/template-load",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    // Control counts from the command line, e.g. `8 32 96 256`; by default 32 and 256.
    let mut counts: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|arg| arg.parse().ok())
        .collect();
    if counts.is_empty() {
        counts = vec![32, 256];
    }
    for count in counts {
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
                    name: String::new(),
                    width: scene.width,
                    height: scene.height,
                    background: scene.background,
                    overflow: String::new(),
                    clip: scene.clip,
                    radius: scene.radius,
                    scroll: scene.scroll,
                    padding: scene.padding,
                    content_width: scene.content_width,
                    content_height: scene.content_height,
                    gap: scene.gap,
                    button: markup::ButtonSpec::default(),
                    buttons: Vec::new(),
                    modals: Vec::new(),
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

        // Retained Stage 1 incremental edit vs cold Runtime replacement:
        let edited_first_tmpl = templates[0].replacen("Item 0", "Edited 0", 1);
        let edited_source = source.replacen("Item 0", "Edited 0", 1);
        let edited_component = reframe_first_template(component, &edited_first_tmpl);
        let mut src_rt = Runtime::from_sources(source, component).unwrap();
        let mut src_toggle = false;
        measure("reconcile_sources_1_edit", count, 6, || {
            src_toggle = !src_toggle;
            let (s, c) = if src_toggle {
                (edited_source.as_str(), edited_component.as_str())
            } else {
                (source, component)
            };
            let flags = src_rt
                .reconcile_sources(black_box(s), black_box(c))
                .unwrap();
            debug_assert!(!flags.is_empty());
        });

        let (ir_base, ir_paint, ir_text) = build_edited_ir_pair(count);
        measure("cold_from_scene_ir", count, 6, || {
            drop(Runtime::from_scene_ir(black_box(&ir_base)).unwrap());
        });
        let mut ir_rt = Runtime::from_scene_ir(&ir_base).unwrap();
        ir_rt.set_reduced_motion(true);
        let mut paint_toggle = false;
        measure("reconcile_scene_ir_paint_only", count, 6, || {
            paint_toggle = !paint_toggle;
            let target = if paint_toggle { &ir_paint } else { &ir_base };
            let flags = ir_rt.reconcile_scene_ir(black_box(target)).unwrap();
            assert_eq!(flags, DirtyFlags::PAINT);
        });
        let mut edit_toggle = false;
        measure("reconcile_scene_ir_1_edit", count, 6, || {
            edit_toggle = !edit_toggle;
            let target = if edit_toggle { &ir_text } else { &ir_base };
            let flags = ir_rt.reconcile_scene_ir(black_box(target)).unwrap();
            assert!(!flags.is_empty());
        });
        let mut gpu_edit_rt = Runtime::from_scene_ir(&ir_base).unwrap();
        let mut gpu_edit_toggle = false;
        let mut gpu_paints_buf = Vec::new();
        measure("gpu_transport_reconciled_1_edit", count, 6, || {
            use forma::display_list::RenderScene;
            gpu_edit_toggle = !gpu_edit_toggle;
            let target = if gpu_edit_toggle { &ir_text } else { &ir_base };
            gpu_edit_rt.reconcile_scene_ir(black_box(target)).unwrap();
            black_box(gpu_edit_rt.vector_snapshot(1., false).commands.len());
            gpu_edit_rt.gpu_paints_into(&mut gpu_paints_buf);
            black_box(gpu_paints_buf.len());
            black_box(gpu_edit_rt.gpu_tiles(900, 600, 1., false).len());
        });
        println!(
            "{{\"case\":\"input_sizes\",\"controls\":{count},\"document_bytes\":{},\"transport_bytes\":{}}}",
            source.len(),
            component.len()
        );
    }
}
