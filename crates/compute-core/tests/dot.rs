#![recursion_limit = "256"]
use compute_core::{ComputeError, ComputeRuntime, gpu_compute::GpuContext};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(20);
fn context() -> Option<GpuContext> {
    let result = GpuContext::new();
    assert!(
        result.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "GPU required"
    );
    result
}
fn reference(a: &[f32], b: &[f32]) -> (f64, f64) {
    a.iter()
        .zip(b)
        .map(|(&a, &b)| f64::from(a) * f64::from(b))
        .fold((0.0, 0.0), |(sum, scale), x| (sum + x, scale + x.abs()))
}
fn check(actual: f32, expected: (f64, f64)) {
    assert!(
        (f64::from(actual) - expected.0).abs() <= 3e-6 * expected.1.max(1.0),
        "{actual} != {} (L1 {})",
        expected.0,
        expected.1
    );
}
#[test]
fn fused_dot_preserves_runtime_send_sync() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<ComputeRuntime>();
}
#[test]
fn dot_matches_f64_for_tails_large_inputs_signs_and_dynamic_range() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    for n in [0, 1, 63, 64, 65, 255, 256, 257, 2047, 2048, 2049, 1_000_003] {
        let a: Vec<f32> = (0..n).map(|i| ((i % 37) as f32 - 18.0) * 0.125).collect();
        let b: Vec<f32> = (0..n).map(|i| ((i % 19) as f32 - 9.0) * 0.25).collect();
        let ga = rt.upload(&a).unwrap();
        let gb = rt.upload(&b).unwrap();
        let mut p = rt.program();
        let dot = p.dot(&ga, &gb).unwrap();
        check(
            p.submit_read(&dot).unwrap().wait(TIMEOUT).unwrap()[0],
            reference(&a, &b),
        );
    }
    let a: Vec<f32> = (0_usize..65_539)
        .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 } * 2.0f32.powi((i % 41) as i32 - 20))
        .collect();
    let b: Vec<f32> = (0..a.len())
        .map(|i| ((i % 23) + 1) as f32 * 0.03125)
        .collect();
    let ga = rt.upload(&a).unwrap();
    let gb = rt.upload(&b).unwrap();
    let mut p = rt.program();
    let dot = p.dot(&ga, &gb).unwrap();
    check(
        p.submit_read(&dot).unwrap().wait(TIMEOUT).unwrap()[0],
        reference(&a, &b),
    );
}
#[test]
fn dot_into_reuses_scalar_prefix_and_preserves_queued_results() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let n = 8193;
    let input = rt.zeros::<f32>(n).unwrap();
    let storage = rt.upload(&[99.0f32, 71.0, 72.0, 73.0]).unwrap();
    let output = storage.prefix(1).unwrap();
    let mut p = rt.program();
    p.dot_into(&input, &input, &output).unwrap();
    let consumed = p.affine(&output, 0.5, 1.0).unwrap();
    let mut reads = Vec::new();
    for iteration in 0..3 {
        let values: Vec<f32> = (0..n)
            .map(|i| ((i + iteration) % 11) as f32 * 0.25)
            .collect();
        rt.write(&input, 0, &values).unwrap();
        let expected = reference(&values, &values).0 * 0.5 + 1.0;
        reads.push((p.submit_read(&consumed).unwrap(), expected));
    }
    for (read, expected) in reads.into_iter().rev() {
        check(read.wait(TIMEOUT).unwrap()[0], (expected, expected));
    }
    assert_eq!(
        &rt.read(&storage).unwrap().wait(TIMEOUT).unwrap()[1..],
        &[71.0, 72.0, 73.0]
    );
    let empty = rt.zeros::<f32>(0).unwrap();
    let mut p = rt.program();
    p.dot_into(&empty, &empty, &output).unwrap();
    assert_eq!(
        p.submit_read(&output).unwrap().wait(TIMEOUT).unwrap(),
        [0.0]
    );
}
#[test]
fn dot_validation_is_atomic_and_scalar_broadcast_is_not_implicit() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let foreign_rt = ComputeRuntime::new(&c).unwrap();
    let a = rt.upload(&[1.0f32, 2.0]).unwrap();
    let scalar = rt.upload(&[3.0f32]).unwrap();
    let foreign = foreign_rt.zeros::<f32>(2).unwrap();
    let out = rt.upload(&[99.0f32]).unwrap();
    let mut p = rt.program();
    assert!(matches!(
        p.dot(&a, &scalar),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        p.dot(&a, &foreign),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        p.dot_into(&a, &a, &a),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        p.dot_into(&scalar, &scalar, &scalar),
        Err(ComputeError::AliasedOutput)
    ));
    assert!(matches!(
        p.dot_into(&a, &foreign, &out),
        Err(ComputeError::ForeignArray)
    ));
    let dot = p.dot(&a, &a).unwrap();
    assert_eq!(p.submit_read(&dot).unwrap().wait(TIMEOUT).unwrap(), [5.0]);
    assert_eq!(rt.read(&out).unwrap().wait(TIMEOUT).unwrap(), [99.0]);
}
