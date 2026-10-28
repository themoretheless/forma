use compute_core::{
    ComputeError, ComputeRuntime, GpuLowTensor, GpuTensor, TensorComputeError,
    gpu_compute::{GpuBuffer, GpuContext},
    tensor_core::{
        Layout, LowDtype, ScatterOp, Shape, TensorBackend, TensorIndexBackend, TensorLowBackend,
    },
};
fn context() -> Option<GpuContext> {
    let c = GpuContext::new();
    assert!(c.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    c
}
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn decode(dtype: LowDtype, bits: u16) -> f32 {
    if dtype == LowDtype::Bf16 {
        return f32::from_bits(u32::from(bits) << 16);
    }
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = (bits >> 10) & 31;
    let fraction = u32::from(bits & 1023);
    if exponent == 31 {
        return f32::from_bits(sign | 0x7f800000 | (fraction << 13));
    }
    if exponent != 0 {
        return f32::from_bits(sign | ((u32::from(exponent) + 112) << 23) | (fraction << 13));
    }
    let value = fraction as f32 * 2f32.powi(-24);
    if sign == 0 { value } else { -value }
}
fn low_output(rt: &ComputeRuntime, dtype: LowDtype, s: Shape) -> (GpuLowTensor, GpuLowTensor) {
    let n = s.numel();
    let back = rt
        .upload_low_bits(dtype, shape(&[n + 4]), &vec![0x5aa5; n + 4])
        .unwrap();
    let output = back.narrow(0, 1, n).unwrap().reshape(s).unwrap();
    (back, output)
}
fn check_low(rt: &ComputeRuntime, back: &GpuLowTensor, expected: &[u16]) {
    let raw = rt.read_low_bits(back).unwrap();
    assert_eq!(raw[0], 0x5aa5);
    assert_eq!(&raw[1..1 + expected.len()], expected);
    assert_eq!(&raw[1 + expected.len()..], &[0x5aa5; 3]);
}
fn float_output(rt: &ComputeRuntime, s: Shape) -> (GpuTensor, GpuTensor) {
    let n = s.numel();
    let back = rt.upload_f32(shape(&[n + 4]), &vec![77.; n + 4]).unwrap();
    let output = GpuTensor::from_layout(
        back.values().clone(),
        Layout::new(
            s.clone(),
            Layout::contiguous(s).unwrap().strides().to_vec(),
            1,
        )
        .unwrap(),
    )
    .unwrap();
    (back, output)
}
fn check_float(rt: &ComputeRuntime, back: &GpuTensor, expected: &[f32]) {
    let raw = rt.read_f32(back).unwrap();
    assert_eq!(raw[0], 77.);
    assert_eq!(&raw[1 + expected.len()..], &[77.; 3]);
    for (&a, &b) in raw[1..1 + expected.len()].iter().zip(expected) {
        if b.is_nan() {
            assert!(a.is_nan());
        } else {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
}
#[test]
fn packed_low_scatter_satisfies_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_low_scatter_backend(&rt).unwrap();
}
#[test]
fn packed_replace_replays_strided_duplicate_updates_and_preserves_output_halfwords() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let n = 65_539;
        let base = rt.zeros_low(dtype, shape(&[4, 2])).unwrap();
        let input = base.permute(&[1, 0]).unwrap();
        let source = rt.zeros_low(dtype, shape(&[n + 2, 2])).unwrap();
        let updates = source.narrow(0, 1, n).unwrap().permute(&[1, 0]).unwrap();
        let indices_back = rt.upload_u32(shape(&[n + 2]), &vec![0; n + 2]).unwrap();
        let indices = GpuTensor::from_layout(
            indices_back.values().clone(),
            Layout::new(shape(&[n]), vec![1], 1).unwrap(),
        )
        .unwrap();
        let (low_back, low) = low_output(&rt, dtype, input.shape().clone());
        let (float_back, float) = float_output(&rt, input.shape().clone());
        let count_back = rt.upload_u32(shape(&[3]), &[999; 3]).unwrap();
        let count = GpuTensor::from_layout(
            count_back.values().clone(),
            Layout::new(shape(&[]), vec![], 1).unwrap(),
        )
        .unwrap();
        let mut p = rt.program();
        p.tensor_scatter_low_into(
            ScatterOp::Replace,
            &input,
            &indices,
            &updates,
            1,
            &low,
            &count,
        )
        .unwrap();
        p.tensor_scatter_low_f32_into(
            ScatterOp::Replace,
            &input,
            &indices,
            &updates,
            1,
            &float,
            &count,
        )
        .unwrap();
        for iteration in 0..4 {
            let base_bits: Vec<_> = (0..8)
                .map(|i| (0x7000 + i * 719 + iteration) as u16)
                .collect();
            let update_bits: Vec<_> = (0..(n + 2) * 2)
                .map(|i| ((i * 173 + iteration * 31) & 65535) as u16)
                .collect();
            let ids: Vec<u32> = (0..n)
                .map(|i| match iteration {
                    0 => (i % 4) as u32,
                    1 => u32::MAX,
                    2 => 0,
                    _ => {
                        if i % 3 == 0 {
                            5
                        } else {
                            (i % 4) as u32
                        }
                    }
                })
                .collect();
            rt.write_low_storage_bits(&base, &base_bits).unwrap();
            rt.write_low_storage_bits(&source, &update_bits).unwrap();
            rt.write(indices_back.values(), 1, &ids).unwrap();
            let mut expected = vec![0; 8];
            for row in 0..2 {
                for col in 0..4 {
                    expected[row * 4 + col] = base_bits[col * 2 + row];
                }
                for (i, &index) in ids.iter().enumerate() {
                    if index < 4 {
                        expected[row * 4 + index as usize] = update_bits[(i + 1) * 2 + row];
                    }
                }
            }
            p.submit();
            check_low(&rt, &low_back, &expected);
            check_float(
                &rt,
                &float_back,
                &expected
                    .iter()
                    .map(|&b| decode(dtype, b))
                    .collect::<Vec<_>>(),
            );
            assert_eq!(
                rt.read_u32(&count_back).unwrap(),
                [999, ids.iter().filter(|&&i| i >= 4).count() as u32, 999]
            );
            assert_eq!(rt.read_low_bits(&source).unwrap(), update_bits);
        }
    }
}
#[test]
fn packed_extrema_preserve_tiny_bits_and_arithmetic_rounds_completed_destination_once() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let base_bits = [0, 0x8000, 1, 0x8001];
        let base = rt.upload_low_bits(dtype, shape(&[4]), &base_bits).unwrap();
        let n = 513;
        let ids: Vec<u32> = (0..n).map(|i| (i % 4) as u32).collect();
        let indices = rt.upload_u32(shape(&[n]), &ids).unwrap();
        let updates = rt.zeros_low(dtype, shape(&[n])).unwrap();
        let (low_back, low) = low_output(&rt, dtype, shape(&[4]));
        let (float_back, float) = float_output(&rt, shape(&[4]));
        let count = rt.upload_u32(shape(&[]), &[77]).unwrap();
        for op in [ScatterOp::Min, ScatterOp::Max] {
            let mut p = rt.program();
            p.tensor_scatter_low_into(op, &base, &indices, &updates, 0, &low, &count)
                .unwrap();
            p.tensor_scatter_low_f32_into(op, &base, &indices, &updates, 0, &float, &count)
                .unwrap();
            for iteration in 0..2 {
                let values: Vec<u16> = (0..n)
                    .map(|i| [0, 0x8000, 1, 0x8001, 2, 0x8002][(i + iteration) % 6])
                    .collect();
                rt.write_low_storage_bits(&updates, &values).unwrap();
                let mut expected = base_bits;
                for (&i, &b) in ids.iter().zip(&values) {
                    let a = expected[i as usize];
                    let order = decode(dtype, b).total_cmp(&decode(dtype, a));
                    if (op == ScatterOp::Min && order.is_lt())
                        || (op == ScatterOp::Max && order.is_gt())
                    {
                        expected[i as usize] = b;
                    }
                }
                p.submit();
                check_low(&rt, &low_back, &expected);
                check_float(
                    &rt,
                    &float_back,
                    &expected
                        .iter()
                        .map(|&b| decode(dtype, b))
                        .collect::<Vec<_>>(),
                );
                assert_eq!(rt.read_u32(&count).unwrap(), [0]);
            }
        }
        let one = if dtype == LowDtype::F16 {
            0x3c00
        } else {
            0x3f80
        };
        let increment = if dtype == LowDtype::F16 {
            0x1000
        } else {
            0x3b80
        };
        let base = rt.upload_low_bits(dtype, shape(&[1]), &[one]).unwrap();
        let indices = rt.upload_u32(shape(&[3]), &[0; 3]).unwrap();
        let updates = rt
            .upload_low_bits(dtype, shape(&[3]), &[increment; 3])
            .unwrap();
        let (low_back, low) = low_output(&rt, dtype, shape(&[1]));
        let (float_back, float) = float_output(&rt, shape(&[1]));
        let mut p = rt.program();
        p.tensor_scatter_low_into(ScatterOp::Add, &base, &indices, &updates, 0, &low, &count)
            .unwrap();
        p.tensor_scatter_low_f32_into(ScatterOp::Add, &base, &indices, &updates, 0, &float, &count)
            .unwrap();
        for (ids, active) in [([0, 0, 0], 3), ([u32::MAX; 3], 0), ([0, u32::MAX, 0], 2)] {
            rt.write(indices.values(), 0, &ids).unwrap();
            p.submit();
            let expected = 1. + active as f32 * decode(dtype, increment);
            check_float(&rt, &float_back, &[expected]);
            let low_bits = if active == 3 {
                one + 2
            } else if active == 2 {
                one + 1
            } else {
                one
            };
            check_low(&rt, &low_back, &[low_bits]);
            if active == 3 {
                assert_ne!(expected.to_bits(), decode(dtype, low_bits).to_bits());
            }
            assert_eq!(rt.read_u32(&count).unwrap(), [3 - active]);
        }
        let two = 0x4000;
        let half = if dtype == LowDtype::F16 {
            0x3800
        } else {
            0x3f00
        };
        rt.write_low_storage_bits(&updates, &[two, one | 0x8000, half])
            .unwrap();
        let mut product = rt.program();
        product
            .tensor_scatter_low_into(
                ScatterOp::Multiply,
                &base,
                &indices,
                &updates,
                0,
                &low,
                &count,
            )
            .unwrap();
        product
            .tensor_scatter_low_f32_into(
                ScatterOp::Multiply,
                &base,
                &indices,
                &updates,
                0,
                &float,
                &count,
            )
            .unwrap();
        for (ids, negative) in [
            ([0, 0, 0], true),
            ([u32::MAX; 3], false),
            ([0, u32::MAX, 0], false),
        ] {
            rt.write(indices.values(), 0, &ids).unwrap();
            product.submit();
            check_float(&rt, &float_back, &[if negative { -1. } else { 1. }]);
            check_low(&rt, &low_back, &[if negative { one | 0x8000 } else { one }]);
        }
    }
}
#[test]
fn packed_scatter_rejects_aliases_and_invalid_recordings_without_resetting_outputs() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let foreign_rt = ComputeRuntime::new(&ctx).unwrap();
    let input = rt.zeros_low(LowDtype::F16, shape(&[2, 2])).unwrap();
    let updates = rt.zeros_low(LowDtype::F16, shape(&[2, 2])).unwrap();
    let foreign = foreign_rt.zeros_low(LowDtype::F16, shape(&[2, 2])).unwrap();
    let other = rt.zeros_low(LowDtype::Bf16, shape(&[2, 2])).unwrap();
    let indices = rt.upload_u32(shape(&[2]), &[0, 1]).unwrap();
    let count = rt.upload_u32(shape(&[]), &[777]).unwrap();
    let (back, output) = low_output(&rt, LowDtype::F16, shape(&[2, 2]));
    let allocation = GpuBuffer::new(
        &ctx,
        16,
        compute_core::wgpu::BufferUsages::STORAGE
            | compute_core::wgpu::BufferUsages::COPY_SRC
            | compute_core::wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    let low_alias = GpuLowTensor::from_packed(
        LowDtype::F16,
        rt.import_buffer::<u32>(allocation.clone(), 4).unwrap(),
        8,
        Layout::contiguous(shape(&[2, 2])).unwrap(),
    )
    .unwrap();
    let float_alias = GpuTensor::from_array(
        rt.import_buffer::<f32>(allocation, 4).unwrap(),
        shape(&[2, 2]),
    )
    .unwrap();
    let count_alias = GpuTensor::from_layout(
        input.packed_words().clone(),
        Layout::contiguous(shape(&[])).unwrap(),
    )
    .unwrap();
    let mut p = rt.program();
    for op in [
        ScatterOp::Replace,
        ScatterOp::Add,
        ScatterOp::Multiply,
        ScatterOp::Min,
        ScatterOp::Max,
    ] {
        assert!(
            p.tensor_scatter_low(op, &input, &indices, &other, 1)
                .is_err()
        );
        assert!(
            p.tensor_scatter_low(op, &foreign, &indices, &updates, 1)
                .is_err()
        );
        assert!(
            p.tensor_scatter_low_into(op, &input, &indices, &updates, 1, &input, &count)
                .is_err()
        );
        assert!(
            p.tensor_scatter_low_into(op, &input, &indices, &updates, 1, &output, &count_alias)
                .is_err()
        );
        assert!(
            p.tensor_scatter_low_into(
                op,
                &input,
                &indices,
                &updates,
                1,
                &output.permute(&[1, 0]).unwrap(),
                &count
            )
            .is_err()
        );
        assert!(
            p.tensor_scatter_low_into(op, &input, &indices, &updates, 2, &output, &count)
                .is_err()
        );
        assert!(matches!(
            p.tensor_scatter_low_f32_into(
                op,
                &low_alias,
                &indices,
                &updates,
                1,
                &float_alias,
                &count
            ),
            Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
        ));
    }
    p.submit();
    assert_eq!(rt.read_low_bits(&back).unwrap(), [0x5aa5; 8]);
    assert_eq!(rt.read_u32(&count).unwrap(), [777]);
    p.tensor_scatter_low_into(
        ScatterOp::Replace,
        &input,
        &indices,
        &updates,
        1,
        &output,
        &count,
    )
    .unwrap();
    p.submit();
    check_low(&rt, &back, &[0; 4]);
    assert_eq!(rt.read_u32(&count).unwrap(), [0]);
}

