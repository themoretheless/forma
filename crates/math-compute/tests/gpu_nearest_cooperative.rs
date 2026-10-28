#![cfg(feature = "gpu")]
use compute_core::{Binding, ComputeRuntime, Kernel, uniform_f32};
use gpu_compute::{GpuContext, wgpu};
use math_compute::{
    NEAREST_NEIGHBOR_COOPERATIVE_WGSL, NEAREST_NEIGHBOR_WGSL,
    gpu::{MathGpuSession, NearestNeighborAlgorithm, PointCloudView},
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
fn kernel(context: &GpuContext, cooperative: bool) -> Kernel {
    Kernel::with_workgroup_size(
        &context.device,
        "nearest parity",
        if cooperative {
            NEAREST_NEIGHBOR_COOPERATIVE_WGSL
        } else {
            NEAREST_NEIGHBOR_WGSL
        },
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
            Binding::StorageReadWrite,
        ],
        if cooperative { 64 } else { 256 },
    )
    .unwrap()
}
fn raw(
    context: &GpuContext,
    runtime: &ComputeRuntime,
    kernel: &Kernel,
    q: &[f32],
    t: &[f32],
    cooperative: bool,
) -> (Vec<u32>, Vec<f32>) {
    let queries = runtime.upload(q).unwrap();
    let targets = runtime.upload(t).unwrap();
    let n = q.len() / 3;
    let indices = runtime.zeros::<u32>(n).unwrap();
    let distances = runtime.zeros::<f32>(n).unwrap();
    let params = uniform_f32(
        &context.device,
        &context.queue,
        &[
            f32::from_bits(n as u32),
            f32::from_bits((t.len() / 3) as u32),
            0.,
            0.,
        ],
    );
    let bind = kernel.create_bind_group(
        &context.device,
        &[
            &params,
            queries.view().raw(),
            targets.view().raw(),
            indices.view().raw(),
            distances.view().raw(),
        ],
    );
    let mut encoder = context.device.create_command_encoder(&Default::default());
    if n != 0 {
        kernel.record_dispatch(
            &mut encoder,
            &bind,
            if cooperative {
                n as u32
            } else {
                kernel.workgroup_count(n as u32)
            },
        );
    }
    let mut i = runtime.record_read(&mut encoder, &indices).unwrap();
    let mut d = runtime.record_read(&mut encoder, &distances).unwrap();
    let submission = context.queue.submit([encoder.finish()]);
    i.submitted(submission.clone());
    d.submitted(submission);
    (
        i.wait(Duration::from_secs(10)).unwrap(),
        d.wait(Duration::from_secs(10)).unwrap(),
    )
}
fn points(n: usize, phase: usize) -> Vec<f32> {
    (0..n * 3)
        .map(|i| (((i * 109 + phase) * 101) % 1024) as f32 / 16. - 32.)
        .collect()
}

