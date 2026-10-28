#![cfg(feature = "gpu")]
use compute_core::{ComputeError, ComputeRuntime, GpuArray, GpuElement};
use gpu_compute::{BufferError, GpuBuffer, GpuContext, wgpu};
use math_compute::gpu::{
    GpuMathError, GpuPointCloudStats, MathGpuProgram, MathGpuSession, PointCloudView,
};
use math_core::{ID, V3};
use std::time::Duration;

fn context() -> Option<GpuContext> {
    let result = GpuContext::new();
    assert!(
        result.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "GPU required"
    );
    result
}
fn flatten(points: &[V3]) -> Vec<f32> {
    points.iter().flatten().map(|&v| v as f32).collect()
}
fn points(count: usize) -> Vec<V3> {
    (0..count)
        .map(|i| {
            [
                10.0 + (i % 37) as f64 * 0.25,
                -20.0 + (i % 19) as f64 * 0.5,
                3.0 - (i % 11) as f64 * 0.125,
            ]
        })
        .collect()
}
fn read<T: GpuElement>(
    context: &GpuContext,
    runtime: &ComputeRuntime,
    plan: &MathGpuProgram<'_>,
    output: &GpuArray<T>,
) -> Vec<T> {
    let mut encoder = context.device.create_command_encoder(&Default::default());
    plan.record(&mut encoder);
    let mut ticket = runtime.record_read(&mut encoder, output).unwrap();
    ticket.submitted(context.queue.submit([encoder.finish()]));
    ticket.wait(Duration::from_secs(10)).unwrap()
}
fn close(actual: &[f32], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &b)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (a as f64 - b).abs() <= 3e-4 * b.abs().max(1.0),
            "slot {i}: {a} != {b}"
        );
    }
}
fn expected_stats(points: &[V3]) -> Vec<f64> {
    let reference = math_core::point_cloud_stats(points).unwrap();
    let mut result = vec![0.; GpuPointCloudStats::LEN];
    result[GpuPointCloudStats::MIN].copy_from_slice(&reference.bounds.min);
    result[GpuPointCloudStats::MAX].copy_from_slice(&reference.bounds.max);
    let n = points.len() as f64;
    result[GpuPointCloudStats::SUM].copy_from_slice(&reference.moments.centroid.map(|v| v * n));
    result[GpuPointCloudStats::CENTROID].copy_from_slice(&reference.moments.centroid);
    for (k, (a, b)) in [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 2)]
        .into_iter()
        .enumerate()
    {
        result[GpuPointCloudStats::OUTER_SUM.start + k] = reference.moments.second_moment[a][b] * n;
        result[GpuPointCloudStats::COVARIANCE.start + k] = reference.moments.covariance[a][b];
    }
    result
}

#[test]
fn recorded_neighbors_compose_with_transform_sum_and_generic_compute() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    // Shared cloned context and another runtime are valid domain inputs.
    let shared_runtime = ComputeRuntime::new(&context.clone()).unwrap();
    let math = MathGpuSession::new(&context.clone());
    let queries = points(517);
    let targets = points(23);
    let q = shared_runtime.upload(&flatten(&queries)).unwrap();
    let t = runtime.upload(&flatten(&targets)).unwrap();
    let mut plan = math.program(&runtime).unwrap();
    let moved = plan
        .transform(PointCloudView::new(&q).unwrap(), ID, [0.5, 0.25, -0.125])
        .unwrap();
    let found = plan
        .nearest_neighbors(
            PointCloudView::new(&moved).unwrap(),
            PointCloudView::new(&t).unwrap(),
        )
        .unwrap();
    let sum = plan.sum(&found.squared_distances).unwrap();
    let mut generic = runtime.program();
    let average = generic
        .affine(&sum, 1.0 / queries.len() as f32, 0.)
        .unwrap();
    let moved_cpu: Vec<V3> = queries
        .iter()
        .map(|p| [p[0] + 0.5, p[1] + 0.25, p[2] - 0.125])
        .collect();
    let expected = math_core::nearest_neighbor(&moved_cpu, &targets);
    for shift in [0.0, 0.5] {
        let updated: Vec<V3> = queries.iter().map(|p| [p[0] + shift, p[1], p[2]]).collect();
        shared_runtime.write(&q, 0, &flatten(&updated)).unwrap();
        let updated_moved: Vec<V3> = updated
            .iter()
            .map(|p| [p[0] + 0.5, p[1] + 0.25, p[2] - 0.125])
            .collect();
        let expected = if shift == 0. {
            expected.clone()
        } else {
            math_core::nearest_neighbor(&updated_moved, &targets)
        };
        let mut encoder = context.device.create_command_encoder(&Default::default());
        plan.record(&mut encoder);
        generic.record(&mut encoder);
        let mut indices = runtime.record_read(&mut encoder, &found.indices).unwrap();
        let mut distances = runtime
            .record_read(&mut encoder, &found.squared_distances)
            .unwrap();
        let mut mean = runtime.record_read(&mut encoder, &average).unwrap();
        let submission = context.queue.submit([encoder.finish()]);
        indices.submitted(submission.clone());
        distances.submitted(submission.clone());
        mean.submitted(submission);
        assert_eq!(
            indices.wait(Duration::from_secs(10)).unwrap(),
            expected.iter().map(|p| p.0).collect::<Vec<_>>()
        );
        close(
            &distances.wait(Duration::from_secs(10)).unwrap(),
            &expected.iter().map(|p| p.1).collect::<Vec<_>>(),
        );
        close(
            &mean.wait(Duration::from_secs(10)).unwrap(),
            &[expected.iter().map(|p| p.1).sum::<f64>() / queries.len() as f64],
        );
    }
}

