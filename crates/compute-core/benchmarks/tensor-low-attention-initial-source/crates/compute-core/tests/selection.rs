use compute_core::{CompareOp, ComputeError, ComputeRuntime, gpu_compute::GpuContext};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);

fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    if context.is_none() {
        assert!(
            std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
            "COMPUTE_REQUIRE_GPU set but no GPU adapter found"
        );
        eprintln!("selection test skipped: no GPU adapter");
    }
    context
}

#[test]
fn exclusive_scan_matches_wrapping_cpu_across_blocks_and_levels() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    for len in [
        0, 1, 255, 256, 257, 1023, 1024, 1025, 65535, 65536, 65537, 262147, 1048577,
    ] {
        let input = runtime.zeros::<u32>(len).unwrap();
        let storage = runtime.upload(&vec![0xa5a5a5a5u32; len + 7]).unwrap();
        let output = storage.prefix(len).unwrap();
        let mut program = runtime.program();
        program.exclusive_scan_into(&input, &output).unwrap();
        for iteration in 0..3 {
            let values: Vec<u32> = (0..len)
                .map(|i| match (i + iteration) % 5 {
                    0 => u32::MAX,
                    1 => 0,
                    _ => (i as u32)
                        .wrapping_mul(1664525)
                        .wrapping_add(iteration as u32),
                })
                .collect();
            runtime.write(&input, 0, &values).unwrap();
            let actual = program
                .submit_read(&storage)
                .unwrap()
                .wait(TIMEOUT)
                .unwrap();
            let mut total = 0u32;
            for (i, value) in values.iter().enumerate() {
                assert_eq!(
                    actual[i], total,
                    "length {len}, iteration {iteration}, element {i}"
                );
                total = total.wrapping_add(*value);
            }
            assert_eq!(
                &actual[len..],
                &[0xa5a5a5a5u32; 7],
                "prefix write changed capacity tail"
            );
        }
    }
}

#[test]
fn exclusive_scan_covers_more_blocks_than_dispatch_limit() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let grid = 65535 * 256;
    let len = grid + 17;
    let input = runtime.zeros::<u32>(len).unwrap();
    runtime.write(&input, 0, &[u32::MAX]).unwrap();
    runtime.write(&input, 256, &[2]).unwrap();
    runtime.write(&input, grid - 1, &[5]).unwrap();
    let mut program = runtime.program();
    let output = program.exclusive_scan(&input).unwrap();
    let actual = program.submit_read(&output).unwrap().wait(TIMEOUT).unwrap();
    for (i, value) in actual.into_iter().enumerate() {
        let expected = match i {
            0 => 0,
            1..=256 => u32::MAX,
            _ if i < grid => 1,
            _ => 6,
        };
        assert_eq!(value, expected, "element {i}");
    }
}

#[test]
fn compact_is_stable_composable_and_clears_tail_on_every_run() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    for len in [0, 1, 255, 256, 257, 1023, 1024, 1025, 65537, 1048577] {
        let source_values: Vec<f32> = (0..len).map(|i| (i % 41) as f32 - 20.0).collect();
        let input = runtime.upload(&source_values).unwrap();
        let keep = runtime.zeros::<u32>(len).unwrap();
        let mut program = runtime.program();
        let mapped = program.affine(&input, 2.0, 1.0).unwrap();
        let selected = program.compact(&mapped, &keep).unwrap();
        let sum = program.sum(selected.values()).unwrap();
        let final_value = program.affine(&sum, 2.0, -1.0).unwrap();
        assert_eq!(selected.capacity(), len);
        let mut reads = Vec::new();
        for iteration in 0..4 {
            let masks: Vec<u32> = (0..len)
                .map(|i| match iteration {
                    0 => u32::MAX,
                    1 if i % 7 == 0 => 7,
                    2 if i % 3 == 1 => 0x8000_0000,
                    _ => 0,
                })
                .collect();
            runtime.write(&keep, 0, &masks).unwrap();
            // External changes to outputs cannot leave stale values or counts.
            runtime.write(selected.count(), 0, &[u32::MAX]).unwrap();
            let expected: Vec<f32> = source_values
                .iter()
                .zip(&masks)
                .filter_map(|(v, mask)| (*mask != 0).then_some(v * 2.0 + 1.0))
                .collect();
            let mut encoder = context.device.create_command_encoder(&Default::default());
            program.record(&mut encoder);
            let mut values_read = runtime
                .record_read(&mut encoder, selected.values())
                .unwrap();
            let mut count_read = runtime.record_read(&mut encoder, selected.count()).unwrap();
            let mut total_read = runtime.record_read(&mut encoder, &final_value).unwrap();
            let submission = context.queue.submit([encoder.finish()]);
            values_read.submitted(submission.clone());
            count_read.submitted(submission.clone());
            total_read.submitted(submission);
            reads.push((values_read, count_read, total_read, expected));
        }
        // Each submission's readback observes its own mask, even when awaited later.
        for (values_read, count_read, total_read, expected) in reads.into_iter().rev() {
            let values = values_read.wait(TIMEOUT).unwrap();
            assert_eq!(count_read.wait(TIMEOUT).unwrap(), [expected.len() as u32]);
            assert_eq!(&values[..expected.len()], &expected);
            assert!(
                values[expected.len()..].iter().all(|v| *v == 0.0),
                "stale compact tail at capacity {len}"
            );
            assert_eq!(
                total_read.wait(TIMEOUT).unwrap(),
                [expected.iter().sum::<f32>() * 2.0 - 1.0]
            );
        }
    }
}

