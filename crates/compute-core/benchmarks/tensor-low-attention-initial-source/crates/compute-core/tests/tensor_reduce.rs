use compute_core::{
    ComputeError, ComputeRuntime, GpuTensor, TensorComputeError,
    gpu_compute::GpuContext,
    tensor_core::{Layout, ReduceOp, Shape, TensorError},
};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(30);
const OPS: [ReduceOp; 4] = [
    ReduceOp::Sum,
    ReduceOp::Product,
    ReduceOp::Min,
    ReduceOp::Max,
];
fn context() -> Option<GpuContext> {
    let result = GpuContext::new();
    assert!(result.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    result
}
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn close(actual: f32, expected: f64) {
    assert!(
        actual.is_finite() && (f64::from(actual) - expected).abs() <= 3e-5 * expected.abs().max(1.),
        "{actual} != {expected}"
    );
}

#[test]
fn shader_reductions_satisfy_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_reduce_backend(&rt).unwrap();
}

#[test]
fn shader_vector_matmul_satisfies_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_matmul_backend(&rt).unwrap();
}

#[test]
fn f32_reductions_cover_tails_hierarchies_offsets_and_repeated_updates() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for n in [0, 1, 255, 256, 257, 4095, 4096, 4097, 131075] {
        let raw = rt.zeros::<f32>(2 * n + 3).unwrap();
        let input = GpuTensor::from_layout(
            raw.clone(),
            Layout::new(shape(&[2, n]), vec![1, 2], 1).unwrap(),
        )
        .unwrap();
        let output_raw = rt.upload(&[-999.0_f32; 6]).unwrap();
        let output = GpuTensor::from_layout(
            output_raw.clone(),
            Layout::new(shape(&[2, 1]), vec![1, 1], 2).unwrap(),
        )
        .unwrap();
        for op in OPS {
            let mut p = rt.program();
            if n == 0 && matches!(op, ReduceOp::Min | ReduceOp::Max) {
                assert!(matches!(
                    p.tensor_reduce_into(op, &input, &[1], true, &output),
                    Err(TensorComputeError::Tensor(TensorError::EmptyReduction))
                ));
                continue;
            }
            p.tensor_reduce_into(op, &input, &[1], true, &output)
                .unwrap();
            for iteration in 0..2 {
                let values: Vec<f32> = (0..2 * n)
                    .map(|i| {
                        if op == ReduceOp::Product {
                            if i / 2 == n / 2 {
                                2.
                            } else if (i + iteration) % 3 == 0 {
                                -1.
                            } else {
                                1.
                            }
                        } else {
                            ((i + 11 * iteration) % 31) as f32 / 8. - 1.75
                        }
                    })
                    .collect();
                rt.write(&raw, 1, &values).unwrap();
                let actual = p.submit_read(&output_raw).unwrap().wait(TIMEOUT).unwrap();
                for row in 0..2 {
                    let initial = match op {
                        ReduceOp::Sum => 0.,
                        ReduceOp::Product => 1.,
                        ReduceOp::Min => f64::INFINITY,
                        ReduceOp::Max => f64::NEG_INFINITY,
                    };
                    let expected =
                        (0..n)
                            .map(|col| f64::from(values[col * 2 + row]))
                            .fold(initial, |a, b| match op {
                                ReduceOp::Sum => a + b,
                                ReduceOp::Product => a * b,
                                ReduceOp::Min => a.min(b),
                                ReduceOp::Max => a.max(b),
                            });
                    close(actual[row + 2], expected);
                }
                assert_eq!(&actual[..2], &[-999.; 2]);
                assert_eq!(&actual[4..], &[-999.; 2]);
            }
        }
        let mut p = rt.program();
        if n == 0 {
            assert!(matches!(
                p.tensor_mean_into(&input, &[1], true, &output),
                Err(TensorComputeError::Tensor(TensorError::EmptyReduction))
            ));
        } else {
            p.tensor_mean_into(&input, &[1], true, &output).unwrap();
            for iteration in 0..2 {
                let values: Vec<f32> = (0..2 * n)
                    .map(|i| ((i + 7 * iteration) % 31) as f32 / 8. - 1.75)
                    .collect();
                rt.write(&raw, 1, &values).unwrap();
                let actual = p.submit_read(&output_raw).unwrap().wait(TIMEOUT).unwrap();
                for row in 0..2 {
                    close(
                        actual[row + 2],
                        (0..n).map(|c| f64::from(values[c * 2 + row])).sum::<f64>() / n as f64,
                    );
                }
            }
        }
    }
}