#[test]
fn policy_stays_within_measured_metal_envelope() {
    use NearestNeighborAlgorithm::{Cooperative, Scalar};
    for (n, m) in [(256, 512), (4096, 4096), (16384, 8192)] {
        assert_eq!(
            NearestNeighborAlgorithm::for_shape(wgpu::Backend::Metal, n, m),
            Cooperative
        );
    }
    for (n, m) in [
        (255, 512),
        (256, 511),
        (16385, 8192),
        (4096, 512),
        (256, 8193),
        (usize::MAX, usize::MAX),
    ] {
        assert_eq!(
            NearestNeighborAlgorithm::for_shape(wgpu::Backend::Metal, n, m),
            Scalar
        );
    }
    for backend in [
        wgpu::Backend::Vulkan,
        wgpu::Backend::Dx12,
        wgpu::Backend::Gl,
        wgpu::Backend::BrowserWebGpu,
    ] {
        assert_eq!(
            NearestNeighborAlgorithm::for_shape(backend, 4096, 4096),
            Scalar
        );
    }
}
#[test]
fn cooperative_matches_raw_reference_for_partial_groups_ties_and_overflow() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let baseline = kernel(&context, false);
    let candidate = kernel(&context, true);
    for (n, m) in [
        (0, 0),
        (1, 0),
        (1, 1),
        (17, 63),
        (65, 65),
        (257, 513),
        (256, 512),
        (4096, 4096),
    ] {
        let q = points(n, 7);
        let t = points(m, 19);
        assert_eq!(
            raw(&context, &runtime, &candidate, &q, &t, true),
            raw(&context, &runtime, &baseline, &q, &t, false),
            "{n}x{m}"
        );
    }
    let q = vec![0.; 256 * 3];
    let mut t = vec![1000.; 513 * 3];
    // Equal minima straddle lane, workgroup and stride boundaries.
    for index in [7, 63, 64, 255, 256, 512] {
        t[index * 3] = if index % 2 == 0 { -1. } else { 1. };
        t[index * 3 + 1] = 0.;
        t[index * 3 + 2] = 0.;
    }
    let (indices, distances) = raw(&context, &runtime, &candidate, &q, &t, true);
    assert_eq!(indices, vec![7; 256]);
    assert_eq!(distances, vec![1.; 256]);
    t[7 * 3] = 1. + f32::EPSILON;
    t[63 * 3] = 1. + f32::EPSILON;
    assert_eq!(
        raw(&context, &runtime, &candidate, &q, &t, true),
        raw(&context, &runtime, &baseline, &q, &t, false)
    );
    assert_eq!(
        raw(&context, &runtime, &candidate, &q, &t, true).0,
        vec![64; 256]
    );
    let huge = vec![f32::MAX; 256 * 3];
    assert_eq!(
        raw(
            &context,
            &runtime,
            &candidate,
            &huge,
            &vec![0.; 513 * 3],
            true
        ),
        (vec![u32::MAX; 256], vec![f32::MAX; 256])
    );
}
#[test]
fn recorded_and_synchronous_dispatch_share_results_and_reuse_shape_changes() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let session = MathGpuSession::new(&context);
    let baseline = kernel(&context, false);
    // Start with the optimized shape, shrink into scalar, then reenter the
    // optimized path without growing the synchronous cache allocations.
    for (n, m) in [(257, 513), (17, 65), (256, 512)] {
        let q = points(n, 7);
        let t = points(m, 19);
        let reference = raw(&context, &runtime, &baseline, &q, &t, false);
        let cq: Vec<[f64; 3]> = q
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
            .collect();
        let ct: Vec<[f64; 3]> = t
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
            .collect();
        let expected: Vec<(u32, f64)> = reference
            .0
            .iter()
            .copied()
            .zip(reference.1.iter().map(|&v| v as f64))
            .collect();
        assert_eq!(
            session.try_nearest_neighbor(&cq, &ct).unwrap().value,
            expected
        );
        let queries = runtime.upload(&q).unwrap();
        let targets = runtime.upload(&t).unwrap();
        let mut plan = session.program(&runtime).unwrap();
        let output = plan
            .nearest_neighbors(
                PointCloudView::new(&queries).unwrap(),
                PointCloudView::new(&targets).unwrap(),
            )
            .unwrap();
        for shift in [0., 0.25] {
            let updated: Vec<f32> = q.iter().map(|v| v + shift).collect();
            runtime.write(&queries, 0, &updated).unwrap();
            let mut encoder = context.device.create_command_encoder(&Default::default());
            plan.record(&mut encoder);
            let mut i = runtime.record_read(&mut encoder, &output.indices).unwrap();
            let mut d = runtime
                .record_read(&mut encoder, &output.squared_distances)
                .unwrap();
            let submission = context.queue.submit([encoder.finish()]);
            i.submitted(submission.clone());
            d.submitted(submission);
            assert_eq!(
                (
                    i.wait(Duration::from_secs(10)).unwrap(),
                    d.wait(Duration::from_secs(10)).unwrap()
                ),
                raw(&context, &runtime, &baseline, &updated, &t, false)
            );
        }
    }
}
