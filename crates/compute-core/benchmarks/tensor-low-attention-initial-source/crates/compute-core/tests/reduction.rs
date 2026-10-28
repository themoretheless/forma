use compute_core::{ComputeError, ComputeRuntime, gpu_compute::GpuContext};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);
fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    if context.is_none() {
        assert!(
            std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
            "required GPU unavailable"
        );
    }
    context
}
fn assert_sum(actual: f32, values: &[f32]) {
    let expected = values.iter().map(|&v| f64::from(v)).sum::<f64>();
    let norm = values.iter().map(|&v| f64::from(v).abs()).sum::<f64>();
    // A forward-error bound scaled by the input norm remains meaningful for
    // cancellation near zero. This is f32 arithmetic, not compensated summation.
    let tolerance = 2e-6 * norm.max(1.0);
    assert!(
        actual.is_finite() && (f64::from(actual) - expected).abs() <= tolerance,
        "sum {actual}, f64 {expected}, norm {norm}, tolerance {tolerance}"
    );
}
#[test]
fn arbitrary_lengths_and_unpadded_scalar_buffers_match_f64() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for n in [
        0, 1, 2, 3, 4, 5, 255, 256, 257, 4095, 4096, 4097, 65535, 65536, 65537, 1_000_003,
        4_000_003, 16_000_003,
    ] {
        let values: Vec<f32> = (0..n)
            .map(|i| ((i * 31 % 127) as f32 - 63.0) / 128.0)
            .collect();
        let input = rt.upload(&values).unwrap();
        // The logical input has its actual byte size: no vec4 padding contract.
        assert_eq!(input.view().raw().size(), (n.max(1) * 4) as u64);
        let mut program = rt.program();
        let output = program.sum(&input).unwrap();
        let actual = program.submit_read(&output).unwrap().wait(TIMEOUT).unwrap()[0];
        // These bounded dyadic values are exact under both tested schedules;
        // unlike a norm-based bound, equality catches a lost final element.
        let expected = values.iter().map(|&v| f64::from(v)).sum::<f64>();
        assert_eq!(f64::from(actual), expected, "length {n}");
    }
}
#[test]
fn cancellation_and_dynamic_range_use_f64_reference() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for pattern in [
        [1000.0, -1000.0, 0.0625, 0.125],
        [1e6, 0.25, -1e6, 0.125],
        [1e-8, -1e-8, 1e-10, 0.0],
        [1e8, 1.0, -1e8, 1.0],
    ] {
        let values: Vec<f32> = (0..1_000_003).map(|i| pattern[i % 4]).collect();
        let input = rt.upload(&values).unwrap();
        let mut program = rt.program();
        let output = program.sum(&input).unwrap();
        let actual = program.submit_read(&output).unwrap().wait(TIMEOUT).unwrap()[0];
        assert_sum(actual, &values);
    }
}
#[test]
fn reused_program_overwrites_output_and_respects_prefix_sentinels() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for n in [0, 1, 3, 4097, 1_000_003] {
        let input_storage = rt.zeros::<f32>(n + 7).unwrap();
        rt.write(&input_storage, n, &[123456.0; 7]).unwrap();
        let input = input_storage.prefix(n).unwrap();
        let result_storage = rt.upload(&[-7.0f32, 54321.0, 54321.0]).unwrap();
        let result = result_storage.prefix(1).unwrap();
        let mut program = rt.program();
        let mapped = program.affine(&input, 2.0, -1.0).unwrap();
        program.sum_into(&mapped, &result).unwrap();
        let consumed = program.affine(&result, 0.5, 1.0).unwrap();
        for step in 0..3 {
            let values: Vec<f32> = (0..n).map(|i| ((i + step) % 13) as f32).collect();
            rt.write(&input, 0, &values).unwrap();
            rt.write(&result, 0, &[-99.0]).unwrap();
            let actual = program
                .submit_read(&consumed)
                .unwrap()
                .wait(TIMEOUT)
                .unwrap()[0];
            let expected = values
                .iter()
                .map(|&v| f64::from(2.0 * v - 1.0))
                .sum::<f64>()
                * 0.5
                + 1.0;
            assert!((f64::from(actual) - expected).abs() <= 2e-6 * expected.abs().max(1.0));
        }
        assert_eq!(
            &rt.read(&result_storage).unwrap().wait(TIMEOUT).unwrap()[1..],
            &[54321.0; 2]
        );
        assert_eq!(
            &rt.read(&input_storage).unwrap().wait(TIMEOUT).unwrap()[n..],
            &[123456.0; 7]
        );
    }
}
#[test]
fn invalid_sum_outputs_leave_program_usable() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let other = ComputeRuntime::new(&ctx).unwrap();
    let input = rt.upload(&[1.0f32, 2.0, 3.0]).unwrap();
    let output = rt.zeros::<f32>(1).unwrap();
    let foreign = other.zeros::<f32>(1).unwrap();
    let mut program = rt.program();
    assert!(matches!(
        program.sum_into(&foreign, &output),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.sum_into(&input, &foreign),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.sum_into(&input, &input.prefix(1).unwrap()),
        Err(ComputeError::AliasedOutput)
    ));
    for n in [0, 2] {
        assert!(matches!(
            program.sum_into(&input, &rt.zeros(n).unwrap()),
            Err(ComputeError::LengthMismatch { .. })
        ));
    }
    program.sum_into(&input, &output).unwrap();
    assert_eq!(
        program.submit_read(&output).unwrap().wait(TIMEOUT).unwrap(),
        [6.0]
    );
}

#[test]
fn raw_reduction_retains_custom_workgroups_and_composes_in_one_pass() {
    use compute_core::{Binding, Kernel, Reduction, shaders::BLOCK_SUM_WGSL};
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for (wg, n) in [(1, 257), (64, 1_000_003)] {
        let kernel = Kernel::with_workgroup_size(
            rt.device(),
            "raw sum",
            BLOCK_SUM_WGSL,
            "main",
            &[
                Binding::Uniform,
                Binding::StorageRead,
                Binding::StorageReadWrite,
            ],
            wg,
        )
        .unwrap();
        let input = rt.zeros::<f32>(n + 3).unwrap();
        rt.write(&input, n, &[99999.0; 3]).unwrap();
        let output = rt.upload(&[-1.0f32, 88888.0, 88888.0]).unwrap();
        let plan = Reduction::with_output(
            rt.device(),
            rt.queue(),
            &kernel,
            input.view().raw(),
            n as u32,
            output.view().raw(),
        );
        for (iteration, use_existing_pass) in [false, true].into_iter().enumerate() {
            let values: Vec<f32> = (0..n).map(|i| ((i + iteration) % 7) as f32).collect();
            rt.write(&input, 0, &values).unwrap();
            let mut encoder = rt.device().create_command_encoder(&Default::default());
            if use_existing_pass {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                plan.record_in_pass(&mut pass);
            } else {
                plan.record(&mut encoder);
            }
            let mut read = rt.record_read(&mut encoder, &output).unwrap();
            read.submitted(rt.queue().submit([encoder.finish()]));
            let actual = read.wait(TIMEOUT).unwrap();
            assert_sum(actual[0], &values);
            assert_eq!(&actual[1..], &[88888.0; 2]);
        }
    }
}
