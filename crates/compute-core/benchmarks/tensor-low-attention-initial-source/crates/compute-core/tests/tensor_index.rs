use compute_core::{ComputeRuntime, gpu_compute::GpuContext};
fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    assert!(
        context.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "shader GPU required"
    );
    context
}
#[test]
fn shader_index_backend_satisfies_shared_conformance() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    tensor_core::conformance::check_index_backend(&runtime).unwrap();
}

use compute_core::{
    CompareOp, ComputeError, GpuTensor, TensorComputeError,
    tensor_core::{Layout, ScanOptions, Shape},
    wgpu,
};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(30);
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

#[test]
fn prepared_mask_scan_gather_compact_chain_reuses_all_gpu_state() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let width = 513;
    let input_raw = rt.zeros::<f32>(width * 3).unwrap();
    let input = GpuTensor::from_array(input_raw.clone(), shape(&[width, 3]))
        .unwrap()
        .permute(&[1, 0])
        .unwrap();
    let threshold = GpuTensor::from_array(rt.upload(&[0.0f32]).unwrap(), shape(&[])).unwrap();
    let zero = GpuTensor::from_array(rt.upload(&[0.0f32]).unwrap(), shape(&[])).unwrap();
    let indices = GpuTensor::from_array(rt.zeros::<u32>(6).unwrap(), shape(&[2, 3])).unwrap();
    let mut p = rt.program();
    let mask = p
        .tensor_compare(CompareOp::Greater, &input, &threshold)
        .unwrap();
    let selected = p.tensor_select(&mask, &input, &zero).unwrap();
    let scanned = p
        .tensor_scan(
            &selected,
            1,
            ScanOptions {
                inclusive: false,
                reverse: true,
            },
        )
        .unwrap();
    let gathered = p.tensor_gather(&scanned, &indices, 1).unwrap();
    let keep = p
        .tensor_compare(CompareOp::Greater, &gathered.values, &zero)
        .unwrap();
    let compacted = p.tensor_compact(&gathered.values, &keep).unwrap();
    let total = p.sum(compacted.values.values()).unwrap();
    for (iteration, (limit, ids)) in [
        (-1.0, [0, 512, 513, 1, u32::MAX, 255]),
        (0.5, [0, 1, 2, 3, 4, 5]),
        (1000.0, [999, 1000, 1001, u32::MAX, 513, 600]),
    ]
    .into_iter()
    .enumerate()
    {
        let values: Vec<f32> = (0..width * 3)
            .map(|i| ((i + iteration * 11) % 37) as f32 / 8.0 - 2.0)
            .collect();
        rt.write(&input_raw, 0, &values).unwrap();
        rt.write(threshold.values(), 0, &[limit]).unwrap();
        rt.write(indices.values(), 0, &ids).unwrap();
        let mut expected = Vec::new();
        for row in 0..3 {
            let mut scan = vec![0.0f32; width];
            let mut sum = 0.;
            for col in (0..width).rev() {
                scan[col] = sum;
                let v = values[col * 3 + row];
                if v > limit {
                    sum += v;
                }
            }
            for &id in &ids {
                let v = scan.get(id as usize).copied().unwrap_or(0.0);
                if v > 0.0 {
                    expected.push(v);
                }
            }
        }
        let wanted_sum: f32 = expected.iter().sum();
        let wanted_count = expected.len() as u32;
        expected.resize(18, 0.0);
        let mut encoder = rt.device().create_command_encoder(&Default::default());
        p.record(&mut encoder);
        let mut total_read = rt.record_read(&mut encoder, &total).unwrap();
        let mut values_read = rt
            .record_read(&mut encoder, compacted.values.values())
            .unwrap();
        let mut count_read = rt
            .record_read(&mut encoder, compacted.count.values())
            .unwrap();
        let mut invalid_read = rt
            .record_read(&mut encoder, gathered.invalid_count.values())
            .unwrap();
        let submission = rt.queue().submit([encoder.finish()]);
        total_read.submitted(submission.clone());
        values_read.submitted(submission.clone());
        count_read.submitted(submission.clone());
        invalid_read.submitted(submission);
        assert_eq!(total_read.wait(TIMEOUT).unwrap(), [wanted_sum]);
        assert_eq!(values_read.wait(TIMEOUT).unwrap(), expected);
        assert_eq!(count_read.wait(TIMEOUT).unwrap(), [wanted_count]);
        assert_eq!(
            invalid_read.wait(TIMEOUT).unwrap(),
            [ids.iter().filter(|&&i| i >= width as u32).count() as u32]
        );
    }
}

