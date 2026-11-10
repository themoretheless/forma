//! Canvas: application geometry inside a reserved rectangle of a scene.
use forma::Runtime;
use forma::canvas::{
    CanvasBuilder, CanvasError, CanvasEventKind, CanvasView, MAX_EDGES, MAX_PRIMITIVES,
};

const TEMPLATE: &str = "component Button { width: 100; height: 40; text: ''; \
    background: #00000000; Rectangle { Canvas {} } }";

fn scene(other_x: u32) -> String {
    format!(
        "component Demo {{ Frame {{ width: 300; height: 200; background: #000000; \
         Button {{ key: 'graph'; x: 50; y: 40; width: 100; height: 80; }} \
         Button {{ key: 'other'; x: {other_x}; y: 150; width: 40; height: 30; }} }} }}"
    )
}

fn runtime() -> Runtime {
    Runtime::from_sources(&scene(200), TEMPLATE).unwrap()
}

fn red_square() -> forma::canvas::CanvasContent {
    let mut b = CanvasBuilder::new();
    // Far larger than the 100x80 canvas: everything outside must be clipped.
    b.rect([-500., -500., 1000., 1000.], [255, 0, 0, 255])
        .unwrap();
    b.finish()
}

fn pixel(px: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    px[i..i + 4].try_into().unwrap()
}

#[test]
fn canvas_keys_and_bounds_come_from_markup() {
    let rt = runtime();
    assert_eq!(rt.canvas_keys(), ["graph", "other"]);
    assert_eq!(rt.canvas_bounds("graph"), Some([50., 40., 100., 80.]));
    assert_eq!(rt.canvas_bounds("missing"), None);
}

#[test]
fn geometry_is_clipped_to_the_canvas_on_cpu_and_in_the_display_list() {
    let mut rt = runtime();
    rt.set_canvas_content("graph", red_square());
    let px = rt.pixels(300, 200, 1.);
    assert_eq!(pixel(&px, 300, 100, 80), [255, 0, 0, 255]);
    assert_eq!(pixel(&px, 300, 50, 40), [255, 0, 0, 255]);
    assert_eq!(pixel(&px, 300, 149, 119), [255, 0, 0, 255]);
    for (x, y) in [
        (49, 80),
        (150, 80),
        (100, 39),
        (100, 120),
        (10, 10),
        (290, 190),
    ] {
        assert_eq!(pixel(&px, 300, x, y), [0, 0, 0, 255], "({x}, {y})");
    }
    // The shared display list wraps the fragment in a clip of the canvas rect.
    let list = rt.vector_snapshot(1., true);
    let commands: Vec<&[f32]> = list.commands.chunks_exact(20).collect();
    let fill = commands
        .iter()
        .position(|c| c[0] == 1. && c[8] == 1. && c[9] == 0.)
        .expect("canvas fill command");
    let clip = commands[..fill]
        .iter()
        .rposition(|c| c[0] == 2.)
        .expect("canvas clip");
    assert_eq!(&commands[clip][4..8], &[50., 40., 100., 80.]);
    assert_eq!(commands[fill + 1][0], 3.);
}