#[test]
fn statistics_cover_single_odd_and_hierarchical_clouds_and_reuse_outputs() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let math = MathGpuSession::new(&context);
    for count in [1, 257, 65_537] {
        let source = points(count);
        let input = runtime.upload(&flatten(&source)).unwrap();
        let mut plan = math.program(&runtime).unwrap();
        let stats = plan
            .point_cloud_stats(PointCloudView::new(&input).unwrap())
            .unwrap();
        assert_eq!(stats.samples, count);
        close(
            &read(&context, &runtime, &plan, &stats.values),
            &expected_stats(&source),
        );
        let shifted: Vec<V3> = source
            .iter()
            .map(|p| [p[0] + 0.25, p[1] - 0.5, p[2] + 0.125])
            .collect();
        runtime.write(&input, 0, &flatten(&shifted)).unwrap();
        close(
            &read(&context, &runtime, &plan, &stats.values),
            &expected_stats(&shifted),
        );
        let reusable = runtime.zeros::<f32>(GpuPointCloudStats::LEN).unwrap();
        let mut reuse = math.program(&runtime).unwrap();
        reuse
            .point_cloud_stats_into(PointCloudView::new(&input).unwrap(), &reusable)
            .unwrap();
        close(
            &read(&context, &runtime, &reuse, &reusable),
            &expected_stats(&shifted),
        );
    }
}

#[test]
fn empty_neighbor_targets_and_ties_have_explicit_sentinels() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let math = MathGpuSession::new(&context);
    let queries = runtime.upload(&[0.0f32, 0., 0., 0., 0., 0.]).unwrap();
    let targets = runtime.upload(&[-1.0f32, 0., 0., 1., 0., 0.]).unwrap();
    let empty = runtime.zeros::<f32>(0).unwrap();
    let mut plan = math.program(&runtime).unwrap();
    let found = plan
        .nearest_neighbors(
            PointCloudView::new(&queries).unwrap(),
            PointCloudView::new(&targets).unwrap(),
        )
        .unwrap();
    assert_eq!(read(&context, &runtime, &plan, &found.indices), [0, 0]);
    let missing = plan
        .nearest_neighbors(
            PointCloudView::new(&queries).unwrap(),
            PointCloudView::new(&empty).unwrap(),
        )
        .unwrap();
    assert_eq!(
        read(&context, &runtime, &plan, &missing.indices),
        [u32::MAX; 2]
    );
    assert_eq!(
        read(&context, &runtime, &plan, &missing.squared_distances),
        [f32::MAX; 2]
    );
    let no_queries = plan
        .nearest_neighbors(
            PointCloudView::new(&empty).unwrap(),
            PointCloudView::new(&targets).unwrap(),
        )
        .unwrap();
    assert!(read(&context, &runtime, &plan, &no_queries.indices).is_empty());
    assert!(
        plan.point_cloud_stats(PointCloudView::new(&empty).unwrap())
            .is_err()
    );
}

