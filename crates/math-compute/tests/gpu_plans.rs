#![cfg(feature = "gpu")]
use compute_core::{BinaryOp, ComputeError, ComputeRuntime, ScratchPool};
use gpu_compute::{BufferError, GpuBuffer, GpuContext, wgpu};
use math_compute::gpu::{GpuArithmetic, GpuMathError, MathGpuSession, PointCloudView};
use math_core::ID;
use std::time::Duration;
fn context() -> Option<GpuContext> {
    let c = GpuContext::new();
    assert!(
        c.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "GPU required"
    );
    c
}
#[test]
fn math_and_generic_compute_share_one_submission_and_final_readback() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let math = MathGpuSession::new(&c.clone());
    let points = rt
        .upload(&[0.0f32, 0.0, 0.0, 1.0, 2.0, 3.0, -2.0, 1.0, 4.0])
        .unwrap();
    let mut plan = math.program(&rt).unwrap();
    let transformed = plan
        .transform(PointCloudView::new(&points).unwrap(), ID, [1.0, 2.0, 3.0])
        .unwrap();
    let distances = plan
        .squared_distances(
            PointCloudView::new(&points).unwrap(),
            PointCloudView::new(&transformed).unwrap(),
        )
        .unwrap();
    let sum = plan.sum(&distances).unwrap();
    let mut generic = rt.program();
    let final_output = generic.affine(&sum, 0.5, 1.0).unwrap();
    for _ in 0..3 {
        let mut encoder = c.device.create_command_encoder(&Default::default());
        plan.record(&mut encoder);
        generic.record(&mut encoder);
        let mut read = rt.record_read(&mut encoder, &final_output).unwrap();
        read.submitted(c.queue.submit([encoder.finish()]));
        assert_eq!(read.wait(Duration::from_secs(5)).unwrap(), [22.0]);
    }
    let invalid = rt.zeros::<f32>(4).unwrap();
    assert!(PointCloudView::new(&invalid).is_err());
    let Some(other) = context() else { return };
    assert!(!c.same_device(&other));
    assert!(c.same_device(&c.clone()));
    let other_rt = ComputeRuntime::new(&other).unwrap();
    let foreign_buffer = GpuBuffer::new(
        &other,
        16,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    assert!(matches!(
        rt.import_buffer::<f32>(foreign_buffer, 4),
        Err(ComputeError::Buffer(BufferError::ForeignDevice))
    ));
    assert!(matches!(
        math.program(&other_rt),
        Err(GpuMathError::Buffer(BufferError::ForeignDevice))
    ));
    let foreign = other_rt.zeros::<f32>(3).unwrap();
    assert!(matches!(
        plan.transform(PointCloudView::new(&foreign).unwrap(), ID, [0.; 3]),
        Err(GpuMathError::Buffer(BufferError::ForeignDevice))
    ));
}
#[test]
fn scratch_growth_retains_old_plans_and_reports_generations() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let mut pool = ScratchPool::new(&rt, 128);
    let first = pool.reserve::<f32>("input", 4).unwrap();
    rt.write(&first.array, 0, &[1.0, 2.0, 3.0, 4.0]).unwrap();
    let mut old_plan = rt.program();
    let squared = old_plan
        .binary(BinaryOp::Multiply, &first.array, &first.array)
        .unwrap();
    let old_read = old_plan.submit_read(&squared).unwrap();
    let smaller = pool.reserve::<f32>("input", 2).unwrap();
    assert_eq!(smaller.generation, first.generation);
    assert_eq!(pool.reserved_bytes(), 16);
    let grown = pool.reserve::<f32>("input", 16).unwrap();
    assert!(grown.generation > first.generation);
    rt.write(&grown.array, 0, &[99.; 16]).unwrap();
    assert_eq!(
        old_read.wait(Duration::from_secs(5)).unwrap(),
        [1., 4., 9., 16.]
    );
    assert_eq!(
        old_plan
            .submit_read(&squared)
            .unwrap()
            .wait(Duration::from_secs(5))
            .unwrap(),
        [1., 4., 9., 16.]
    );
    assert!(matches!(
        pool.reserve::<f32>("other", 17),
        Err(ComputeError::BudgetExceeded { .. })
    ));
    assert_eq!(pool.reserved_bytes(), 64);
    let alias = pool.reserve::<f32>("input", 16).unwrap();
    assert!(matches!(
        rt.program()
            .affine_into(&grown.array, 1.0, 0.0, &alias.array),
        Err(ComputeError::AliasedOutput)
    ));
}
#[test]
fn borrowed_buffer_validation_and_execution_reports() {
    let Some(c) = context() else { return };
    let rt = ComputeRuntime::new(&c).unwrap();
    let bytes = GpuBuffer::new(
        &c,
        512,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    assert!(matches!(
        bytes.view(4..16).unwrap().storage_binding(&c),
        Err(BufferError::UnalignedBinding)
    ));
    assert!(bytes.view(0..516).is_err());
    let nonstorage = GpuBuffer::new(&c, 16, wgpu::BufferUsages::COPY_DST).unwrap();
    assert!(matches!(
        rt.import_buffer::<f32>(nonstorage, 4),
        Err(ComputeError::Buffer(BufferError::MissingUsage))
    ));
    let math = MathGpuSession::new(&c);
    let report = math
        .try_squared_distance_pair_sum(&[[1., 2., 3.]], &[[0.; 3]])
        .unwrap();
    assert_eq!(report.value, 14.0);
    assert_eq!(report.backend, c.backend_report());
    assert_eq!(report.arithmetic, GpuArithmetic::SoftwareBinary64);
    assert_eq!(
        math.try_point_bounds(&[[f64::MAX, 0., 0.]])
            .unwrap()
            .value
            .min,
        [f64::MAX, 0., 0.]
    );
    assert!(math.point_bounds(&[[f64::NAN, 0., 0.]]).is_none());
}
