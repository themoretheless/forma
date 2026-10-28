use compute_core::{
    ComputeError, ComputeRuntime, GpuElement, GpuTensor, TensorComputeError,
    gpu_compute::{GpuBuffer, GpuContext},
    tensor_core::{Layout, ScatterOp, Shape},
    wgpu,
};
use std::{fmt::Debug, time::Duration};
const TIMEOUT: Duration = Duration::from_secs(30);
fn context() -> Option<GpuContext> {
    let result = GpuContext::new();
    assert!(result.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    result
}
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn upload<T: GpuElement>(rt: &ComputeRuntime, dims: &[usize], data: &[T]) -> GpuTensor<T> {
    GpuTensor::from_array(rt.upload(data).unwrap(), shape(dims)).unwrap()
}

#[test]
fn shader_scatter_satisfies_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_scatter_backend(&rt).unwrap();
}

#[test]
fn replace_is_logically_last_and_repeated_plans_reset_owners_counts_and_base() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let columns = 257;
    let base_raw = rt.zeros::<f32>(44).unwrap();
    let base = GpuTensor::from_layout(
        base_raw.clone(),
        Layout::new(shape(&[2, 7, 3]), vec![21, 1, 7], 1).unwrap(),
    )
    .unwrap();
    let indices_raw = rt.zeros::<u32>(columns * 2 + 2).unwrap();
    let indices = GpuTensor::from_layout(
        indices_raw.clone(),
        Layout::new(shape(&[2, columns]), vec![1, 2], 1).unwrap(),
    )
    .unwrap();
    let updates_raw = rt.zeros::<f32>(3 * columns * 2 + 2).unwrap();
    let updates = GpuTensor::from_layout(
        updates_raw.clone(),
        Layout::new(shape(&[2, columns, 3]), vec![1, 2, columns * 2], 1).unwrap(),
    )
    .unwrap();
    let output_raw = rt.upload(&[-999.0_f32; 46]).unwrap();
    let output = GpuTensor::from_layout(
        output_raw.clone(),
        Layout::new(shape(&[2, 7, 3]), vec![21, 3, 1], 2).unwrap(),
    )
    .unwrap();
    let count_raw = rt.upload(&[0xdeadbeef_u32; 3]).unwrap();
    let count = GpuTensor::from_layout(
        count_raw.clone(),
        Layout::new(shape(&[]), vec![], 1).unwrap(),
    )
    .unwrap();
    let mut p = rt.program();
    p.tensor_scatter_into(
        ScatterOp::Replace,
        &base,
        &indices,
        &updates,
        1,
        &output,
        &count,
    )
    .unwrap();
    let total = p.tensor_sum(&output, &[0, 1, 2], false).unwrap();
    for iteration in 0..4 {
        let initial: Vec<f32> = (0..42).map(|i| (i + iteration * 7) as f32 / 8.).collect();
        let ids: Vec<u32> = (0..2 * columns)
            .map(|i| match iteration {
                0 => (i % 9) as u32,
                1 => u32::MAX,
                2 => {
                    if i == 0 {
                        3
                    } else {
                        u32::MAX
                    }
                }
                _ => ((2 * columns - i) % 8) as u32,
            })
            .collect();
        let values: Vec<f32> = (0..3 * columns * 2)
            .map(|i| ((i + iteration * 11) % 211) as f32 / 8. - 10.)
            .collect();
        rt.write(&base_raw, 1, &initial).unwrap();
        rt.write(&indices_raw, 1, &ids).unwrap();
        rt.write(&updates_raw, 1, &values).unwrap();
        let mut expected = vec![0.0_f32; 42];
        for outer in 0..2 {
            for axis in 0..7 {
                for inner in 0..3 {
                    expected[(outer * 7 + axis) * 3 + inner] =
                        initial[outer * 21 + inner * 7 + axis];
                }
            }
        }
        // Logical order [index_row,index_column] differs from physical order.
        for row in 0..2 {
            for col in 0..columns {
                let target = ids[col * 2 + row] as usize;
                if target < 7 {
                    for outer in 0..2 {
                        for inner in 0..3 {
                            expected[(outer * 7 + target) * 3 + inner] =
                                values[inner * columns * 2 + col * 2 + row];
                        }
                    }
                }
            }
        }
        let mut encoder = rt.device().create_command_encoder(&Default::default());
        p.record(&mut encoder);
        let mut result = rt.record_read(&mut encoder, &output_raw).unwrap();
        let mut invalid = rt.record_read(&mut encoder, &count_raw).unwrap();
        let mut sum = rt.record_read(&mut encoder, total.values()).unwrap();
        let submitted = rt.queue().submit([encoder.finish()]);
        result.submitted(submitted.clone());
        invalid.submitted(submitted.clone());
        sum.submitted(submitted);
        let result = result.wait(TIMEOUT).unwrap();
        assert_eq!(&result[2..44], expected, "iteration {iteration}");
        assert_eq!(&result[..2], &[-999.; 2]);
        assert_eq!(&result[44..], &[-999.; 2]);
        assert_eq!(
            invalid.wait(TIMEOUT).unwrap(),
            [
                0xdeadbeef,
                ids.iter().filter(|&&i| i >= 7).count() as u32,
                0xdeadbeef
            ]
        );
        assert_eq!(sum.wait(TIMEOUT).unwrap(), [expected.iter().sum::<f32>()]);
    }
}

