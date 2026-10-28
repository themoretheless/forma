use compute_core::{
    BinaryOp, ComputeRuntime, FusionError, FusionGraph, UnaryOp, gpu_compute::GpuContext,
};
use std::time::Duration;

fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    assert!(
        context.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "GPU required"
    );
    context
}

#[test]
fn graph_rejects_foreign_handles_even_when_node_indices_match() {
    let mut first = FusionGraph::new(1);
    let mut second = FusionGraph::new(1);
    let a = first.input(0).unwrap();
    let b = second.input(0).unwrap();
    assert!(matches!(
        first.unary(UnaryOp::Square, &b),
        Err(FusionError::ForeignExpression)
    ));
    assert!(matches!(
        first.binary(BinaryOp::Add, &a, &b),
        Err(FusionError::ForeignExpression)
    ));
    assert!(matches!(
        first.binary(BinaryOp::Add, &b, &a),
        Err(FusionError::ForeignExpression)
    ));
    assert!(matches!(
        first.affine(&b, 2.0, 1.0),
        Err(FusionError::ForeignExpression)
    ));
    // Failed attempts do not invalidate local handles or their clones.
    let squared = first.unary(UnaryOp::Square, &a.clone()).unwrap();
    first.binary(BinaryOp::Add, &a, &squared).unwrap();
}

#[test]
fn graph_validates_slots_and_all_host_supplied_floats_without_gpu() {
    let mut graph = FusionGraph::new(2);
    assert!(matches!(
        graph.input(2),
        Err(FusionError::InputSlot { slot: 2, count: 2 })
    ));
    assert!(matches!(
        graph.input(usize::MAX),
        Err(FusionError::InputSlot {
            slot: usize::MAX,
            count: 2
        })
    ));
    let input = graph.input(1).unwrap();
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(matches!(
            graph.constant(value),
            Err(FusionError::NonfiniteConstant)
        ));
        assert!(matches!(
            graph.affine(&input, value, 0.0),
            Err(FusionError::NonfiniteConstant)
        ));
        assert!(matches!(
            graph.affine(&input, 1.0, value),
            Err(FusionError::NonfiniteConstant)
        ));
    }
    for value in [0.0, -0.0, f32::MAX, f32::MIN, f32::MIN_POSITIVE] {
        let constant = graph.constant(value).unwrap();
        graph.binary(BinaryOp::Add, &input, &constant).unwrap();
    }
    let mut constant_only = FusionGraph::new(0);
    assert!(matches!(
        constant_only.input(0),
        Err(FusionError::InputSlot { slot: 0, count: 0 })
    ));
    constant_only.constant(3.5).unwrap();
}

#[test]
fn seven_live_inputs_use_all_uniform_step_fields_with_scalar_broadcast() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let mut graph = FusionGraph::new(7);
    let mut expression = graph.input(0).unwrap();
    for slot in 1..7 {
        let input = graph.input(slot).unwrap();
        expression = graph.binary(BinaryOp::Add, &expression, &input).unwrap();
    }
    let kernel = graph.compile(&context, &[expression]).unwrap();
    let len = 259;
    let mut inputs = Vec::new();
    for slot in 0..6 {
        let values: Vec<f32> = (0..len)
            .map(|i| slot as f32 + (i % 11) as f32 * 0.25)
            .collect();
        inputs.push(runtime.upload(&values).unwrap());
    }
    // step6 lies beyond the second 16-byte uniform block. Its zero value must
    // still broadcast, while every earlier input uses elementwise indexing.
    inputs.push(runtime.upload(&[1.5f32]).unwrap());
    let mut program = runtime.program();
    let output = program
        .fused(&kernel, &inputs.iter().collect::<Vec<_>>(), len)
        .unwrap();
    let actual = program
        .submit_read(&output[0])
        .unwrap()
        .wait(Duration::from_secs(10))
        .unwrap();
    let expected: Vec<f32> = (0..len)
        .map(|i| 16.5 + 6.0 * (i % 11) as f32 * 0.25)
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn repeated_readonly_input_allocations_are_accepted() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let mut graph = FusionGraph::new(2);
    let a = graph.input(0).unwrap();
    let b = graph.input(1).unwrap();
    let product = graph.binary(BinaryOp::Multiply, &a, &b).unwrap();
    let kernel = graph.compile(&context, &[product]).unwrap();
    let values: Vec<f32> = (0..513).map(|i| (i % 17) as f32 + 0.5).collect();
    let input = runtime.upload(&values).unwrap();
    let clone = input.clone();
    let mut program = runtime.program();
    let output = program
        .fused(&kernel, &[&input, &clone], values.len())
        .unwrap();
    let actual = program
        .submit_read(&output[0])
        .unwrap()
        .wait(Duration::from_secs(10))
        .unwrap();
    assert_eq!(actual, values.iter().map(|v| v * v).collect::<Vec<_>>());
}
