//! Paired existing per-operation chain vs a generated fused shader, with full
//! output readback. CPU reference performs the same arithmetic in a single loop.
//! cargo run --release --offline -p compute-core --example bench_fusion
use compute_core::{ComputeRuntime, FusionGraph, UnaryOp, gpu_compute::GpuContext};
use std::time::{Duration, Instant};
fn reference(input: &[f32]) -> Vec<f32> {
    input
        .iter()
        .map(|x| {
            let a = (x * 0.5 + 0.25).sin();
            let b = (a * a * 0.75 + 0.1).cos();
            (b * b * 0.25 - 0.2).sin().abs()
        })
        .collect()
}
fn check(a: &[f32], expected: &[f32]) {
    assert_eq!(a.len(), expected.len());
    assert!(a.iter().zip(expected).all(|(a, b)| (a - b).abs() < 3e-6));
}
fn median_ms(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2] * 1000.0
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = GpuContext::new().ok_or("GPU adapter required")?;
    let rt = ComputeRuntime::new(&context)?;
    let mut graph = FusionGraph::new(1);
    let x = graph.input(0)?;
    let a = graph.affine(&x, 0.5, 0.25)?;
    let a = graph.unary(UnaryOp::Sin, &a)?;
    let a = graph.unary(UnaryOp::Square, &a)?;
    let b = graph.affine(&a, 0.75, 0.1)?;
    let b = graph.unary(UnaryOp::Cos, &b)?;
    let b = graph.unary(UnaryOp::Square, &b)?;
    let c = graph.affine(&b, 0.25, -0.2)?;
    let c = graph.unary(UnaryOp::Sin, &c)?;
    let c = graph.unary(UnaryOp::Abs, &c)?;
    let start = Instant::now();
    let kernel = graph.compile(&context, &[c])?;
    println!(
        "backend={}, operations={}, compile_ms={:.3}; wall time includes submission, wait and full readback; 3 warmups, 15 rotated samples",
        context.backend_label(),
        kernel.operation_count(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    for n in [4097, 1_000_003] {
        let values: Vec<f32> = (0..n).map(|i| (i % 251) as f32 * 0.0078125 - 1.0).collect();
        let expected = reference(&values);
        let input = rt.upload(&values)?;
        let mut old = rt.program();
        let a = old.affine(&input, 0.5, 0.25)?;
        let a = old.unary(UnaryOp::Sin, &a)?;
        let a = old.unary(UnaryOp::Square, &a)?;
        let b = old.affine(&a, 0.75, 0.1)?;
        let b = old.unary(UnaryOp::Cos, &b)?;
        let b = old.unary(UnaryOp::Square, &b)?;
        let c = old.affine(&b, 0.25, -0.2)?;
        let c = old.unary(UnaryOp::Sin, &c)?;
        let old_output = old.unary(UnaryOp::Abs, &c)?;
        let mut fused = rt.program();
        let out = fused.fused(&kernel, &[&input], n)?;
        let run = |path| -> Result<Vec<f32>, Box<dyn std::error::Error>> {
            if path == 4 {
                return Ok(reference(std::hint::black_box(&values)));
            }
            if path >= 2 {
                rt.write(&input, 0, &values)?;
            }
            Ok(if path % 2 == 0 {
                old.submit_read(&old_output)?
            } else {
                fused.submit_read(&out[0])?
            }
            .wait(Duration::from_secs(10))?)
        };
        for i in 0..3 {
            for j in 0..5 {
                check(&run((i + j) % 5)?, &expected);
            }
        }
        let mut samples: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::new());
        for i in 0..15 {
            for j in 0..5 {
                let path = (i + j) % 5;
                let start = Instant::now();
                let got = run(path)?;
                samples[path].push(start.elapsed().as_secs_f64());
                check(&got, &expected);
                std::hint::black_box(got);
            }
        }
        let [old, fused, old_upload, fused_upload, cpu] = samples.map(median_ms);
        println!(
            "n={n} resident_chain_ms={old:.3} resident_fused_ms={fused:.3} upload_chain_ms={old_upload:.3} upload_fused_ms={fused_upload:.3} cpu_ms={cpu:.3} dispatches=9->1 temporary_bytes={}->0 readback_bytes={}",
            n * 4 * 8,
            n * 4
        );
    }
    Ok(())
}