#[test]
fn view_change_retransforms_without_rebuilding_geometry() {
    let mut rt = runtime();
    let mut b = CanvasBuilder::new();
    for i in 0..50 {
        b.circle([i as f32 * 3., 10.], 2., [0, 255, 0, 255])
            .unwrap();
        b.text([0., 30.], "узел", 10., [255; 4]).unwrap();
    }
    let content = std::sync::Arc::new(b.finish());
    rt.set_canvas_content("graph", content.clone());
    let first = rt.vector_snapshot(1., true);
    rt.pixels(300, 200, 1.);
    let raster_before = rt.raster_stats();
    let stats = rt.canvas_stats("graph").unwrap();
    rt.set_canvas_view("graph", CanvasView::new([7., -3.], 1.5).unwrap())
        .unwrap();
    let second = rt.vector_snapshot(1., true);
    rt.pixels(300, 200, 1.);
    let after = rt.canvas_stats("graph").unwrap();
    // The content itself is never rebuilt: same allocation, same edge count.
    assert!(std::sync::Arc::ptr_eq(
        &rt.canvas_content("graph").unwrap(),
        &content
    ));
    assert_eq!(after.content_sets, stats.content_sets);
    assert_eq!(after.view_sets, stats.view_sets + 1);
    // One placement serves both the GPU list and the CPU raster.
    assert_eq!(after.placements, stats.placements + 1);
    assert_eq!(first.edges.len(), second.edges.len());
    assert!(!std::sync::Arc::ptr_eq(&first, &second));
    // Control rasters (existing counters) are not rebuilt by a view change.
    assert_eq!(rt.raster_stats()[0], raster_before[0]);
    // Same view again is a no-op.
    rt.set_canvas_view("graph", CanvasView::new([7., -3.], 1.5).unwrap())
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &second,
        &rt.vector_snapshot(1., true)
    ));
    assert!(
        rt.set_canvas_view(
            "graph",
            CanvasView {
                pan: [0., 0.],
                zoom: f32::NAN
            }
        )
        .is_err()
    );
}

#[test]
fn content_and_view_survive_a_scene_rebuild() {
    let mut rt = runtime();
    rt.set_canvas_content("graph", red_square());
    let view = CanvasView::new([4., 5.], 2.).unwrap();
    rt.set_canvas_view("graph", view).unwrap();
    // An unrelated control moves: a new scene, carried over by key.
    let mut next = Runtime::from_sources(&scene(220), TEMPLATE).unwrap();
    next.preserve_interaction(&rt);
    assert_eq!(next.canvas_view("graph"), Some(view));
    let px = next.pixels(300, 200, 1.);
    assert_eq!(pixel(&px, 300, 100, 80), [255, 0, 0, 255]);
}

#[test]
fn pointer_and_wheel_map_through_pan_and_zoom() {
    let mut rt = runtime();
    rt.set_canvas_content("graph", red_square());
    rt.set_canvas_view("graph", CanvasView::new([10., 20.], 2.).unwrap())
        .unwrap();
    rt.pointer(5., 5., 0); // outside every canvas
    rt.pointer(70., 60., 0); // local (20, 20) -> content (5, 0)
    rt.pointer(70., 60., 1);
    rt.pointer(200., 190., 0); // captured: still delivered, outside the rect
    rt.pointer(200., 190., 2);
    let events = rt.take_canvas_events();
    let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            CanvasEventKind::Move,
            CanvasEventKind::Press,
            CanvasEventKind::Move,
            CanvasEventKind::Release,
            CanvasEventKind::Leave,
        ]
    );
    assert!(events.iter().all(|e| e.key == "graph"));
    assert_eq!(events[0].local, [20., 20.]);
    assert_eq!(events[0].content, [5., 0.]);
    assert_eq!(events[2].local, [150., 150.]);
    assert_eq!(events[2].content, [70., 65.]);
    // Wheel over the canvas goes to the application with the pointer position.
    rt.pointer(90., 50., 0);
    rt.scroll(0., -3.);
    let events = rt.take_canvas_events();
    assert_eq!(events[1].kind, CanvasEventKind::Wheel { dx: 0., dy: -3. });
    assert_eq!(events[1].local, [40., 10.]);
    assert_eq!(events[1].content, [15., -5.]);
    // Content point helper agrees with the events.
    assert_eq!(
        rt.canvas_point_to_content("graph", 90., 50.),
        Some([15., -5.])
    );
}

