use compute_core::{
    CompareOp, ComputeError, ComputeRuntime, GpuLowTensor, GpuTensor, TensorComputeError,
    gpu_compute::{GpuBuffer, GpuContext},
    tensor_core::{
        Layout, LowDtype, ScanOptions, Shape, TensorBackend, TensorIndexBackend, TensorLowBackend,
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
fn integer(dtype: LowDtype, v: i32) -> u16 {
    let bits = (v as f32).to_bits();
    if dtype == LowDtype::Bf16 {
        return (bits >> 16) as u16;
    }
    if v == 0 {
        return 0;
    }
    ((bits >> 16) & 0x8000) as u16
        | ((((bits >> 23) & 255) - 112) << 10) as u16
        | ((bits & 0x7fffff) >> 13) as u16
}
#[test]
fn packed_index_backend_satisfies_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    tensor_core::conformance::check_low_index_backend(&rt).unwrap();
}
fn low_output(rt: &ComputeRuntime, dtype: LowDtype, s: Shape) -> (GpuLowTensor, GpuLowTensor) {
    let n = s.numel();
    let backing = rt
        .upload_low_bits(dtype, shape(&[n + 4]), &vec![0x5aa5; n + 4])
        .unwrap();
    let output = backing.narrow(0, 1, n).unwrap().reshape(s).unwrap();
    (backing, output)
}
fn check_low_output(rt: &ComputeRuntime, backing: &GpuLowTensor, expected: &[u16]) {
    let raw = rt.read_low_bits(backing).unwrap();
    assert_eq!(raw[0], 0x5aa5);
    assert_eq!(&raw[1..1 + expected.len()], expected);
    assert_eq!(&raw[1 + expected.len()..], &[0x5aa5; 3]);
}
#[test]
fn raw_payload_moves_replay_into_odd_halfwords_and_reset_resident_counts() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let n = 131_077;
        let backing = rt.zeros_low(dtype, shape(&[n + 2])).unwrap();
        let input = backing.narrow(0, 1, n).unwrap();
        let mask = rt.upload_u32(shape(&[n]), &vec![0; n]).unwrap();
        let other = rt.upload_low_bits(dtype, shape(&[]), &[0xffff]).unwrap();
        let (selected_back, selected) = low_output(&rt, dtype, shape(&[n]));
        let (compact_back, compact) = low_output(&rt, dtype, shape(&[n]));
        let count_back = rt.upload_u32(shape(&[3]), &[999; 3]).unwrap();
        let count = GpuTensor::from_layout(
            count_back.values().clone(),
            Layout::new(shape(&[]), vec![], 1).unwrap(),
        )
        .unwrap();
        let ids = rt.upload_u32(shape(&[7]), &[0; 7]).unwrap();
        let (gather_back, gather) = low_output(&rt, dtype, shape(&[7]));
        let invalid_back = rt.upload_u32(shape(&[3]), &[999; 3]).unwrap();
        let invalid = GpuTensor::from_layout(
            invalid_back.values().clone(),
            Layout::new(shape(&[]), vec![], 1).unwrap(),
        )
        .unwrap();
        let mut p = rt.program();
        p.tensor_select_low_into(&mask, &input, &other, &selected)
            .unwrap();
        p.tensor_compact_low_into(&selected, &mask, &compact, &count)
            .unwrap();
        p.tensor_gather_low_into(&compact, &ids, 0, &gather, &invalid)
            .unwrap();
        for iteration in 0..4 {
            let raw: Vec<u16> = (0..n + 2)
                .map(|i| ((i * 173 + iteration * 37) & 65535) as u16)
                .collect();
            let keep: Vec<u32> = (0..n)
                .map(|i| match iteration {
                    0 => u32::MAX,
                    1 => u32::from(i % 257 == 0),
                    2 => 0,
                    _ => u32::from(i % 2 == 0),
                })
                .collect();
            let indices = if iteration == 2 {
                [u32::MAX; 7]
            } else {
                [0, 1, 256, 65535, 131076, n as u32, u32::MAX]
            };
            rt.write_low_storage_bits(&backing, &raw).unwrap();
            rt.write(mask.values(), 0, &keep).unwrap();
            rt.write(ids.values(), 0, &indices).unwrap();
            p.submit();
            let selected_expected: Vec<u16> = (0..n)
                .map(|i| if keep[i] != 0 { raw[i + 1] } else { 0xffff })
                .collect();
            let mut compact_expected: Vec<_> = (0..n)
                .filter(|&i| keep[i] != 0)
                .map(|i| raw[i + 1])
                .collect();
            let retained = compact_expected.len() as u32;
            compact_expected.resize(n, 0);
            let gather_expected: Vec<_> = indices
                .iter()
                .map(|&i| compact_expected.get(i as usize).copied().unwrap_or(0))
                .collect();
            check_low_output(&rt, &selected_back, &selected_expected);
            check_low_output(&rt, &compact_back, &compact_expected);
            check_low_output(&rt, &gather_back, &gather_expected);
            assert_eq!(rt.read_u32(&count_back).unwrap(), [999, retained, 999]);
            assert_eq!(
                rt.read_u32(&invalid_back).unwrap(),
                [
                    999,
                    indices.iter().filter(|&&i| i >= n as u32).count() as u32,
                    999
                ]
            );
            assert_eq!(rt.read_low_bits(&backing).unwrap(), raw);
        }
    }
}
#[test]
fn direct_packed_scans_replay_strided_boundaries_and_preserve_output_neighbors() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for n in [0, 1, 255, 256, 257, 4097, 131077] {
            let storage = rt.zeros_low(dtype, shape(&[n + 2, 2])).unwrap();
            let input = storage.narrow(0, 1, n).unwrap().permute(&[1, 0]).unwrap();
            let (low_back, low) = low_output(&rt, dtype, shape(&[2, n]));
            let float_back = rt
                .upload_f32(shape(&[n * 2 + 4]), &vec![77.; n * 2 + 4])
                .unwrap();
            let float = GpuTensor::from_layout(
                float_back.values().clone(),
                Layout::new(shape(&[2, n]), vec![n, 1], 1).unwrap(),
            )
            .unwrap();
            for inclusive in [false, true] {
                for reverse in [false, true] {
                    let mut p = rt.program();
                    let options = ScanOptions { inclusive, reverse };
                    p.tensor_scan_low_f32_into(&input, 1, options, &float)
                        .unwrap();
                    p.tensor_scan_low_into(&input, 1, options, &low).unwrap();
                    for iteration in 0..2 {
                        let values: Vec<i32> = (0..(n + 2) * 2)
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
                        let mut expected = vec![0; n * 2];
                        for row in 0..2 {
                            let mut sum = 0;
                            for position in 0..n {
                                let col = if reverse { n - 1 - position } else { position };
                                let value = values[(col + 1) * 2 + row];
                                if inclusive {
                                    sum += value;
                                }
                                expected[row * n + col] = sum;
                                if !inclusive {
                                    sum += value;
                                }
                            }
                        }
                        check_low_output(
                            &rt,
                            &low_back,
                            &expected
                                .iter()
                                .map(|&v| integer(dtype, v))
                                .collect::<Vec<_>>(),
                        );
                        let actual = rt.read_f32(&float_back).unwrap();
                        assert_eq!(actual[0], 77.);
                        assert_eq!(
                            &actual[1..n * 2 + 1],
                            expected.iter().map(|&v| v as f32).collect::<Vec<_>>()
                        );
                        assert_eq!(&actual[n * 2 + 1..], &[77.; 3]);
                    }
                }
            }
        }
    }
}
#[test]
fn packed_index_recording_rejects_alias_dtype_owner_layout_and_rolls_back() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let foreign_rt = ComputeRuntime::new(&ctx).unwrap();
    let input = rt
        .upload_low_bits(LowDtype::F16, shape(&[2, 2]), &[0x3c00; 4])
        .unwrap();
    let other = rt.zeros_low(LowDtype::Bf16, shape(&[2, 2])).unwrap();
    let foreign = foreign_rt.zeros_low(LowDtype::F16, shape(&[2, 2])).unwrap();
    let mask = rt.upload_u32(shape(&[2, 2]), &[1; 4]).unwrap();
    let ids = rt.upload_u32(shape(&[2]), &[0, 1]).unwrap();
    let (back, output) = low_output(&rt, LowDtype::F16, shape(&[2, 2]));
    let count = rt.upload_u32(shape(&[]), &[777]).unwrap();
    let allocation = GpuBuffer::new(
        &ctx,
        16,
        compute_core::wgpu::BufferUsages::STORAGE
            | compute_core::wgpu::BufferUsages::COPY_SRC
            | compute_core::wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    let alias_low = GpuLowTensor::from_packed(
        LowDtype::F16,
        rt.import_buffer::<u32>(allocation.clone(), 4).unwrap(),
        8,
        Layout::contiguous(shape(&[4])).unwrap(),
    )
    .unwrap();
    let alias_f32 =
        GpuTensor::from_array(rt.import_buffer::<f32>(allocation, 4).unwrap(), shape(&[4]))
            .unwrap();
    let alias_u32 = GpuTensor::from_layout(
        alias_low.packed_words().clone(),
        Layout::contiguous(shape(&[4])).unwrap(),
    )
    .unwrap();
    let alias_count = GpuTensor::from_layout(
        input.packed_words().clone(),
        Layout::new(shape(&[]), vec![], 0).unwrap(),
    )
    .unwrap();
    let mut p = rt.program();
    assert!(
        p.tensor_compare_low(CompareOp::Equal, &input, &other)
            .is_err()
    );
    assert!(p.tensor_select_low(&mask, &input, &other).is_err());
    assert!(
        p.tensor_compare_low(CompareOp::Equal, &input, &foreign)
            .is_err()
    );
    assert!(p.tensor_gather_low(&foreign, &ids, 0).is_err());
    assert!(p.tensor_compact_low(&foreign, &mask).is_err());
    assert!(
        p.tensor_scan_low_f32(&foreign, 1, ScanOptions::default())
            .is_err()
    );
    assert!(
        p.tensor_select_low_into(&mask, &input, &input, &input)
            .is_err()
    );
    assert!(
        p.tensor_select_low_into(&mask, &input, &input, &output.permute(&[1, 0]).unwrap())
            .is_err()
    );
    assert!(
        p.tensor_gather_low_into(&input, &ids, 0, &output, &alias_count)
            .is_err()
    );
    assert!(
        p.tensor_gather_low_into(&input, &ids, 7, &output, &count)
            .is_err()
    );
    assert!(
        p.tensor_compact_low_into(
            &input,
            &mask,
            &output.reshape(shape(&[4])).unwrap(),
            &alias_count
        )
        .is_err()
    );
    assert!(matches!(
        p.tensor_compare_low_into(CompareOp::Equal, &alias_low, &alias_low, &alias_u32),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.tensor_scan_low_f32_into(&alias_low, 0, ScanOptions::default(), &alias_f32),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(
        p.tensor_scan_low_into(&input, 1, ScanOptions::default(), &other)
            .is_err()
    );
    assert!(
        p.tensor_scan_low_into(&input, 2, ScanOptions::default(), &output)
            .is_err()
    );
    p.submit();
    assert_eq!(rt.read_u32(&count).unwrap(), [777]);
    assert_eq!(rt.read_low_bits(&back).unwrap(), [0x5aa5; 8]);
    p.tensor_select_low_into(&mask, &input, &input, &output)
        .unwrap();
    p.submit();
    check_low_output(&rt, &back, &[0x3c00; 4]);
}

#[test]
fn prepared_low_compare_select_scan_gather_compact_chain_reuses_gpu_buffers() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let width = 257;
        let raw = rt.zeros_low(dtype, shape(&[width, 3])).unwrap();
        let input = raw.permute(&[1, 0]).unwrap();
        let zero = rt.zeros_low(dtype, shape(&[])).unwrap();
        let threshold = rt.zeros_low(dtype, shape(&[])).unwrap();
        let ids = rt.upload_u32(shape(&[5]), &[0; 5]).unwrap();
        let mut p = rt.program();
        let mask = p
            .tensor_compare_low(CompareOp::Greater, &input, &threshold)
            .unwrap();
        let selected = p.tensor_select_low(&mask, &input, &zero).unwrap();
        let scanned = p
            .tensor_scan_low(
                &selected,
                1,
                ScanOptions {
                    inclusive: false,
                    reverse: true,
                },
            )
            .unwrap();
        let gathered = p.tensor_gather_low(&scanned, &ids, 1).unwrap();
        let positive = p
            .tensor_compare_low(CompareOp::Greater, &gathered.values, &zero)
            .unwrap();
        let compact = p.tensor_compact_low(&gathered.values, &positive).unwrap();
        let floats = p.tensor_cast_to_f32(&compact.values).unwrap();
        let sum = p.sum(floats.values()).unwrap();
        for (iteration, limit) in [0, 1, -1].into_iter().enumerate() {
            let values: Vec<i32> = (0..width * 3)
                .map(|i| ((i / 3 + i % 3 + iteration) % 3) as i32 - 1)
                .collect();
            let indices = if iteration == 1 {
                [u32::MAX; 5]
            } else {
                [0, 1, 256, 257, 128]
            };
            rt.write_low_storage_bits(
                &raw,
                &values
                    .iter()
                    .map(|&v| integer(dtype, v))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            rt.write_low_storage_bits(&threshold, &[integer(dtype, limit)])
                .unwrap();
            rt.write(ids.values(), 0, &indices).unwrap();
            let mut expected = Vec::new();
            for row in 0..3 {
                let mut prefixes = vec![0; width];
                let mut total = 0;
                for col in (0..width).rev() {
                    prefixes[col] = total;
                    let value = values[col * 3 + row];
                    if value > limit {
                        total += value;
                    }
                }
                for &index in &indices {
                    let value = prefixes.get(index as usize).copied().unwrap_or(0);
                    if value > 0 {
                        expected.push(value);
                    }
                }
            }
            let want_sum = expected.iter().sum::<i32>() as f32;
            let count = expected.len() as u32;
            expected.resize(15, 0);
            let read = p
                .submit_read(&sum)
                .unwrap()
                .wait(std::time::Duration::from_secs(30))
                .unwrap();
            assert_eq!(read, [want_sum]);
            assert_eq!(rt.read_u32(&compact.count).unwrap(), [count]);
            assert_eq!(
                rt.read_u32(&gathered.invalid_count).unwrap(),
                [indices.iter().filter(|&&i| i >= width as u32).count() as u32]
            );
            assert_eq!(
                rt.read_low_bits(&compact.values).unwrap(),
                expected
                    .iter()
                    .map(|&v| integer(dtype, v))
                    .collect::<Vec<_>>()
            );
        }
    }
}
