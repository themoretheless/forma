use compute_core::{
    BinaryOp, CompareOp, ComputeError, ComputeRuntime, FusionError, FusionGraph, KernelCache,
    UnaryOp, gpu_compute::GpuContext,
};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(20);
fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    assert!(
        context.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "GPU required"
    );
    context
}
fn near(actual: f32, expected: f64, magnitude: f64) {
    assert!(
        (f64::from(actual) - expected).abs() <= 2e-6 * magnitude.max(1.0),
        "{actual} != {expected}"
    );
}
#[test]
fn fused_map_sum_matches_f64_across_empty_partial_and_large_inputs() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let mut g = FusionGraph::new(2);
    let x = g.input(0).unwrap();
    let y = g.input(1).unwrap();
    let product = g.binary(BinaryOp::Multiply, &x, &y).unwrap();
    let product = g.affine(&product, 0.25, 1.0).unwrap();
    let k = g.compile_sum(&c, &product).unwrap();
    assert_eq!(k.input_count(), 2);
    assert_eq!(k.operation_count(), 2);
    for n in [0, 1, 255, 256, 257, 2048, 2049, 1_000_003] {
        let values: Vec<f32> = (0..n).map(|i| ((i % 31) as f32 - 15.0) * 0.125).collect();
        let a = rt.upload(&values).unwrap();
        let b = rt.upload(&[2.0f32]).unwrap();
        let mut p = rt.program();
        let output = p.fused_sum(&k, &[&a, &b], n).unwrap();
        let consumed = p.affine(&output, 0.5, 2.0).unwrap();
        let got = p.submit_read(&consumed).unwrap().wait(TIMEOUT).unwrap()[0];
        let expected = values
            .iter()
            .map(|&x| f64::from(x) * 0.5 + 1.0)
            .sum::<f64>()
            * 0.5
            + 2.0;
        near(got, expected, n as f64 * 2.0 + 2.0);
    }
}
#[test]
fn predicates_select_values_and_fuse_conditional_sums_for_all_comparisons() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let values: Vec<f32> = (0..1031).map(|i| ((i % 7) as f32 - 3.0) * 0.5).collect();
    let input = rt.upload(&values).unwrap();
    let threshold = rt.upload(&[0.5f32]).unwrap();
    for op in [
        CompareOp::Equal,
        CompareOp::NotEqual,
        CompareOp::Less,
        CompareOp::LessEqual,
        CompareOp::Greater,
        CompareOp::GreaterEqual,
    ] {
        let mut g = FusionGraph::new(2);
        let x = g.input(0).unwrap();
        let t = g.input(1).unwrap();
        let condition = g.compare(op, &x, &t).unwrap();
        let positive = g.unary(UnaryOp::Square, &x).unwrap();
        let zero = g.constant(0.0).unwrap();
        let value = g.select(&condition, &positive, &zero).unwrap();
        let elementwise = g.compile(&c, std::slice::from_ref(&value)).unwrap();
        let sum_kernel = g.compile_sum(&c, &value).unwrap();
        let mut p = rt.program();
        let map = p
            .fused(&elementwise, &[&input, &threshold], values.len())
            .unwrap();
        let total = p
            .fused_sum(&sum_kernel, &[&input, &threshold], values.len())
            .unwrap();
        let expected: Vec<f32> = values
            .iter()
            .map(|&x| {
                let selected = match op {
                    CompareOp::Equal => x == 0.5,
                    CompareOp::NotEqual => x != 0.5,
                    CompareOp::Less => x < 0.5,
                    CompareOp::LessEqual => x <= 0.5,
                    CompareOp::Greater => x > 0.5,
                    CompareOp::GreaterEqual => x >= 0.5,
                };
                if selected { x * x } else { 0.0 }
            })
            .collect();
        assert_eq!(
            p.submit_read(&map[0]).unwrap().wait(TIMEOUT).unwrap(),
            expected
        );
        assert_eq!(
            rt.read(&total).unwrap().wait(TIMEOUT).unwrap(),
            [expected.iter().sum::<f32>()]
        );
    }
}
#[test]
fn constant_and_broadcast_sums_reuse_outputs_and_reset_empty_results() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let mut g = FusionGraph::new(0);
    let value = g.constant(1.25).unwrap();
    let mut cache = KernelCache::new(&c, 2);
    let k = g.compile_sum_cached(&mut cache, &value).unwrap();
    let same = g.compile_sum_cached(&mut cache, &value).unwrap();
    let mapped = g.compile_cached(&mut cache, &[value]).unwrap();
    assert_eq!(cache.stats().hits, 1);
    assert_ne!(k.source(), mapped.source());
    assert_eq!(k.source(), same.source());
    cache.clear();
    let destination = rt.upload(&[99.0f32]).unwrap();
    for n in [4097, 0, 1] {
        let mut p = rt.program();
        p.fused_sum_into(&k, &[], n, &destination).unwrap();
        assert_eq!(
            p.submit_read(&destination).unwrap().wait(TIMEOUT).unwrap(),
            [n as f32 * 1.25]
        );
    }
    let mut g = FusionGraph::new(1);
    let x = g.input(0).unwrap();
    let k = g.compile_sum(&c, &x).unwrap();
    let scalar = rt.upload(&[0.5f32]).unwrap();
    let mut p = rt.program();
    p.fused_sum_into(&k, &[&scalar], 8193, &destination)
        .unwrap();
    let first = p.submit_read(&destination).unwrap();
    rt.write(&scalar, 0, &[0.25]).unwrap();
    let second = p.submit_read(&destination).unwrap();
    assert_eq!(second.wait(TIMEOUT).unwrap(), [8193.0 * 0.25]);
    assert_eq!(first.wait(TIMEOUT).unwrap(), [8193.0 * 0.5]);
}
#[test]
fn invalid_sum_bindings_and_predicates_do_not_append_work() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let other_rt = ComputeRuntime::new(&c).unwrap();
    let mut g = FusionGraph::new(1);
    let x = g.input(0).unwrap();
    let local = g.compare(CompareOp::Equal, &x, &x).unwrap();
    let mut other = FusionGraph::new(1);
    let foreign_x = other.input(0).unwrap();
    let foreign = other
        .compare(CompareOp::Equal, &foreign_x, &foreign_x)
        .unwrap();
    assert!(matches!(
        g.compare(CompareOp::Less, &x, &foreign_x),
        Err(FusionError::ForeignExpression)
    ));
    assert!(matches!(
        g.select(&foreign, &x, &x),
        Err(FusionError::ForeignExpression)
    ));
    assert!(matches!(
        g.select(&local, &foreign_x, &x),
        Err(FusionError::ForeignExpression)
    ));
    let k = g.compile_sum(&c, &x).unwrap();
    let input = rt.upload(&[2.0f32]).unwrap();
    let bad_output = rt.upload(&[99.0f32, 99.0]).unwrap();
    let foreign_output = other_rt.zeros(1).unwrap();
    let mut p = rt.program();
    assert!(matches!(
        p.fused_sum_into(&k, &[&input], 1, &input),
        Err(FusionError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.fused_sum_into(&k, &[&input], 1, &bad_output),
        Err(FusionError::Compute(ComputeError::LengthMismatch { .. }))
    ));
    assert!(matches!(
        p.fused_sum_into(&k, &[&input], 1, &foreign_output),
        Err(FusionError::Compute(ComputeError::ForeignArray))
    ));
    assert!(matches!(
        p.fused_sum(&k, &[&input], usize::MAX),
        Err(FusionError::Compute(ComputeError::TooLarge { .. }))
    ));
    let sum = p.fused_sum(&k, &[&input], 17).unwrap();
    assert_eq!(p.submit_read(&sum).unwrap().wait(TIMEOUT).unwrap(), [34.0]);
    assert_eq!(
        rt.read(&bad_output).unwrap().wait(TIMEOUT).unwrap(),
        [99.0; 2]
    );
}