#[test]
fn limits_are_errors_not_panics() {
    let mut b = CanvasBuilder::new();
    for _ in 0..MAX_PRIMITIVES {
        b.rect([0., 0., 0., 0.], [0; 4]).unwrap();
    }
    assert_eq!(
        b.line([0., 0.], [1., 1.], 1., [255; 4]),
        Err(CanvasError::TooManyPrimitives {
            limit: MAX_PRIMITIVES
        })
    );
    let mut b = CanvasBuilder::new();
    let big: Vec<[f32; 2]> = (0..60_000).map(|i| [i as f32, (i % 7) as f32]).collect();
    let mut result = Ok(());
    for _ in 0..(MAX_EDGES / 60_000 + 1) {
        result = b.polygon(&big, [255; 4]);
        if result.is_err() {
            break;
        }
    }
    assert_eq!(result, Err(CanvasError::TooManyEdges { limit: MAX_EDGES }));
    assert!(b.finish().edge_count() <= MAX_EDGES);
}

/// Graph-sized workload: 5000 filled circles and 20000 lines. Times are
/// printed (`--nocapture`); the assertion only guards correctness.
#[test]
fn graph_workload_builds_and_paints_on_cpu() {
    let mut rt = Runtime::from_sources(
        "component Demo { Frame { width: 1280; height: 800; background: #000000; \
         Button { key: 'graph'; x: 0; y: 0; width: 1280; height: 800; } \
         Button { key: 'other'; x: 0; y: 0; width: 1; height: 1; } } }",
        TEMPLATE,
    )
    .unwrap();
    let mut seed = 1u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let nodes: Vec<[f32; 2]> = (0..5000).map(|_| [next() * 1280., next() * 800.]).collect();
    let started = std::time::Instant::now();
    let mut b = CanvasBuilder::with_capacity(25_000, 200_000);
    for i in 0..20_000 {
        let a = nodes[i % 5000];
        let c = nodes[(i * 7919 + 13) % 5000];
        b.line(a, c, 1., [90, 110, 160, 160]).unwrap();
    }
    for &n in &nodes {
        b.circle(n, 4., [240, 200, 80, 255]).unwrap();
    }
    let content = b.finish();
    let build = started.elapsed();
    let started = std::time::Instant::now();
    rt.set_canvas_content("graph", content);
    let mut out = vec![0u8; 1280 * 800 * 4];
    rt.paint_rgba(&mut out, 1280, 800, 1., true).unwrap();
    let paint = started.elapsed();
    let started = std::time::Instant::now();
    rt.set_canvas_view("graph", CanvasView::new([-100., -50.], 1.25).unwrap())
        .unwrap();
    rt.paint_rgba(&mut out, 1280, 800, 1., true).unwrap();
    let pan_zoom = started.elapsed();
    let started = std::time::Instant::now();
    let list = rt.vector_snapshot(1., true);
    let gpu_list = started.elapsed();
    println!(
        "canvas 5000 circles + 20000 lines: build {build:?}, first CPU paint {paint:?}, \
         view change + CPU paint {pan_zoom:?}, merged display list {gpu_list:?} \
         ({} commands, {} edges)",
        list.commands.len() / 20,
        list.edges.len() / 4
    );
    assert!(out.chunks_exact(4).any(|p| p == [240, 200, 80, 255]));
    // Laid-out graphs mostly join neighbours: the same counts with edges of at
    // most ~40 px instead of random 500 px chords.
    let mut b = CanvasBuilder::with_capacity(25_000, 200_000);
    for i in 0..20_000 {
        let a = nodes[i % 5000];
        let c = [a[0] + next() * 40. - 20., a[1] + next() * 40. - 20.];
        b.line(a, c, 1., [90, 110, 160, 160]).unwrap();
    }
    for &n in &nodes {
        b.circle(n, 4., [240, 200, 80, 255]).unwrap();
    }
    rt.set_canvas_content("graph", b.finish());
    let started = std::time::Instant::now();
    rt.paint_rgba(&mut out, 1280, 800, 1., true).unwrap();
    println!("canvas short edges: CPU paint {:?}", started.elapsed());
}