fn contention<T: GpuElement + Debug + PartialEq>(
    rt: &ComputeRuntime,
    bases: [T; 2],
    value: impl Fn(ScatterOp, usize, usize) -> T,
    fold: impl Fn(ScatterOp, T, T) -> T,
) {
    let n = 4097;
    let initial: Vec<T> = bases
        .iter()
        .flat_map(|&v| std::iter::repeat_n(v, 17))
        .collect();
    let base = upload(rt, &[2, 17], &initial);
    let indices = GpuTensor::from_array(rt.zeros::<u32>(n).unwrap(), shape(&[n])).unwrap();
    let updates = GpuTensor::from_array(rt.zeros::<T>(n).unwrap(), shape(&[n])).unwrap();
    for op in [
        ScatterOp::Add,
        ScatterOp::Multiply,
        ScatterOp::Min,
        ScatterOp::Max,
    ] {
        let mut p = rt.program();
        let result = p.tensor_scatter(op, &base, &indices, &updates, 1).unwrap();
        for iteration in 0..3 {
            let ids: Vec<u32> = (0..n)
                .map(|i| match iteration {
                    0 => (i % 19) as u32,
                    1 => u32::MAX,
                    _ => 1,
                })
                .collect();
            let updates_cpu: Vec<T> = (0..n).map(|i| value(op, i, iteration)).collect();
            rt.write(indices.values(), 0, &ids).unwrap();
            rt.write(updates.values(), 0, &updates_cpu).unwrap();
            let mut expected = initial.clone();
            for row in 0..2 {
                for (i, &target) in ids.iter().enumerate() {
                    if target < 17 {
                        let index = row * 17 + target as usize;
                        expected[index] = fold(op, expected[index], updates_cpu[i]);
                    }
                }
            }
            let mut encoder = rt.device().create_command_encoder(&Default::default());
            p.record(&mut encoder);
            let mut output = rt
                .record_read(&mut encoder, result.values.values())
                .unwrap();
            let mut count = rt
                .record_read(&mut encoder, result.invalid_count.values())
                .unwrap();
            let submission = rt.queue().submit([encoder.finish()]);
            output.submitted(submission.clone());
            count.submitted(submission);
            assert_eq!(
                output.wait(TIMEOUT).unwrap(),
                expected,
                "op {op:?}, iteration {iteration}"
            );
            assert_eq!(
                count.wait(TIMEOUT).unwrap(),
                [ids.iter().filter(|&&i| i >= 17).count() as u32]
            );
        }
    }
}