#[test]
fn u32_reductions_preserve_bits_wrapping_identities_and_hierarchy() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for n in [0, 1, 255, 256, 257, 4097, 131075] {
        let raw = rt.zeros::<u32>(2 * n + 3).unwrap();
        let input = GpuTensor::from_layout(
            raw.clone(),
            Layout::new(shape(&[2, n]), vec![1, 2], 1).unwrap(),
        )
        .unwrap();
        let output_raw = rt.upload(&[0xdeadbeefu32; 6]).unwrap();
        let output = GpuTensor::from_layout(
            output_raw.clone(),
            Layout::new(shape(&[2]), vec![1], 2).unwrap(),
        )
        .unwrap();
        for op in OPS {
            let mut p = rt.program();
            if n == 0 && matches!(op, ReduceOp::Min | ReduceOp::Max) {
                assert!(
                    p.tensor_reduce_into(op, &input, &[1], false, &output)
                        .is_err()
                );
                continue;
            }
            p.tensor_reduce_into(op, &input, &[1], false, &output)
                .unwrap();
            for iteration in 0..2 {
                let values: Vec<u32> = (0..2 * n)
                    .map(|i| [u32::MAX, 3, 0xfffffffb, 0x80000001, 17][(i + iteration) % 5])
                    .collect();
                rt.write(&raw, 1, &values).unwrap();
                let actual = p.submit_read(&output_raw).unwrap().wait(TIMEOUT).unwrap();
                for row in 0..2 {
                    let initial = match op {
                        ReduceOp::Sum | ReduceOp::Max => 0,
                        ReduceOp::Product => 1,
                        ReduceOp::Min => u32::MAX,
                    };
                    let expected = (0..n)
                        .map(|col| values[col * 2 + row])
                        .fold(initial, |a, b| match op {
                            ReduceOp::Sum => a.wrapping_add(b),
                            ReduceOp::Product => a.wrapping_mul(b),
                            ReduceOp::Min => a.min(b),
                            ReduceOp::Max => a.max(b),
                        });
                    assert_eq!(
                        actual[row + 2],
                        expected,
                        "n={n}, op={op:?}, iteration={iteration}, row={row}"
                    );
                }
                assert_eq!(&actual[..2], &[0xdeadbeef; 2]);
                assert_eq!(&actual[4..], &[0xdeadbeef; 2]);
            }
        }
    }
}

