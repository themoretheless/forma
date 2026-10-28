use compute_core::{
    BinaryOp, ComputeError, ComputeRuntime, FusionError, FusionGraph, KernelCache, UnaryOp,
    gpu_compute::GpuContext,
};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(20);
fn context() -> Option<GpuContext> {
    let c = GpuContext::new();
    if c.is_none() {
        assert!(
            std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
            "GPU required"
        );
        eprintln!("fusion tests skipped: no adapter");
    }
    c
}
fn near(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len());
    for (i, (&a, &b)) in a.iter().zip(b).enumerate() {
        assert!((a - b).abs() < 2e-5 * b.abs().max(1.0), "[{i}] {a} != {b}");
    }
}
#[test]
fn branches_share_expressions_and_prune_dead_nodes_before_caching() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let mut cache = KernelCache::new(&c, 1);
    let mut g = FusionGraph::new(3);
    let a = g.input(0).unwrap();
    let b = g.input(2).unwrap();
    let s = g.binary(BinaryOp::Add, &a, &b).unwrap();
    let duplicate = g.binary(BinaryOp::Add, &a, &b).unwrap();
    let squared = g.binary(BinaryOp::Multiply, &s, &duplicate).unwrap();
    let k = g
        .compile_cached(&mut cache, &[s.clone(), squared.clone()])
        .unwrap();
    assert_eq!(k.operation_count(), 2);
    assert_eq!(k.input_count(), 3);
    let dead = g.input(1).unwrap();
    g.unary(UnaryOp::Sin, &dead).unwrap();
    let same = g.compile_cached(&mut cache, &[s, squared]).unwrap();
    assert_eq!(k.source(), same.source());
    assert_eq!(cache.stats().hits, 1);
    assert!(!k.source().contains("input1:"));
    let x: Vec<f32> = (0..1027).map(|i| i as f32 * 0.125).collect();
    let a = rt.upload(&x).unwrap();
    let unused = rt.upload(&[42.0f32]).unwrap();
    let scalar = rt.upload(&[2.0f32]).unwrap();
    let mut p = rt.program();
    let out = p.fused(&k, &[&a, &unused, &scalar], x.len()).unwrap();
    // A prepared fused kernel remains live after its cache is cleared.
    cache.clear();
    near(
        &p.submit_read(&out[0]).unwrap().wait(TIMEOUT).unwrap(),
        &x.iter().map(|v| v + 2.0).collect::<Vec<_>>(),
    );
    near(
        &rt.read(&out[1]).unwrap().wait(TIMEOUT).unwrap(),
        &x.iter().map(|v| (v + 2.0).powi(2)).collect::<Vec<_>>(),
    );
    rt.write(&scalar, 0, &[3.0]).unwrap();
    near(
        &p.submit_read(&out[1]).unwrap().wait(TIMEOUT).unwrap(),
        &x.iter().map(|v| (v + 3.0).powi(2)).collect::<Vec<_>>(),
    );
}
#[test]
fn generated_operations_and_broadcast_order_match_cpu() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let values: Vec<f32> = (0..259).map(|i| 0.25 + (i % 11) as f32 * 0.125).collect();
    let input = rt.upload(&values).unwrap();
    let scalar = rt.upload(&[0.5f32]).unwrap();
    for op in [
        UnaryOp::Negate,
        UnaryOp::Abs,
        UnaryOp::Square,
        UnaryOp::Sqrt,
        UnaryOp::Reciprocal,
        UnaryOp::Exp,
        UnaryOp::Log,
        UnaryOp::Sin,
        UnaryOp::Cos,
    ] {
        let mut g = FusionGraph::new(1);
        let x = g.input(0).unwrap();
        let y = g.unary(op, &x).unwrap();
        let k = g.compile(&c, &[y]).unwrap();
        let mut p = rt.program();
        let out = p.fused(&k, &[&input], values.len()).unwrap();
        let expected: Vec<_> = values
            .iter()
            .map(|&x| match op {
                UnaryOp::Negate => -x,
                UnaryOp::Abs => x.abs(),
                UnaryOp::Square => x * x,
                UnaryOp::Sqrt => x.sqrt(),
                UnaryOp::Reciprocal => 1.0 / x,
                UnaryOp::Exp => x.exp(),
                UnaryOp::Log => x.ln(),
                UnaryOp::Sin => x.sin(),
                UnaryOp::Cos => x.cos(),
            })
            .collect();
        near(
            &p.submit_read(&out[0]).unwrap().wait(TIMEOUT).unwrap(),
            &expected,
        );
    }
    for op in [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Min,
        BinaryOp::Max,
    ] {
        for swap in [false, true] {
            let mut g = FusionGraph::new(2);
            let a = g.input(0).unwrap();
            let b = g.input(1).unwrap();
            let y = g.binary(op, &a, &b).unwrap();
            let k = g.compile(&c, &[y]).unwrap();
            let mut p = rt.program();
            let args = if swap {
                [&scalar, &input]
            } else {
                [&input, &scalar]
            };
            let out = p.fused(&k, &args, values.len()).unwrap();
            let expected: Vec<_> = values
                .iter()
                .map(|&v| {
                    let (a, b) = if swap { (0.5, v) } else { (v, 0.5) };
                    match op {
                        BinaryOp::Add => a + b,
                        BinaryOp::Subtract => a - b,
                        BinaryOp::Multiply => a * b,
                        BinaryOp::Divide => a / b,
                        BinaryOp::Min => a.min(b),
                        BinaryOp::Max => a.max(b),
                    }
                })
                .collect();
            near(
                &p.submit_read(&out[0]).unwrap().wait(TIMEOUT).unwrap(),
                &expected,
            );
        }
    }
}
#[test]
fn constant_only_empty_and_scalar_expressions_have_defined_results() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let mut g = FusionGraph::new(0);
    let x = g.constant(-0.0).unwrap();
    let y = g.constant(1.25).unwrap();
    let y = g.affine(&y, 2.0, 0.5).unwrap();
    let k = g.compile(&c, &[x, y]).unwrap();
    for n in [0, 1, 259] {
        let mut p = rt.program();
        let out = p.fused(&k, &[], n).unwrap();
        let got = p.submit_read(&out[0]).unwrap().wait(TIMEOUT).unwrap();
        assert!(got.iter().all(|v| v.to_bits() == (-0.0f32).to_bits()));
        assert_eq!(
            rt.read(&out[1]).unwrap().wait(TIMEOUT).unwrap(),
            vec![3.0; n]
        );
    }
}
#[test]
fn invalid_binding_calls_leave_the_prepared_program_usable() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let other = ComputeRuntime::new(&c).unwrap();
    let mut g = FusionGraph::new(1);
    let x = g.input(0).unwrap();
    let y = g.affine(&x, 2.0, 1.0).unwrap();
    let k = g.compile(&c, &[y.clone(), y]).unwrap();
    let a = rt.upload(&[1.0f32, 2.0, 3.0]).unwrap();
    let b = rt.zeros::<f32>(3).unwrap();
    let short = rt.zeros::<f32>(2).unwrap();
    let foreign = other.zeros::<f32>(3).unwrap();
    let mut p = rt.program();
    assert!(matches!(
        p.fused(&k, &[], 3),
        Err(FusionError::InputCount { .. })
    ));
    assert!(matches!(
        p.fused_into(&k, &[&a], &[]),
        Err(FusionError::OutputCount { .. })
    ));
    assert!(matches!(
        p.fused_into(&k, &[&a], &[&b, &b]),
        Err(FusionError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.fused_into(&k, &[&a], &[&a, &b]),
        Err(FusionError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.fused_into(&k, &[&a], &[&b, &short]),
        Err(FusionError::Compute(ComputeError::LengthMismatch { .. }))
    ));
    assert!(matches!(
        p.fused_into(&k, &[&a], &[&b, &foreign]),
        Err(FusionError::Compute(ComputeError::ForeignArray))
    ));
    assert!(matches!(
        p.fused(&k, &[&foreign], 3),
        Err(FusionError::Compute(ComputeError::ForeignArray))
    ));
    assert!(matches!(
        p.fused(&k, &[&short], 3),
        Err(FusionError::Compute(ComputeError::LengthMismatch { .. }))
    ));
    let out = p.fused(&k, &[&a], 3).unwrap();
    // Reduction consumes the fused output in the same compute pass.
    let sum = p.sum(&out[0]).unwrap();
    assert_eq!(p.submit_read(&sum).unwrap().wait(TIMEOUT).unwrap(), [15.0]);
    assert_eq!(rt.read(&b).unwrap().wait(TIMEOUT).unwrap(), [0.0; 3]);
}

#[test]
fn compilation_and_execution_reject_wrong_graph_or_device_and_binding_overflow() {
    let Some(c) = context() else { return };
    let mut g = FusionGraph::new(0);
    let constant = g.constant(2.0).unwrap();
    assert!(matches!(g.compile(&c, &[]), Err(FusionError::NoOutputs)));
    let mut other_graph = FusionGraph::new(0);
    let foreign = other_graph.constant(3.0).unwrap();
    assert!(matches!(
        g.compile(&c, &[foreign]),
        Err(FusionError::ForeignExpression)
    ));
    let too_many =
        vec![constant.clone(); c.device.limits().max_storage_buffers_per_shader_stage as usize + 1];
    assert!(matches!(
        g.compile(&c, &too_many),
        Err(FusionError::BindingLimit { .. })
    ));
    let k = g.compile(&c, &[constant]).unwrap();
    let Some(other_c) = context() else { return };
    let foreign_rt = ComputeRuntime::new(&other_c).unwrap();
    assert!(matches!(
        foreign_rt.program().fused(&k, &[], 1),
        Err(FusionError::ForeignDevice)
    ));
}

#[test]
fn grid_stride_writes_tail_above_the_workgroup_dispatch_limit() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let mut g = FusionGraph::new(0);
    let constant = g.constant(2.0).unwrap();
    let k = g.compile(&c, &[constant]).unwrap();
    let n = 65535 * 256 + 7;
    let mut p = rt.program();
    let out = p.fused(&k, &[], n).unwrap();
    let actual = p.submit_read(&out[0]).unwrap().wait(TIMEOUT).unwrap();
    assert_eq!(actual.len(), n);
    assert!(actual.iter().all(|v| *v == 2.0));
}