#[test]
fn typed_atomic_folds_include_base_handle_contention_and_preserve_wrapping() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    contention(
        &rt,
        [2.0_f32, -3.0],
        |op, i, iteration| {
            if op == ScatterOp::Multiply {
                if (i + iteration) % 3 == 0 { -1. } else { 1. }
            } else {
                [-1.25, 0.75, 2., -4.][(i + iteration) % 4]
            }
        },
        |op, a, b| match op {
            ScatterOp::Add => a + b,
            ScatterOp::Multiply => a * b,
            ScatterOp::Min => a.min(b),
            ScatterOp::Max => a.max(b),
            _ => unreachable!(),
        },
    );
    contention(
        &rt,
        [u32::MAX - 4, u32::MAX - 7],
        |_, i, iteration| [u32::MAX, 3, 0x80000001, 17][(i + iteration) % 4],
        |op, a, b| match op {
            ScatterOp::Add => a.wrapping_add(b),
            ScatterOp::Multiply => a.wrapping_mul(b),
            ScatterOp::Min => a.min(b),
            ScatterOp::Max => a.max(b),
            _ => unreachable!(),
        },
    );
}

#[test]
fn scatter_rejects_cross_type_aliases_foreign_buffers_and_transactional_overflow() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let other = ComputeRuntime::new(&ctx).unwrap();
    let buffer = GpuBuffer::new(
        &ctx,
        16,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    let output = GpuTensor::from_array(
        rt.import_buffer::<f32>(buffer.clone(), 4).unwrap(),
        shape(&[4]),
    )
    .unwrap();
    let indices =
        GpuTensor::from_array(rt.import_buffer::<u32>(buffer, 4).unwrap(), shape(&[4])).unwrap();
    let base = upload(&rt, &[4], &[1.0_f32, 2., 3., 4.]);
    let update = upload(&rt, &[], &[7.0_f32]);
    let count = upload(&rt, &[], &[77u32]);
    let foreign = upload(&other, &[], &[0u32]);
    let distinct = upload(&rt, &[4], &[0u32, 1, 2, 3]);
    let mut p = rt.program();
    assert!(matches!(
        p.tensor_scatter_into(
            ScatterOp::Replace,
            &base,
            &indices,
            &update,
            0,
            &output,
            &count
        ),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.tensor_scatter_into(ScatterOp::Add, &base, &distinct, &update, 0, &base, &count),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.tensor_scatter_into(
            ScatterOp::Min,
            &base,
            &distinct,
            &update,
            0,
            &output,
            &foreign
        ),
        Err(TensorComputeError::Compute(ComputeError::ForeignArray))
    ));
    assert!(
        p.tensor_scatter(ScatterOp::Max, &base, &distinct, &update, 1)
            .is_err()
    );
    assert!(
        p.tensor_scatter(
            ScatterOp::Max,
            &base,
            &distinct,
            &upload(&rt, &[1, 4], &[0.0_f32; 4]),
            0
        )
        .is_err()
    );
    // The expanded updates have 2^32 elements although all physical inputs are
    // tiny. The preparation fails after local reset/copy planning; neither may
    // enter the caller's program or modify its sentinel output/count.
    let huge_base = upload(&rt, &[1, 1], &[1.0_f32])
        .broadcast_to(shape(&[65536, 1]))
        .unwrap();
    let huge_indices = upload(&rt, &[1], &[0u32])
        .broadcast_to(shape(&[65536]))
        .unwrap();
    let output_raw = rt.upload(&vec![-999.0_f32; 65536]).unwrap();
    let huge_output = GpuTensor::from_array(output_raw.clone(), shape(&[65536, 1])).unwrap();
    assert!(matches!(
        p.tensor_scatter_into(
            ScatterOp::Replace,
            &huge_base,
            &huge_indices,
            &update,
            1,
            &huge_output,
            &count
        ),
        Err(TensorComputeError::IndexTooLarge)
    ));
    assert_eq!(
        p.submit_read(&output_raw).unwrap().wait(TIMEOUT).unwrap(),
        vec![-999.; 65536]
    );
    assert_eq!(
        rt.read(count.values()).unwrap().wait(TIMEOUT).unwrap(),
        [77]
    );
    let result = p
        .tensor_scatter(ScatterOp::Add, &base, &distinct, &update, 0)
        .unwrap();
    assert_eq!(
        p.submit_read(result.values.values())
            .unwrap()
            .wait(TIMEOUT)
            .unwrap(),
        [8., 9., 10., 11.]
    );
}
