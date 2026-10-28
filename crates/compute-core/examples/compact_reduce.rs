//! Reusable GPU selection -> reduction with only scalar readback.
//! cargo run --release --offline -p compute-core --example compact_reduce
use compute_core::{CompareOp, ComputeRuntime, gpu_compute::GpuContext};
use std::time::{Duration, Instant};

fn reference(values: &[f32], threshold: f32) -> (u32, f64) {
    values
        .iter()
        .filter(|&&value| value > threshold)
        .fold((0, 0.0), |(n, sum), &value| (n + 1, sum + f64::from(value)))
}
fn check(actual: (u32, f64), expected: (u32, f64)) {
    assert_eq!(actual.0, expected.0);
    assert!((actual.1 - expected.1).abs() <= 2e-6 * expected.1.abs().max(1.0));
}
fn median_ms(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2] * 1000.0
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = GpuContext::new().ok_or("GPU adapter required")?;
    let runtime = ComputeRuntime::new(&context)?;
    println!(
        "backend: {}; times include host submission, waits and scalar readback",
        context.backend_label()
    );
    for n in [4097, 1_000_003] {
        let values: Vec<f32> = (0..n).map(|i| ((i % 251) as f32 - 125.0) * 0.125).collect();
        let threshold = 0.0f32;
        let expected = reference(&values, threshold);
        let input = runtime.upload(&values)?;
        let threshold_gpu = runtime.upload(&[threshold])?;
        let prepare = Instant::now();
        let mut plan = runtime.program();
        let mask = plan.compare(CompareOp::Greater, &input, &threshold_gpu)?;
        let selected = plan.compact(&input, &mask)?;
        let sum = plan.sum(selected.values())?;
        let prepare_ms = prepare.elapsed().as_secs_f64() * 1000.0;
        let execute = || -> Result<(u32, f64), Box<dyn std::error::Error>> {
            let mut encoder = context.device.create_command_encoder(&Default::default());
            plan.record(&mut encoder);
            let mut count_read = runtime.record_read(&mut encoder, selected.count())?;
            let mut sum_read = runtime.record_read(&mut encoder, &sum)?;
            let submission = context.queue.submit([encoder.finish()]);
            count_read.submitted(submission.clone());
            sum_read.submitted(submission);
            let count = count_read.wait(Duration::from_secs(10))?[0];
            let sum = sum_read.wait(Duration::from_secs(10))?[0];
            Ok((count, f64::from(sum)))
        };
        for _ in 0..3 {
            check(execute()?, expected);
        }
        let mut samples = [Vec::new(), Vec::new(), Vec::new()];
        for iteration in 0..9 {
            for offset in 0..3 {
                let path = (iteration + offset) % 3;
                let start = Instant::now();
                let got = match path {
                    0 => execute()?,
                    1 => {
                        runtime.write(&input, 0, &values)?;
                        runtime.write(&threshold_gpu, 0, &[threshold])?;
                        execute()?
                    }
                    _ => reference(
                        std::hint::black_box(&values),
                        std::hint::black_box(threshold),
                    ),
                };
                samples[path].push(start.elapsed().as_secs_f64());
                check(got, expected);
                std::hint::black_box(got);
            }
        }
        let [resident, upload, cpu] = samples;
        println!(
            "n={n}, kept={}: plan {:.3} ms; resident GPU {:.3} ms; upload + GPU {:.3} ms; CPU selected sum {:.3} ms; readback=8 bytes",
            expected.0,
            prepare_ms,
            median_ms(resident),
            median_ms(upload),
            median_ms(cpu)
        );
    }
    Ok(())
}
