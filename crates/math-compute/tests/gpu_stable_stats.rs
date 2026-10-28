#![cfg(feature = "gpu")]
use compute_core::{ComputeError, ComputeRuntime, GpuArray};
use gpu_compute::{BufferError, GpuContext};
use math_compute::gpu::{
    GpuArithmetic, GpuMathError, GpuPointCloudStats, MathGpuProgram, MathGpuSession, PointCloudView,
};
use math_core::{ID, V3};
use std::time::Duration;

const PAIRS: [(usize, usize); 6] = [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 2)];
fn gpu_context() -> Option<GpuContext> {
    let context = GpuContext::new();
    assert!(
        context.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "GPU required"
    );
    context
}
fn cloud(n: usize, offset: f32) -> Vec<f32> {
    (0..n)
        .flat_map(|i| {
            let x = ((i % 17) as f32 - 8.) * 0.25;
            let y = (((i * 7) % 13) as f32 - 6.) * 0.5;
            [
                offset + x,
                -2. * offset + y,
                0.5 * offset + x * 0.5 - y * 0.25,
            ]
        })
        .collect()
}
fn reference(input: &[f32]) -> math_core::PointCloudStats {
    let points: Vec<V3> = input
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| p.map(f64::from))
        .collect();
    math_core::point_cloud_stats(&points).unwrap()
}
fn read(runtime: &ComputeRuntime, plan: &MathGpuProgram<'_>, output: &GpuArray<f32>) -> Vec<f32> {
    let context = runtime.context();
    let mut encoder = context.device.create_command_encoder(&Default::default());
    plan.record(&mut encoder);
    let mut ticket = runtime.record_read(&mut encoder, output).unwrap();
    ticket.submitted(context.queue.submit([encoder.finish()]));
    ticket.wait(Duration::from_secs(10)).unwrap()
}
fn check_covariance(packed: &[f32], input: &[f32]) {
    let expected = reference(input).moments.covariance;
    let scale = expected
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(1., f64::max);
    let tolerance = 5e-6 * scale;
    let mut covariance = [[0.; 3]; 3];
    for (k, &(a, b)) in PAIRS.iter().enumerate() {
        let value = f64::from(packed[GpuPointCloudStats::COVARIANCE.start + k]);
        assert!(
            (value - expected[a][b]).abs() <= tolerance,
            "field {k}: {value} != {}",
            expected[a][b]
        );
        covariance[a][b] = value;
        covariance[b][a] = value;
    }
    // Check the eigenvalues (including the planar fixture's null space), then
    // exercise additional quadratic forms of the symmetric reconstruction.
    assert_eq!(covariance, math_core::tr(covariance));
    let (eigenvalues, _) = math_core::eigen(covariance);
    assert!(eigenvalues.into_iter().all(|v| v >= -tolerance));
    for x in -2..=2 {
        for y in -2..=2 {
            for z in -2..=2 {
                let v = [f64::from(x), f64::from(y), f64::from(z)];
                let quadratic = math_core::dot(v, math_core::mv(covariance, v));
                assert!(
                    quadratic >= -tolerance * math_core::dot(v, v),
                    "negative quadratic {quadratic}"
                );
            }
        }
    }
}

#[test]
fn centered_stats_preserve_translated_covariance_and_raw_packed_fields() {
    let Some(context) = gpu_context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let math = MathGpuSession::new(&context.clone());
    for count in [1, 2, 257, 65_537] {
        let mut unshifted = None::<Vec<f32>>;
        for offset in [0., 10_000., 1_000_000.] {
            let source = cloud(count, offset);
            let input = runtime.upload(&source).unwrap();
            let mut legacy = math.program(&runtime).unwrap();
            let old = legacy
                .point_cloud_stats(PointCloudView::new(&input).unwrap())
                .unwrap();
            let mut stable = math.program(&runtime).unwrap();
            let new = stable
                .point_cloud_stats_stable(PointCloudView::new(&input).unwrap())
                .unwrap();
            assert_eq!(new.samples, count);
            let old = read(&runtime, &legacy, &old.values);
            let new = read(&runtime, &stable, &new.values);
            assert_eq!(&old[..18], &new[..18], "raw moments ABI changed");
            check_covariance(&new, &source);
            if let Some(unshifted) = &unshifted {
                for (&a, &b) in new[18..].iter().zip(&unshifted[18..]) {
                    assert!(
                        (a - b).abs() < 2e-5,
                        "translation changed covariance: {a} {b}"
                    );
                }
            } else {
                unshifted = Some(new);
            }
        }
    }
    // At a large common offset the raw subtraction loses all small variance.
    let input = runtime
        .upload(&[
            999_999f32, 999_999., 999_999., 1_000_001., 1_000_001., 1_000_001.,
        ])
        .unwrap();
    let mut plan = math.program(&runtime).unwrap();
    let old = plan
        .point_cloud_stats(PointCloudView::new(&input).unwrap())
        .unwrap();
    let new = plan
        .point_cloud_stats_stable(PointCloudView::new(&input).unwrap())
        .unwrap();
    let legacy = read(&runtime, &plan, &old.values);
    let stable = read(&runtime, &plan, &new.values);
    assert!((legacy[18] - 1.).abs() > 1.);
    for &value in &stable[18..24] {
        assert_eq!(value, 1.);
    }
}

