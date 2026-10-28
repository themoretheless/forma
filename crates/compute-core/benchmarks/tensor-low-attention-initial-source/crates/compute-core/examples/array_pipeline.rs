//! Typed GPU pipeline: center an array, then normalize it to unit L2 norm.
//! cargo run --release --offline -p compute-core --example array_pipeline
use compute_core::{BinaryOp, ComputeRuntime, UnaryOp, gpu_compute::GpuContext};
use std::time::{Duration, Instant};

fn reference(input: &[f32]) -> Vec<f32> {
    let mean = (input.iter().map(|&x| f64::from(x)).sum::<f64>() / input.len() as f64) as f32;
    let mut output: Vec<f32> = input.iter().map(|v| v - mean).collect();
    let norm = output
        .iter()
        .map(|&v| f64::from(v).powi(2))
        .sum::<f64>()
        .sqrt() as f32;
    for value in &mut output {
        *value /= norm;
    }
    output
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2] * 1000.0
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = GpuContext::new().ok_or("GPU adapter required")?;
    let runtime = ComputeRuntime::new(&context)?;
    println!("backend: {}", context.backend_label());
    for n in [4097, 1_000_003] {
        let values: Vec<f32> = (0..n).map(|i| ((i % 31) as f32 - 15.0) * 0.125).collect();
        let input = runtime.upload(&values)?;
        let mut program = runtime.program();
        let total = program.sum(&input)?;
        let mean = program.affine(&total, 1.0 / n as f32, 0.0)?;
        let centered = program.binary(BinaryOp::Subtract, &input, &mean)?;
        let squared_norm = program.dot(&centered, &centered)?;
        let norm = program.unary(UnaryOp::Sqrt, &squared_norm)?;
        let output = program.binary(BinaryOp::Divide, &centered, &norm)?;
        let timeout = Duration::from_secs(10);
        for _ in 0..3 {
            program.submit_read(&output)?.wait(timeout)?;
        }
        let expected = reference(&values);
        let mut resident = Vec::new();
        let mut uploaded = Vec::new();
        let mut cpu = Vec::new();
        let mut max_error = 0.0f32;
        // Rotate timing order to avoid always favoring the same path.
        // GPU samples include full readback; uploaded includes input packing.
        for iteration in 0..9 {
            for offset in 0..3 {
                let path = (iteration + offset) % 3;
                let start = Instant::now();
                let actual = match path {
                    0 => program.submit_read(&output)?.wait(timeout)?,
                    1 => {
                        runtime.write(&input, 0, &values)?;
                        program.submit_read(&output)?.wait(timeout)?
                    }
                    _ => reference(std::hint::black_box(&values)),
                };
                let elapsed = start.elapsed().as_secs_f64();
                match path {
                    0 => resident.push(elapsed),
                    1 => uploaded.push(elapsed),
                    _ => cpu.push(elapsed),
                }
                for (&actual, &expected) in actual.iter().zip(&expected) {
                    max_error = max_error.max((actual - expected).abs());
                    assert!((actual - expected).abs() <= 2e-5 * expected.abs().max(1e-4));
                }
                std::hint::black_box(actual);
            }
        }
        println!(
            "n={n}: median GPU resident + full readback {:.3} ms; upload + GPU + readback {:.3} ms; CPU (f64 sums) {:.3} ms; max abs error {max_error:.3e}",
            median(resident),
            median(uploaded),
            median(cpu)
        );
    }
    Ok(())
}