#[test]
fn scans_cover_hierarchy_boundaries_reverse_wrapping_and_offset_outputs() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for n in [0, 1, 255, 256, 257, 65537, 131075] {
        let input_raw = rt.zeros::<u32>(n * 2 + 2).unwrap();
        let input = GpuTensor::from_layout(
            input_raw.clone(),
            Layout::new(shape(&[n, 2]), vec![2, 1], 1).unwrap(),
        )
        .unwrap()
        .permute(&[1, 0])
        .unwrap();
        let output_raw = rt.upload(&vec![0xdeadbeefu32; n * 2 + 4]).unwrap();
        let output = GpuTensor::from_layout(
            output_raw.clone(),
            Layout::new(shape(&[2, n]), vec![n, 1], 2).unwrap(),
        )
        .unwrap();
        for inclusive in [false, true] {
            for reverse in [false, true] {
                let mut p = rt.program();
                p.tensor_scan_into(&input, 1, ScanOptions { inclusive, reverse }, &output)
                    .unwrap();
                for iteration in 0..2 {
                    let values: Vec<u32> = (0..n * 2)
                        .map(|i| [u32::MAX, 3, 17, 0, 0xfffffff0][(i + iteration) % 5])
                        .collect();
                    rt.write(&input_raw, 1, &values).unwrap();
                    let mut expected = vec![0u32; n * 2];
                    for row in 0..2 {
                        let mut sum = 0u32;
                        for position in 0..n {
                            let col = if reverse { n - 1 - position } else { position };
                            let v = values[col * 2 + row];
                            if inclusive {
                                sum = sum.wrapping_add(v);
                            }
                            expected[row * n + col] = sum;
                            if !inclusive {
                                sum = sum.wrapping_add(v);
                            }
                        }
                    }
                    let actual = p.submit_read(&output_raw).unwrap().wait(TIMEOUT).unwrap();
                    assert_eq!(
                        &actual[2..2 + n * 2],
                        expected,
                        "n={n},inclusive={inclusive},reverse={reverse}"
                    );
                    assert_eq!(&actual[..2], &[0xdeadbeef; 2]);
                    assert_eq!(&actual[2 + n * 2..], &[0xdeadbeef; 2]);
                }
            }
        }
    }
}