#[test]
fn vector_batched_matmul_reuses_strided_inputs_and_composes_with_reductions() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let n = 259;
    let raw = rt.zeros::<f32>(n * 3 + 4).unwrap();
    let vector =
        GpuTensor::from_layout(raw.clone(), Layout::new(shape(&[n]), vec![3], 2).unwrap()).unwrap();
    let matrix_raw = rt.zeros::<f32>(2 * 3 * n).unwrap();
    let matrix = GpuTensor::from_array(matrix_raw.clone(), shape(&[2, 3, n]))
        .unwrap()
        .permute(&[0, 2, 1])
        .unwrap();
    let output_raw = rt.upload(&[-999.0_f32; 9]).unwrap();
    let output = GpuTensor::from_layout(
        output_raw.clone(),
        Layout::new(shape(&[2, 3]), vec![3, 1], 1).unwrap(),
    )
    .unwrap();
    let weights =
        GpuTensor::from_array(rt.upload(&[0.5_f32, -1., 0.25]).unwrap(), shape(&[3])).unwrap();
    let mut p = rt.program();
    p.tensor_matmul_into(&vector, &matrix, &output).unwrap();
    let reduced_columns = p.tensor_matmul(&output, &weights).unwrap();
    let total = p.tensor_mean(&reduced_columns, &[0], false).unwrap();
    for iteration in 0..3 {
        let values: Vec<f32> = (0..n * 3 + 4)
            .map(|i| ((i + iteration * 7) % 19) as f32 / 8. - 1.)
            .collect();
        let matrix_values: Vec<f32> = (0..6 * n)
            .map(|i| ((i + iteration * 11) % 23) as f32 / 16. - 0.5)
            .collect();
        rt.write(&raw, 0, &values).unwrap();
        rt.write(&matrix_raw, 0, &matrix_values).unwrap();
        let mut encoder = rt.device().create_command_encoder(&Default::default());
        p.record(&mut encoder);
        let mut output_read = rt.record_read(&mut encoder, &output_raw).unwrap();
        let mut total_read = rt.record_read(&mut encoder, total.values()).unwrap();
        let submission = rt.queue().submit([encoder.finish()]);
        output_read.submitted(submission.clone());
        total_read.submitted(submission);
        let expected: Vec<f64> = (0..6)
            .map(|j| {
                (0..n)
                    .map(|k| f64::from(values[2 + 3 * k]) * f64::from(matrix_values[j * n + k]))
                    .sum()
            })
            .collect();
        let actual = output_read.wait(TIMEOUT).unwrap();
        for (&a, &e) in actual[1..7].iter().zip(&expected) {
            close(a, e);
        }
        assert_eq!(actual[0], -999.);
        assert_eq!(&actual[7..], &[-999.; 2]);
        close(
            total_read.wait(TIMEOUT).unwrap()[0],
            expected
                .iter()
                .enumerate()
                .map(|(i, &v)| v * [0.5, -1., 0.25][i % 3])
                .sum::<f64>()
                / 2.,
        );
    }
}

#[test]
fn reductions_reject_invalid_outputs_owners_axes_and_leave_program_usable() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let other = ComputeRuntime::new(&ctx).unwrap();
    let input =
        GpuTensor::from_array(rt.upload(&[1.0_f32, 2., 3., 4.]).unwrap(), shape(&[2, 2])).unwrap();
    let alias = input.narrow(0, 0, 1).unwrap().reshape(shape(&[2])).unwrap();
    let foreign = GpuTensor::from_array(other.zeros::<f32>(2).unwrap(), shape(&[2])).unwrap();
    let noncontiguous = GpuTensor::from_layout(
        rt.zeros::<f32>(3).unwrap(),
        Layout::new(shape(&[2]), vec![2], 0).unwrap(),
    )
    .unwrap();
    let mut p = rt.program();
    assert!(matches!(
        p.tensor_reduce_into(ReduceOp::Min, &input, &[0], false, &alias),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.tensor_mean_into(&input, &[0], false, &foreign),
        Err(TensorComputeError::Compute(ComputeError::ForeignArray))
    ));
    assert!(matches!(
        p.tensor_reduce_into(ReduceOp::Product, &input, &[0], false, &noncontiguous),
        Err(TensorComputeError::OutputNotContiguous)
    ));
    assert!(
        p.tensor_reduce(ReduceOp::Max, &input, &[0, 0], false)
            .is_err()
    );
    assert!(p.tensor_mean(&input, &[2], false).is_err());
    assert!(p.tensor_mean_into(&input, &[0, 1], false, &alias).is_err());
    let vector = input.narrow(0, 0, 1).unwrap().reshape(shape(&[2])).unwrap();
    assert!(matches!(
        p.tensor_matmul_into(&vector, &input, &vector),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    let output = p
        .tensor_reduce(ReduceOp::Product, &input, &[0, 1], false)
        .unwrap();
    assert_eq!(
        p.submit_read(output.values())
            .unwrap()
            .wait(TIMEOUT)
            .unwrap(),
        [24.]
    );
}
