use compute_core::{
    ComputeError, ComputeRuntime, GpuLowTensor, GpuTensor, TensorComputeError,
    gpu_compute::{GpuBuffer, GpuContext},
    tensor_core::{
        BinaryOp, Layout, LowDtype, ReduceOp, Shape, TensorBackend, TensorLowBackend,
        TensorLowOpsBackend, UnaryOp,
    },
};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(30);
fn context() -> Option<GpuContext> {
    let result = GpuContext::new();
    assert!(result.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    result
}
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn finite(dtype: LowDtype, bits: u16) -> bool {
    match dtype {
        LowDtype::F16 => bits & 0x7c00 != 0x7c00,
        LowDtype::Bf16 => bits & 0x7f80 != 0x7f80,
    }
}
fn decode(dtype: LowDtype, bits: u16) -> f64 {
    if dtype == LowDtype::Bf16 {
        return f64::from(f32::from_bits(u32::from(bits) << 16));
    }
    let sign = if bits & 0x8000 == 0 { 1. } else { -1. };
    let exponent = (bits >> 10) & 31;
    let fraction = f64::from(bits & 1023) / 1024.;
    if exponent == 0 {
        sign * fraction * 2_f64.powi(-14)
    } else {
        sign * (1. + fraction) * 2_f64.powi(i32::from(exponent) - 15)
    }
}
// Test inputs are small integers, exactly representable in either format.
fn integer(dtype: LowDtype, value: i32) -> u16 {
    let bits = (value as f32).to_bits();
    if dtype == LowDtype::Bf16 {
        return (bits >> 16) as u16;
    }
    if value == 0 {
        return 0;
    }
    ((bits >> 16) & 0x8000) as u16
        | ((((bits >> 23) & 255) - 112) << 10) as u16
        | ((bits & 0x7fffff) >> 13) as u16
}
fn close(actual: f32, expected: f64) {
    assert!(
        actual.is_finite()
            && (f64::from(actual) - expected).abs() <= 2e-5 * expected.abs().max(1e-30),
        "{actual} != {expected}"
    );
}
fn close_low(dtype: LowDtype, actual: u16, expected: f64) {
    let (relative, minimum) = match dtype {
        LowDtype::F16 => (0.00051, 2_f64.powi(-24)),
        LowDtype::Bf16 => (0.00391, 2_f64.powi(-133)),
    };
    assert!(
        (decode(dtype, actual) - expected).abs() <= (relative * expected.abs()).max(minimum),
        "{dtype:?} 0x{actual:04x} != {expected}"
    );
}

#[test]
fn packed_low_operations_satisfy_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_low_ops_backend(&rt).unwrap();
}

#[test]
fn packed_sign_and_extrema_preserve_every_finite_pattern_and_halfword_neighbors() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let bits: Vec<u16> = (0..=u16::MAX).filter(|&b| finite(dtype, b)).collect();
        let input = rt
            .upload_low_bits(dtype, shape(&[bits.len()]), &bits)
            .unwrap();
        let right_bits: Vec<u16> = bits.iter().rev().copied().collect();
        let right = rt
            .upload_low_bits(dtype, shape(&[bits.len()]), &right_bits)
            .unwrap();
        for op in [UnaryOp::Negate, UnaryOp::Abs] {
            let backing = rt
                .upload_low_bits(
                    dtype,
                    shape(&[bits.len() + 4]),
                    &vec![0x5aa5; bits.len() + 4],
                )
                .unwrap();
            let output = backing.narrow(0, 1, bits.len()).unwrap();
            let mut p = rt.program();
            p.tensor_unary_low_into(op, &input, &output).unwrap();
            p.submit();
            let actual = rt.read_low_bits(&backing).unwrap();
            assert_eq!(actual[0], 0x5aa5);
            assert_eq!(&actual[bits.len() + 1..], &[0x5aa5; 3]);
            let expected: Vec<u16> = bits
                .iter()
                .map(|&b| {
                    if op == UnaryOp::Negate {
                        b ^ 0x8000
                    } else {
                        b & 0x7fff
                    }
                })
                .collect();
            assert_eq!(&actual[1..bits.len() + 1], expected);
        }
        for op in [BinaryOp::Min, BinaryOp::Max] {
            let output = rt.binary_low(op, &input, &right).unwrap();
            let actual = rt.read_low_bits(&output).unwrap();
            for (i, (&a, &b)) in bits.iter().zip(&right_bits).enumerate() {
                let order = decode(dtype, a).total_cmp(&decode(dtype, b));
                let expected = if (op == BinaryOp::Min && order.is_gt())
                    || (op == BinaryOp::Max && order.is_lt())
                {
                    b
                } else {
                    a
                };
                assert_eq!(actual[i], expected, "{dtype:?} {op:?} at{i}");
            }
        }
        // Explicit signed-zero ties and extrema in the f32-subnormal BF16 range.
        let zeros = rt
            .upload_low_bits(dtype, shape(&[2]), &[0, 0x8000])
            .unwrap();
        for (op, expected) in [(ReduceOp::Min, 0x8000), (ReduceOp::Max, 0)] {
            let output = rt.reduce_low(op, &zeros, &[0], false).unwrap();
            assert_eq!(rt.read_low_bits(&output).unwrap(), [expected]);
        }
        let subnormal: Vec<u16> = (0..131077)
            .map(|i| [0, 0x8000, 1, 2, 0x8001, 0x8002][i % 6])
            .collect();
        let tensor = rt
            .upload_low_bits(dtype, shape(&[subnormal.len()]), &subnormal)
            .unwrap();
        for (op, expected) in [(ReduceOp::Min, 0x8002), (ReduceOp::Max, 2)] {
            let output = rt.reduce_low_f32(op, &tensor, &[0], false).unwrap();
            let actual = rt.read_f32(&output).unwrap();
            assert_eq!(
                actual[0].to_bits(),
                (decode(dtype, expected) as f32).to_bits()
            );
        }
    }
}