#[test]
fn u32_compaction_and_gather_preserve_large_integer_bits_and_reset_counts() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let n = 4097;
    let values: Vec<u32> = (0..n * 2)
        .map(|i| u32::MAX.wrapping_sub(i as u32))
        .collect();
    let input = GpuTensor::from_array(rt.upload(&values).unwrap(), shape(&[n, 2]))
        .unwrap()
        .permute(&[1, 0])
        .unwrap();
    let mask = GpuTensor::from_array(rt.zeros::<u32>(n).unwrap(), shape(&[1, n])).unwrap();
    let output_raw = rt.upload(&vec![77u32; n * 2 + 4]).unwrap();
    let output = GpuTensor::from_layout(
        output_raw.clone(),
        Layout::new(shape(&[n * 2]), vec![1], 2).unwrap(),
    )
    .unwrap();
    let count_raw = rt.upload(&[88u32, 999, 88]).unwrap();
    let count = GpuTensor::from_layout(
        count_raw.clone(),
        Layout::new(shape(&[]), vec![], 1).unwrap(),
    )
    .unwrap();
    let mut p = rt.program();
    p.tensor_compact_into(&input, &mask, &output, &count)
        .unwrap();
    for all in [false, true, false] {
        let mask_values: Vec<u32> = (0..n)
            .map(|i| if all || i % 3 == 0 { 0xffffffff } else { 0 })
            .collect();
        rt.write(mask.values(), 0, &mask_values).unwrap();
        let mut expected = Vec::new();
        for row in 0..2 {
            for col in 0..n {
                if mask_values[col] != 0 {
                    expected.push(values[col * 2 + row]);
                }
            }
        }
        let selected = expected.len() as u32;
        expected.resize(n * 2, 0);
        let actual = p.submit_read(&output_raw).unwrap().wait(TIMEOUT).unwrap();
        assert_eq!(&actual[2..2 + n * 2], expected);
        assert_eq!(&actual[..2], &[77; 2]);
        assert_eq!(&actual[2 + n * 2..], &[77; 2]);
        assert_eq!(
            rt.read(&count_raw).unwrap().wait(TIMEOUT).unwrap(),
            [88, selected, 88]
        );
    }
    let empty = GpuTensor::from_array(rt.zeros::<u32>(0).unwrap(), shape(&[0, 3])).unwrap();
    let ids = GpuTensor::from_array(rt.upload(&[3u32, 0, u32::MAX]).unwrap(), shape(&[3])).unwrap();
    let mut p = rt.program();
    let gathered = p.tensor_gather(&empty, &ids, 1).unwrap();
    for index_values in [[3, u32::MAX, 4], [0, 1, 2]] {
        rt.write(ids.values(), 0, &index_values).unwrap();
        assert!(
            p.submit_read(gathered.values.values())
                .unwrap()
                .wait(TIMEOUT)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            rt.read(gathered.invalid_count.values())
                .unwrap()
                .wait(TIMEOUT)
                .unwrap(),
            [index_values.iter().filter(|&&x| x >= 3).count() as u32]
        );
    }
}

#[test]
fn indexing_errors_reject_cross_type_aliases_foreign_counts_and_invalid_axes() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let other = ComputeRuntime::new(&ctx).unwrap();
    let buffer = compute_core::gpu_compute::GpuBuffer::new(
        &ctx,
        16,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    let floats = GpuTensor::from_array(
        rt.import_buffer::<f32>(buffer.clone(), 4).unwrap(),
        shape(&[4]),
    )
    .unwrap();
    let ints =
        GpuTensor::from_array(rt.import_buffer::<u32>(buffer, 4).unwrap(), shape(&[4])).unwrap();
    let distinct =
        GpuTensor::from_array(rt.upload(&[1u32, 0, 1, 0]).unwrap(), shape(&[4])).unwrap();
    let scalar = GpuTensor::from_array(rt.upload(&[0.0f32]).unwrap(), shape(&[])).unwrap();
    let out = GpuTensor::from_array(rt.zeros::<f32>(4).unwrap(), shape(&[4])).unwrap();
    let foreign = GpuTensor::from_array(other.zeros::<u32>(1).unwrap(), shape(&[])).unwrap();
    let mut p = rt.program();
    assert!(matches!(
        p.tensor_compare_into(CompareOp::Greater, &floats, &scalar, &ints),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.tensor_select_into(&ints, &floats, &scalar, &floats),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.tensor_gather_into(&floats, &distinct, 0, &out, &foreign),
        Err(TensorComputeError::Compute(ComputeError::ForeignArray))
    ));
    assert!(p.tensor_scan(&ints, 1, ScanOptions::default()).is_err());
    assert!(p.tensor_gather(&floats, &distinct, 1).is_err());
    assert!(
        p.tensor_compact_into(&floats, &distinct, &out, &distinct)
            .is_err()
    );
    rt.write(floats.values(), 0, &[1., 2., 3., 4.]).unwrap();
    let result = p
        .tensor_compare(CompareOp::Greater, &floats, &scalar)
        .unwrap();
    assert_eq!(
        p.submit_read(result.values())
            .unwrap()
            .wait(TIMEOUT)
            .unwrap(),
        [1, 1, 1, 1]
    );
}