#[test]
fn domain_into_rejects_foreign_wrong_lengths_and_aliases_before_recording() {
    let Some(context) = context() else { return };
    let Some(other) = self::context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let foreign_runtime = ComputeRuntime::new(&other).unwrap();
    let math = MathGpuSession::new(&context);
    let points = runtime.upload(&flatten(&points(8))).unwrap();
    let foreign = foreign_runtime.zeros::<f32>(24).unwrap();
    let wrong_length = runtime.upload(&[91.0f32; 23]).unwrap();
    let output = runtime.upload(&[73.0f32; 24]).unwrap();
    let mut plan = math.program(&runtime).unwrap();
    let view = PointCloudView::new(&points).unwrap();
    assert!(matches!(
        plan.point_cloud_stats_into(view, &points),
        Err(GpuMathError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        plan.point_cloud_stats_into(view, &wrong_length),
        Err(GpuMathError::Compute(ComputeError::LengthMismatch { .. }))
    ));
    assert!(matches!(
        plan.point_cloud_stats_into(view, &foreign),
        Err(GpuMathError::Buffer(BufferError::ForeignDevice))
    ));
    assert!(matches!(
        plan.point_cloud_stats(PointCloudView::new(&foreign).unwrap()),
        Err(GpuMathError::Buffer(BufferError::ForeignDevice))
    ));
    let shared = GpuBuffer::new(
        &context,
        32,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    let indices = runtime.import_buffer::<u32>(shared.clone(), 8).unwrap();
    let distances = runtime.import_buffer::<f32>(shared, 8).unwrap();
    assert!(matches!(
        plan.nearest_neighbors_into(view, view, &indices, &distances),
        Err(GpuMathError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        plan.nearest_neighbors_into(view, view, &indices, &points.prefix(8).unwrap()),
        Err(GpuMathError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        plan.nearest_neighbors(view, PointCloudView::new(&foreign).unwrap()),
        Err(GpuMathError::Buffer(BufferError::ForeignDevice))
    ));
    assert_eq!(read(&context, &runtime, &plan, &output), [73.; 24]);
    assert_eq!(read(&context, &runtime, &plan, &wrong_length), [91.; 23]);
    plan.point_cloud_stats_into(view, &output).unwrap();
    close(
        &read(&context, &runtime, &plan, &output),
        &expected_stats(&self::points(8)),
    );
}

#[test]
fn nearest_inlier_selection_and_sum_stay_on_gpu_across_repeated_submissions() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let session = MathGpuSession::new(&context);
    let queries = runtime
        .upload(&[
            0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 3.0, 0.0, 0.0, 8.0, 0.0, 0.0,
        ])
        .unwrap();
    let targets = runtime.upload(&[0.0f32, 0.0, 0.0, 4.0, 0.0, 0.0]).unwrap();
    let threshold = runtime.upload(&[2.0f32]).unwrap();
    let mut math = session.program(&runtime).unwrap();
    let nearest = math
        .nearest_neighbors(
            PointCloudView::new(&queries).unwrap(),
            PointCloudView::new(&targets).unwrap(),
        )
        .unwrap();
    let mut compute = runtime.program();
    let keep = compute
        .compare(
            compute_core::CompareOp::LessEqual,
            &nearest.squared_distances,
            &threshold,
        )
        .unwrap();
    let selected = compute.compact(&nearest.squared_distances, &keep).unwrap();
    let sum = compute.sum(selected.values()).unwrap();
    for (threshold_value, expected_count, expected_sum) in
        [(2.0, 3, 2.0), (0.0, 1, 0.0), (20.0, 4, 18.0)]
    {
        runtime.write(&threshold, 0, &[threshold_value]).unwrap();
        let mut encoder = context.device.create_command_encoder(&Default::default());
        math.record(&mut encoder);
        compute.record(&mut encoder);
        let mut count_read = runtime.record_read(&mut encoder, selected.count()).unwrap();
        let mut sum_read = runtime.record_read(&mut encoder, &sum).unwrap();
        let index = context.queue.submit([encoder.finish()]);
        count_read.submitted(index.clone());
        sum_read.submitted(index);
        assert_eq!(
            count_read.wait(Duration::from_secs(10)).unwrap(),
            [expected_count]
        );
        assert_eq!(
            sum_read.wait(Duration::from_secs(10)).unwrap(),
            [expected_sum]
        );
    }
}