#[test]
fn scan_and_compaction_reject_foreign_lengths_and_all_output_aliases() {
    use compute_core::gpu_compute::{GpuBuffer, wgpu::BufferUsages};
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let other = ComputeRuntime::new(&context).unwrap();
    let input = runtime.upload(&[1.0f32, 2.0, 3.0]).unwrap();
    let mask = runtime.upload(&[0u32, 7, 1]).unwrap();
    let output = runtime.zeros::<f32>(3).unwrap();
    let count = runtime.zeros::<u32>(1).unwrap();
    let foreign_mask = other.zeros::<u32>(3).unwrap();
    let foreign_values = other.zeros::<f32>(3).unwrap();
    let foreign_count = other.zeros::<u32>(1).unwrap();
    let mut program = runtime.program();
    assert!(matches!(
        program.exclusive_scan(&foreign_mask),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.exclusive_scan_into(&mask, &foreign_mask),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.exclusive_scan_into(&mask, &count),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        program.exclusive_scan_into(&mask, &mask.clone()),
        Err(ComputeError::AliasedOutput)
    ));
    assert!(matches!(
        program.compact(&input, &foreign_mask),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.compact(&foreign_values, &mask),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.compact(&input, &count),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        program.compact_into(&input, &mask, &output, &foreign_count),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.compact_into(&input, &mask, &foreign_values, &count),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.compact_into(&input, &mask, &input, &count),
        Err(ComputeError::AliasedOutput)
    ));
    assert!(matches!(
        program.compact_into(&input, &mask, &output.prefix(1).unwrap(), &count),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        program.compact_into(&input, &mask, &output, &mask),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        program.compact_into(&input, &mask, &output, &mask.prefix(1).unwrap()),
        Err(ComputeError::AliasedOutput)
    ));

    let shared = GpuBuffer::new(
        &context,
        12,
        BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
    )
    .unwrap();
    let shared_f32 = runtime.import_buffer::<f32>(shared.clone(), 3).unwrap();
    let shared_u32 = runtime.import_buffer::<u32>(shared.clone(), 3).unwrap();
    let shared_count = runtime.import_buffer::<u32>(shared, 1).unwrap();
    assert!(matches!(
        program.compact_into(&input, &shared_u32, &shared_f32, &count),
        Err(ComputeError::AliasedOutput)
    ));
    assert!(matches!(
        program.compact_into(&input, &mask, &shared_f32, &shared_count),
        Err(ComputeError::AliasedOutput)
    ));
    assert!(matches!(
        program.compact_into(&shared_f32, &mask, &output, &shared_count),
        Err(ComputeError::AliasedOutput)
    ));

    let selected = program
        .compact_into(&input, &mask, &output, &count)
        .unwrap();
    assert_eq!(
        program
            .submit_read(selected.values())
            .unwrap()
            .wait(TIMEOUT)
            .unwrap(),
        [2.0, 3.0, 0.0]
    );
    assert_eq!(
        runtime
            .read(selected.count())
            .unwrap()
            .wait(TIMEOUT)
            .unwrap(),
        [2]
    );
}