#[test]
fn packed_reduction_hierarchies_preserve_offsets_and_reuse_f32_and_low_outputs() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for n in [0, 1, 255, 256, 257, 4096, 4097, 131077] {
            let backing = rt.zeros_low(dtype, shape(&[2 * n + 4])).unwrap();
            let input = GpuLowTensor::from_packed(
                dtype,
                backing.packed_words().clone(),
                backing.storage_len(),
                Layout::new(shape(&[2, n]), vec![1, 2], 1).unwrap(),
            )
            .unwrap();
            for op in [
                ReduceOp::Sum,
                ReduceOp::Product,
                ReduceOp::Min,
                ReduceOp::Max,
            ] {
                let float_raw = rt.upload(&[-999.0_f32; 6]).unwrap();
                let output = GpuTensor::from_layout(
                    float_raw.clone(),
                    Layout::new(shape(&[2, 1]), vec![1, 1], 2).unwrap(),
                )
                .unwrap();
                let low_raw = rt
                    .upload_low_bits(dtype, shape(&[7]), &[0x5aa5; 7])
                    .unwrap();
                let low = GpuLowTensor::from_packed(
                    dtype,
                    low_raw.packed_words().clone(),
                    7,
                    Layout::new(shape(&[2, 1]), vec![1, 1], 1).unwrap(),
                )
                .unwrap();
                let mut p = rt.program();
                if n == 0 && matches!(op, ReduceOp::Min | ReduceOp::Max) {
                    assert!(
                        p.tensor_reduce_low_f32_into(op, &input, &[1], true, &output)
                            .is_err()
                    );
                    continue;
                }
                p.tensor_reduce_low_f32_into(op, &input, &[1], true, &output)
                    .unwrap();
                p.tensor_reduce_low_into(op, &input, &[1], true, &low)
                    .unwrap();
                let mean = if n > 0 {
                    Some(p.tensor_mean_low_f32(&input, &[1], false).unwrap())
                } else {
                    None
                };
                for iteration in 0..2 {
                    let values: Vec<i32> = (0..2 * n)
                        .map(|i| {
                            if op == ReduceOp::Product {
                                if i / 2 == n / 2 {
                                    2
                                } else if (i + iteration) % 3 == 0 {
                                    -1
                                } else {
                                    1
                                }
                            } else {
                                ((i + iteration * 3) % 5) as i32 - 2
                            }
                        })
                        .collect();
                    let mut bits = vec![0; 2 * n + 4];
                    for (i, &v) in values.iter().enumerate() {
                        bits[1 + i] = integer(dtype, v);
                    }
                    rt.write_low_storage_bits(&backing, &bits).unwrap();
                    p.submit();
                    let actual = rt.read(&float_raw).unwrap().wait(TIMEOUT).unwrap();
                    let actual_low = rt.read_low_bits(&low_raw).unwrap();
                    let actual_mean = mean.as_ref().map(|m| rt.read_f32(m).unwrap());
                    assert_eq!(&actual[..2], &[-999.; 2]);
                    assert_eq!(&actual[4..], &[-999.; 2]);
                    assert_eq!(actual_low[0], 0x5aa5);
                    assert_eq!(&actual_low[3..], &[0x5aa5; 4]);
                    for row in 0..2 {
                        let expected = (0..n).map(|c| f64::from(values[c * 2 + row])).fold(
                            match op {
                                ReduceOp::Sum => 0.,
                                ReduceOp::Product => 1.,
                                ReduceOp::Min => f64::INFINITY,
                                ReduceOp::Max => f64::NEG_INFINITY,
                            },
                            |a, b| match op {
                                ReduceOp::Sum => a + b,
                                ReduceOp::Product => a * b,
                                ReduceOp::Min => a.min(b),
                                ReduceOp::Max => a.max(b),
                            },
                        );
                        close(actual[row + 2], expected);
                        close_low(dtype, actual_low[row + 1], expected);
                        if let Some(mean) = &actual_mean {
                            close(
                                mean[row],
                                (0..n).map(|c| f64::from(values[c * 2 + row])).sum::<f64>()
                                    / n as f64,
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn resident_low_arithmetic_reduction_chain_updates_without_operand_expansion() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let n = 257;
        let storage = rt.zeros_low(dtype, shape(&[n, 3])).unwrap();
        let input = storage.permute(&[1, 0]).unwrap();
        let one = rt
            .upload_low_bits(dtype, shape(&[]), &[integer(dtype, 1)])
            .unwrap();
        let mut p = rt.program();
        let squared = p.tensor_unary_low(UnaryOp::Square, &input).unwrap();
        let plus_one = p.tensor_binary_low(BinaryOp::Add, &squared, &one).unwrap();
        let sum = p
            .tensor_reduce_low_f32(ReduceOp::Sum, &plus_one, &[1], false)
            .unwrap();
        let low_sum = p
            .tensor_reduce_low(ReduceOp::Sum, &plus_one, &[1], false)
            .unwrap();
        let low_mean = p.tensor_mean_low(&plus_one, &[1], false).unwrap();
        for iteration in 0..3 {
            let values: Vec<i32> = (0..n * 3)
                .map(|i| ((i + iteration) % 5) as i32 - 2)
                .collect();
            rt.write_low_storage_bits(
                &storage,
                &values
                    .iter()
                    .map(|&v| integer(dtype, v))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            p.submit();
            let actual = rt.read_f32(&sum).unwrap();
            let low = rt.read_low_bits(&low_sum).unwrap();
            let mean = rt.read_low_bits(&low_mean).unwrap();
            for row in 0..3 {
                let expected = (0..n)
                    .map(|c| f64::from(values[c * 3 + row].pow(2) + 1))
                    .sum::<f64>();
                close(actual[row], expected);
                close_low(dtype, low[row], expected);
                close_low(dtype, mean[row], expected / n as f64);
            }
        }
    }
}

#[test]
fn low_ops_validate_dtype_owner_layout_and_cross_interpretation_aliases() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let other = ComputeRuntime::new(&ctx).unwrap();
    let a = rt
        .upload_low_bits(LowDtype::F16, shape(&[2, 2]), &[0x3c00; 4])
        .unwrap();
    let b = rt
        .upload_low_bits(LowDtype::Bf16, shape(&[2, 2]), &[0x3f80; 4])
        .unwrap();
    let foreign = other.zeros_low(LowDtype::F16, shape(&[2, 2])).unwrap();
    let output = rt
        .upload_low_bits(LowDtype::F16, shape(&[2, 2]), &[0x5aa5; 4])
        .unwrap();
    let allocation = GpuBuffer::new(
        &ctx,
        16,
        compute_core::wgpu::BufferUsages::STORAGE
            | compute_core::wgpu::BufferUsages::COPY_SRC
            | compute_core::wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    let float_alias = GpuTensor::from_array(
        rt.import_buffer::<f32>(allocation.clone(), 4).unwrap(),
        shape(&[4]),
    )
    .unwrap();
    let low_alias = GpuLowTensor::from_packed(
        LowDtype::F16,
        rt.import_buffer::<u32>(allocation, 4).unwrap(),
        8,
        Layout::contiguous(shape(&[4, 2])).unwrap(),
    )
    .unwrap();
    let mut p = rt.program();
    assert!(p.tensor_binary_low(BinaryOp::Add, &a, &b).is_err());
    assert!(p.tensor_unary_low(UnaryOp::Abs, &foreign).is_err());
    assert!(p.tensor_unary_low_into(UnaryOp::Negate, &a, &b).is_err());
    assert!(p.tensor_binary_low_into(BinaryOp::Add, &a, &a, &a).is_err());
    assert!(
        p.tensor_unary_low_into(UnaryOp::Abs, &a, &output.permute(&[1, 0]).unwrap())
            .is_err()
    );
    assert!(
        p.tensor_reduce_low(ReduceOp::Sum, &a, &[0, 0], false)
            .is_err()
    );
    assert!(p.tensor_mean_low(&a, &[2], false).is_err());
    assert!(matches!(
        p.tensor_reduce_low_f32_into(ReduceOp::Sum, &low_alias, &[1], false, &float_alias),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    p.submit();
    assert_eq!(rt.read_low_bits(&output).unwrap(), [0x5aa5; 4]);
    p.tensor_unary_low_into(UnaryOp::Negate, &a, &output)
        .unwrap();
    p.submit();
    assert_eq!(rt.read_low_bits(&output).unwrap(), [0xbc00; 4]);
}