#[test]
fn recorded_low_scatter_gather_scan_chain_reuses_all_intermediates() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let encode = |v: u32| -> u16 {
            let b = (v as f32).to_bits();
            if dtype == LowDtype::Bf16 {
                return (b >> 16) as u16;
            }
            if v == 0 {
                return 0;
            }
            ((((b >> 23) & 255) - 112) << 10) as u16 | ((b & 0x7fffff) >> 13) as u16
        };
        let base = rt.zeros_low(dtype, shape(&[5])).unwrap();
        let updates = rt.zeros_low(dtype, shape(&[4])).unwrap();
        let indices = rt.upload_u32(shape(&[4]), &[0; 4]).unwrap();
        let mut p = rt.program();
        let scattered = p
            .tensor_scatter_low(ScatterOp::Add, &base, &indices, &updates, 0)
            .unwrap();
        let gathered = p.tensor_gather_low(&scattered.values, &indices, 0).unwrap();
        let prefixes = p
            .tensor_scan_low_f32(&gathered.values, 0, tensor_core::ScanOptions::default())
            .unwrap();
        let total = p.sum(prefixes.values()).unwrap();
        for (iteration, ids) in [[0, 0, 4, 5], [u32::MAX; 4], [4, 3, 2, 1]]
            .into_iter()
            .enumerate()
        {
            let base_values: Vec<u32> = (0..5).map(|i| i + iteration as u32).collect();
            let update_values: Vec<u32> = (0..4).map(|i| i + 1 + iteration as u32).collect();
            rt.write_low_storage_bits(
                &base,
                &base_values.iter().map(|&v| encode(v)).collect::<Vec<_>>(),
            )
            .unwrap();
            rt.write_low_storage_bits(
                &updates,
                &update_values.iter().map(|&v| encode(v)).collect::<Vec<_>>(),
            )
            .unwrap();
            rt.write(indices.values(), 0, &ids).unwrap();
            let mut destinations = base_values.clone();
            for (&index, &value) in ids.iter().zip(&update_values) {
                if index < 5 {
                    destinations[index as usize] += value;
                }
            }
            let mut sum = 0u32;
            let mut expected = 0u32;
            for &index in &ids {
                sum += destinations.get(index as usize).copied().unwrap_or(0);
                expected += sum;
            }
            assert_eq!(
                p.submit_read(&total)
                    .unwrap()
                    .wait(std::time::Duration::from_secs(30))
                    .unwrap(),
                [expected as f32]
            );
            let invalid = ids.iter().filter(|&&i| i >= 5).count() as u32;
            assert_eq!(rt.read_u32(&scattered.invalid_count).unwrap(), [invalid]);
            assert_eq!(rt.read_u32(&gathered.invalid_count).unwrap(), [invalid]);
        }
    }
}