#[test]
fn comparison_masks_match_cpu_and_broadcast_on_either_side() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let av: Vec<f32> = (0..257).map(|i| (i % 17) as f32 - 8.0).collect();
    let bv: Vec<f32> = (0..257).map(|i| (i % 13) as f32 - 6.0).collect();
    let a = runtime.upload(&av).unwrap();
    let b = runtime.upload(&bv).unwrap();
    let scalar = runtime.upload(&[-0.0f32]).unwrap();
    for op in [
        CompareOp::Equal,
        CompareOp::NotEqual,
        CompareOp::Less,
        CompareOp::LessEqual,
        CompareOp::Greater,
        CompareOp::GreaterEqual,
    ] {
        for (left, right, left_values, right_values) in [
            (&a, &b, av.as_slice(), bv.as_slice()),
            (&a, &scalar, av.as_slice(), &[-0.0][..]),
            (&scalar, &b, &[-0.0][..], bv.as_slice()),
        ] {
            let mut program = runtime.program();
            let storage = runtime.upload(&vec![0xa5a5a5a5u32; av.len() + 7]).unwrap();
            let output = storage.prefix(av.len()).unwrap();
            program.compare_into(op, left, right, &output).unwrap();
            let actual = program
                .submit_read(&storage)
                .unwrap()
                .wait(TIMEOUT)
                .unwrap();
            for (i, value) in actual[..av.len()].iter().enumerate() {
                let lv = left_values[if left_values.len() == 1 { 0 } else { i }];
                let rv = right_values[if right_values.len() == 1 { 0 } else { i }];
                let expected = match op {
                    CompareOp::Equal => lv == rv,
                    CompareOp::NotEqual => lv != rv,
                    CompareOp::Less => lv < rv,
                    CompareOp::LessEqual => lv <= rv,
                    CompareOp::Greater => lv > rv,
                    CompareOp::GreaterEqual => lv >= rv,
                };
                assert_eq!(*value, u32::from(expected), "{op:?}({lv}, {rv}) at {i}");
            }
            assert_eq!(&actual[av.len()..], &[0xa5a5a5a5u32; 7]);
        }
    }
    let empty = runtime.zeros::<f32>(0).unwrap();
    let mut program = runtime.program();
    for (a, b) in [(&empty, &empty), (&scalar, &empty), (&empty, &scalar)] {
        let output = program.compare(CompareOp::Greater, a, b).unwrap();
        assert!(
            program
                .submit_read(&output)
                .unwrap()
                .wait(TIMEOUT)
                .unwrap()
                .is_empty()
        );
    }
    let singleton = program.compare(CompareOp::Equal, &scalar, &scalar).unwrap();
    assert_eq!(
        program
            .submit_read(&singleton)
            .unwrap()
            .wait(TIMEOUT)
            .unwrap(),
        [1]
    );
}

#[test]
fn gpu_comparison_compaction_and_sum_reuse_a_changing_threshold_without_mask_readback() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let values: Vec<f32> = (0..65537).map(|i| (i % 41) as f32 - 20.0).collect();
    let input = runtime.upload(&values).unwrap();
    let threshold = runtime.zeros::<f32>(1).unwrap();
    let mut program = runtime.program();
    let mapped = program.affine(&input, 2.0, 1.0).unwrap();
    let mask = program
        .compare(CompareOp::Greater, &mapped, &threshold)
        .unwrap();
    let selected = program.compact(&mapped, &mask).unwrap();
    let sum = program.sum(selected.values()).unwrap();
    let mut reads = Vec::new();
    for cutoff in [-100.0f32, 0.0, 100.0, -3.0] {
        runtime.write(&threshold, 0, &[cutoff]).unwrap();
        let expected = values
            .iter()
            .map(|value| value * 2.0 + 1.0)
            .filter(|value| *value > cutoff)
            .sum::<f32>();
        reads.push((program.submit_read(&sum).unwrap(), expected));
    }
    for (read, expected) in reads.into_iter().rev() {
        assert_eq!(read.wait(TIMEOUT).unwrap(), [expected]);
    }
}

