//! Native host-side binding projection using the same Svet dependency graph as
//! WASM. This exercises scheduling; it does not create a native window/renderer.
#[cfg(not(target_arch = "wasm32"))]
use svet_wasm::native::{Context, Kind, Node};

#[cfg(not(target_arch = "wasm32"))]
struct Model {
    left: i32,
    right: i32,
    show_left: bool,
}
#[cfg(not(target_arch = "wasm32"))]
fn render(graph: &mut Context, view: &Node, fields: &[Node; 3], model: &Model) -> String {
    graph
        .track(view, |reads| {
            reads.read(&fields[2]).unwrap();
            let value = if model.show_left {
                reads.read(&fields[0]).unwrap();
                model.left
            } else {
                reads.read(&fields[1]).unwrap();
                model.right
            };
            format!("Count: {value}")
        })
        .unwrap()
}
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let mut graph = Context::new();
    let fields = [
        graph.create(Kind::Signal).unwrap(),
        graph.create(Kind::Signal).unwrap(),
        graph.create(Kind::Signal).unwrap(),
    ];
    let view = graph.create(Kind::Effect).unwrap();
    let mut model = Model {
        left: 1,
        right: 10,
        show_left: true,
    };
    assert_eq!(render(&mut graph, &view, &fields, &model), "Count: 1");
    graph
        .batch(|graph| {
            model.left = 2;
            assert_eq!(model.left, 2);
            graph.notify(&fields[0]).unwrap();
            model.left = 3;
            graph.notify(&fields[0]).unwrap();
        })
        .unwrap();
    let jobs = graph.take_effects();
    assert_eq!(jobs.len(), 1);
    assert_eq!(render(&mut graph, &jobs[0], &fields, &model), "Count: 3");
    model.show_left = false;
    graph.notify(&fields[2]).unwrap();
    let jobs = graph.take_effects();
    assert_eq!(jobs.len(), 1);
    assert_eq!(render(&mut graph, &jobs[0], &fields, &model), "Count: 10");
    model.left = 4;
    assert_eq!(model.left, 4);
    graph.notify(&fields[0]).unwrap();
    assert!(graph.take_effects().is_empty());
    graph.dispose(&view).unwrap();
    for field in &fields {
        graph.dispose(field).unwrap();
    }
    assert_eq!(graph.telemetry().nodes, 0);
    assert_eq!(graph.telemetry().edges, 0);
    println!("native binding projection passed: batched writes, dynamic dependency switch, graph0");
}

#[cfg(target_arch = "wasm32")]
fn main() {}
