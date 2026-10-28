use compute_core::{BinaryOp, CompareOp, ComputeRuntime, UnaryOp, gpu_compute::GpuContext};
use std::time::Duration;

#[test]
fn shared_pass_orders_reused_outputs_scan_and_reduction() {
    let Some(context) = GpuContext::new() else {
        assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
        return;
    };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let count = 513;
    let input = runtime.zeros::<f32>(count).unwrap();
    let first = runtime.zeros::<f32>(count).unwrap();
    let second = runtime.zeros::<f32>(count).unwrap();
    let mut program = runtime.program();
    program.affine_into(&input, 2.0, 1.0, &first).unwrap();
    program
        .unary_into(UnaryOp::Square, &first, &second)
        .unwrap();
    // Reuse a buffer that was read by the preceding dispatch. Its old contents
    // must remain visible until that dispatch finishes, then be overwritten.
    program
        .binary_into(BinaryOp::Add, &second, &input, &first)
        .unwrap();
    let total = program.sum(&input).unwrap();
    let mean = program.affine(&total, 1.0 / count as f32, 0.0).unwrap();
    let keep = program.compare(CompareOp::Greater, &first, &mean).unwrap();
    let offsets = program.exclusive_scan(&keep).unwrap();
    let compacted = program.compact(&first, &keep).unwrap();
    let selected_sum = program.sum(compacted.values()).unwrap();
    let mut pending = Vec::new();
    for shift in [0, 2, 5] {
        let values: Vec<f32> = (0..count).map(|i| ((i + shift) % 7) as f32 - 3.0).collect();
        let mean = values.iter().sum::<f32>() * (1.0 / count as f32);
        let transformed: Vec<f32> = values
            .iter()
            .map(|&v| (2.0 * v + 1.0).powi(2) + v)
            .collect();
        let selected: Vec<f32> = transformed.iter().copied().filter(|&v| v > mean).collect();
        let mut n = 0u32;
        let expected_offsets: Vec<u32> = transformed
            .iter()
            .map(|&v| {
                let offset = n;
                n += u32::from(v > mean);
                offset
            })
            .collect();
        runtime.write(&input, 0, &values).unwrap();
        let mut encoder = context.device.create_command_encoder(&Default::default());
        program.record(&mut encoder);
        let mut offsets_read = runtime.record_read(&mut encoder, &offsets).unwrap();
        let mut count_read = runtime
            .record_read(&mut encoder, compacted.count())
            .unwrap();
        let mut sum_read = runtime.record_read(&mut encoder, &selected_sum).unwrap();
        let submission = context.queue.submit([encoder.finish()]);
        offsets_read.submitted(submission.clone());
        count_read.submitted(submission.clone());
        sum_read.submitted(submission);
        pending.push((
            offsets_read,
            count_read,
            sum_read,
            expected_offsets,
            n,
            selected.iter().sum::<f32>(),
        ));
    }
    for (offsets, count, sum, expected_offsets, expected_count, expected_sum) in
        pending.into_iter().rev()
    {
        assert_eq!(
            offsets.wait(Duration::from_secs(10)).unwrap(),
            expected_offsets
        );
        assert_eq!(
            count.wait(Duration::from_secs(10)).unwrap(),
            [expected_count]
        );
        assert_eq!(sum.wait(Duration::from_secs(10)).unwrap(), [expected_sum]);
    }
}