#[test]
fn comparison_rejects_invalid_lengths_owners_and_cross_type_output_aliases() {
    use compute_core::gpu_compute::{GpuBuffer, wgpu::BufferUsages};
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let other = ComputeRuntime::new(&context).unwrap();
    let a = runtime.upload(&[1.0f32, 2.0, 3.0]).unwrap();
    let scalar = runtime.upload(&[2.0f32]).unwrap();
    let wrong_length = runtime.zeros::<f32>(2).unwrap();
    let foreign_input = other.zeros::<f32>(3).unwrap();
    let foreign_output = other.zeros::<u32>(3).unwrap();
    let wrong_output = runtime.zeros::<u32>(2).unwrap();
    let mut program = runtime.program();
    assert!(matches!(
        program.compare(CompareOp::Greater, &a, &wrong_length),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        program.compare(CompareOp::Greater, &foreign_input, &a),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.compare(CompareOp::Greater, &a, &foreign_input),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.compare_into(CompareOp::Greater, &a, &scalar, &foreign_output),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.compare_into(CompareOp::Greater, &a, &scalar, &wrong_output),
        Err(ComputeError::LengthMismatch { .. })
    ));
    let shared = GpuBuffer::new(
        &context,
        12,
        BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
    )
    .unwrap();
    let input = runtime.import_buffer::<f32>(shared.clone(), 3).unwrap();
    let output = runtime.import_buffer::<u32>(shared, 3).unwrap();
    assert!(matches!(
        program.compare_into(CompareOp::Equal, &input, &scalar, &output),
        Err(ComputeError::AliasedOutput)
    ));
    assert!(matches!(
        program.compare_into(CompareOp::Equal, &scalar, &input, &output),
        Err(ComputeError::AliasedOutput)
    ));
    let actual = program.compare(CompareOp::Greater, &a, &scalar).unwrap();
    assert_eq!(
        program.submit_read(&actual).unwrap().wait(TIMEOUT).unwrap(),
        [0, 0, 1]
    );
}

#[test]
fn four_item_scan_visits_multiple_blocks_per_workgroup_with_wrapping_and_normalization() {
    use compute_core::{Binding, Kernel, shaders, uniform_f32};
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let kernel = Kernel::new(
        &context.device,
        "scan grid-stride parity",
        shaders::SCAN_BLOCKS4_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
            Binding::StorageReadWrite,
        ],
    )
    .unwrap();
    let block_size = kernel.workgroup_size() as usize * 4;
    let len = block_size * 11 + 37;
    let blocks = len.div_ceil(block_size);
    let values: Vec<u32> = (0..len)
        .map(|i| match i % 4 {
            0 => u32::MAX,
            1 => 0,
            2 => 7,
            _ => i as u32,
        })
        .collect();
    let input = runtime.upload(&values).unwrap();
    for normalize in [false, true] {
        let output = runtime.upload(&vec![0xa5a5a5a5u32; len + 7]).unwrap();
        let totals = runtime.upload(&vec![0xa5a5a5a5u32; blocks + 7]).unwrap();
        let params = uniform_f32(
            &context.device,
            &context.queue,
            &[
                f32::from_bits(len as u32),
                f32::from_bits(3),
                f32::from_bits(u32::from(normalize)),
                0.,
            ],
        );
        let bindings = kernel.create_bind_group(
            &context.device,
            &[
                &params,
                input.view().raw(),
                output.view().raw(),
                totals.view().raw(),
            ],
        );
        let mut encoder = context.device.create_command_encoder(&Default::default());
        // Fewer dispatched groups than logical blocks forces repeated scratch
        // reuse inside each workgroup without a huge test-only allocation.
        kernel.record_dispatch(&mut encoder, &bindings, 3);
        let mut output_read = runtime.record_read(&mut encoder, &output).unwrap();
        let mut totals_read = runtime.record_read(&mut encoder, &totals).unwrap();
        let submission = context.queue.submit([encoder.finish()]);
        output_read.submitted(submission.clone());
        totals_read.submitted(submission);
        let actual = output_read.wait(TIMEOUT).unwrap();
        let totals = totals_read.wait(TIMEOUT).unwrap();
        for (block, &block_total) in totals.iter().take(blocks).enumerate() {
            let mut running = 0u32;
            for i in block * block_size..((block + 1) * block_size).min(len) {
                assert_eq!(
                    actual[i], running,
                    "block {block}, index {i}, normalize {normalize}"
                );
                let value = if normalize {
                    u32::from(values[i] != 0)
                } else {
                    values[i]
                };
                running = running.wrapping_add(value);
            }
            assert_eq!(block_total, running);
        }
        assert_eq!(&actual[len..], &[0xa5a5a5a5; 7]);
        assert_eq!(&totals[blocks..], &[0xa5a5a5a5; 7]);
    }
}
