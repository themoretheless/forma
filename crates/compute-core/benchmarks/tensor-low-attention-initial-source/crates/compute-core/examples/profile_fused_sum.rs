//! GPU query timestamps for real prepared map+sum and fused map/reduce programs.
//! Opt-in profiling keeps GPU elapsed time separate from wall-clock latency.
use compute_core::{
    BinaryOp, ComputeRuntime, FusionGraph, UnaryOp,
    gpu_compute::{GpuContext, GpuTimer},
};
use std::time::{Duration, Instant};
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let c = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&c)?;
    let timer = GpuTimer::new(&c)?;
    println!(
        "backend={} timestamp_period_ns={} 200ms warmup,31 paired samples; GPU timestamps include compute pass only; wall includes scalar readback and a separate post-completion timestamp resolve submission",
        c.backend_label(),
        timer.timestamp_period_ns()
    );
    let mut g = FusionGraph::new(2);
    let x = g.input(0)?;
    let y = g.input(1)?;
    let difference = g.binary(BinaryOp::Subtract, &x, &y)?;
    let square = g.unary(UnaryOp::Square, &difference)?;
    let map_kernel = g.compile(&c, std::slice::from_ref(&square))?;
    let sum_kernel = g.compile_sum(&c, &square)?;
    for n in [4097, 1_000_003, 4_000_003] {
        let a: Vec<f32> = (0..n).map(|i| (i % 127) as f32 / 128.0).collect();
        let b: Vec<f32> = (0..n).map(|i| (i % 31) as f32 / 64.0).collect();
        let expected = a
            .iter()
            .zip(&b)
            .map(|(&x, &y)| f64::from(x - y).powi(2))
            .sum::<f64>();
        let a = rt.upload(&a)?;
        let b = rt.upload(&b)?;
        let mut map = rt.program();
        let mapped = map.fused(&map_kernel, &[&a, &b], n)?;
        let ordinary_sum = map.sum(&mapped[0])?;
        let mut fused = rt.program();
        let fused_sum = fused.fused_sum(&sum_kernel, &[&a, &b], n)?;
        let run = |path| -> Result<(f64, f64), Box<dyn std::error::Error>> {
            let wall = Instant::now();
            let mut encoder = c.device.create_command_encoder(&Default::default());
            let program = if path == 0 { &map } else { &fused };
            let output = if path == 0 { &ordinary_sum } else { &fused_sum };
            let mut time = timer.record_compute(&mut encoder, "map/reduce", |pass| {
                program.record_in_pass(pass)
            })?;
            let mut read = rt.record_read(&mut encoder, output)?;
            let submission = c.queue.submit([timer.finish(encoder)?]);
            time.submitted(submission.clone());
            read.submitted(submission);
            let value = read.wait(Duration::from_secs(10))?[0];
            let timestamp = time.wait(Duration::from_secs(10))?;
            let wall_ms = wall.elapsed().as_secs_f64() * 1000.0;
            assert!((f64::from(value) - expected).abs() < 2e-6 * expected.max(1.0));
            Ok((timestamp.elapsed_ns / 1e6, wall_ms))
        };
        let warm = Instant::now();
        while warm.elapsed() < Duration::from_millis(200) {
            run(0)?;
            run(1)?;
        }
        let mut gpu: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::new());
        let mut host: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::new());
        for i in 0..31 {
            for j in 0..2 {
                let path = (i + j) % 2;
                let (g, h) = run(path)?;
                gpu[path].push(g);
                host[path].push(h);
            }
        }
        println!(
            "n={n} map_gpu_ms={:.6} fused_gpu_ms={:.6} map_wall_ms={:.6} fused_wall_ms={:.6}",
            median(gpu[0].clone()),
            median(gpu[1].clone()),
            median(host[0].clone()),
            median(host[1].clone())
        );
        println!("gpu_samples_ms={gpu:?}\nwall_samples_ms={host:?}");
    }
    Ok(())
}