#[test]
fn centered_stats_correct_rounded_mean_and_reuse_after_transform_and_upload() {
    let Some(context) = gpu_context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let math = MathGpuSession::new(&context);
    let input = runtime
        .upload(&[0f32, 0., 0., 0.125, 0.125, 0.125, 0.125, 0.125, 0.125])
        .unwrap();
    let output = runtime.zeros::<f32>(24).unwrap();
    let mut plan = math.program(&runtime).unwrap();
    let moved = plan
        .transform(PointCloudView::new(&input).unwrap(), ID, [1_000_000.; 3])
        .unwrap();
    plan.point_cloud_stats_stable_into(PointCloudView::new(&moved).unwrap(), &output)
        .unwrap();
    for (data, residual) in [
        (
            vec![0., 0., 0., 0.125, 0.125, 0.125, 0.125, 0.125, 0.125],
            0.125f32,
        ),
        (
            vec![0., 0., 0., 0.25, 0.25, 0.25, 0.25, 0.25, 0.25],
            0.25f32,
        ),
    ] {
        runtime.write(&input, 0, &data).unwrap();
        let expected: Vec<f32> = data.iter().map(|v| v + 1_000_000.).collect();
        let packed = read(&runtime, &plan, &output);
        check_covariance(&packed, &expected);
        let variance = f64::from(residual).powi(2) * 2. / 9.;
        assert!((f64::from(packed[18]) - variance).abs() < 1e-8);
    }
}

#[test]
fn centered_stats_validate_before_appending_and_accept_shared_context() {
    let Some(context) = gpu_context() else { return };
    let Some(foreign_context) = gpu_context() else {
        return;
    };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let shared = ComputeRuntime::new(&context.clone()).unwrap();
    let foreign = ComputeRuntime::new(&foreign_context).unwrap();
    let math = MathGpuSession::new(&context.clone());
    let source = cloud(8, 10_000.);
    let input = shared.upload(&source).unwrap();
    let output = runtime.zeros::<f32>(24).unwrap();
    let empty = runtime.zeros::<f32>(0).unwrap();
    let wrong = runtime.zeros::<f32>(25).unwrap();
    let alien = foreign.zeros::<f32>(24).unwrap();
    let mut plan = math.program(&runtime).unwrap();
    let view = PointCloudView::new(&input).unwrap();
    plan.point_cloud_stats_stable_into(view, &output).unwrap();
    assert!(matches!(
        plan.point_cloud_stats_stable(PointCloudView::new(&empty).unwrap()),
        Err(GpuMathError::InvalidInput(_))
    ));
    assert!(matches!(
        plan.point_cloud_stats_stable_into(view, &input),
        Err(GpuMathError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        plan.point_cloud_stats_stable_into(view, &wrong),
        Err(GpuMathError::Compute(ComputeError::LengthMismatch { .. }))
    ));
    assert!(matches!(
        plan.point_cloud_stats_stable_into(view, &alien),
        Err(GpuMathError::Buffer(BufferError::ForeignDevice))
    ));
    assert!(matches!(
        plan.point_cloud_stats_stable(PointCloudView::new(&alien).unwrap()),
        Err(GpuMathError::Buffer(BufferError::ForeignDevice))
    ));
    check_covariance(&read(&runtime, &plan, &output), &source);
}

#[test]
fn stable_session_preserves_binary64_coordinates() {
    let Some(context) = gpu_context() else { return };
    let session = MathGpuSession::new(&context);
    for n in [257, 1, 3] {
        let flat = cloud(n, 1_000_000.);
        let points: Vec<V3> = flat
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p.map(f64::from))
            .collect();
        let got = session.try_point_cloud_stats_stable(&points).unwrap();
        assert_eq!(got.arithmetic, GpuArithmetic::SoftwareBinary64);
        assert_eq!(got.value.samples, n);
        assert_eq!(got.value.bounds, reference(&flat).bounds);
        let expected = reference(&flat).moments;
        for (a, b) in PAIRS {
            assert!((got.value.moments.covariance[a][b] - expected.covariance[a][b]).abs() < 2e-5);
            assert!(
                (got.value.moments.second_moment[a][b] - expected.second_moment[a][b]).abs()
                    < 2e-6 * expected.second_moment[a][b].abs().max(1.)
            );
        }
    }
    assert!(session.try_point_cloud_stats_stable(&[]).is_err());
    assert!(
        session
            .try_point_cloud_stats_stable(&[[f64::NAN, 0., 0.]])
            .is_err()
    );
    let lost = [[16_777_216.; 3], [16_777_217.; 3]];
    assert_eq!(
        math_core::point_moments(&lost).unwrap().covariance[0][0],
        0.25
    );
    assert_eq!(
        session
            .try_point_cloud_stats_stable(&lost)
            .unwrap()
            .value
            .moments
            .covariance,
        [[0.25; 3]; 3]
    );
}
