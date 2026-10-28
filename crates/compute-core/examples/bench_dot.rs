//! Default dot versus materialized products, with frozen and current sum plans.
//! Resident inputs; GPU timestamps and unprofiled scalar readback are separate.
use compute_core::{
    BinaryOp, Binding, ComputeBatch, ComputeRuntime, Kernel,
    gpu_compute::{GpuContext, GpuTimer},
    uniform_f32, wgpu,
};
use std::time::{Duration, Instant};

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

fn report(path: &str, mode: &str, samples: &[f64]) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    println!(
        "path={path} mode={mode} median_ms={:.6} p90_ms={:.6} samples_ms={samples:?}",
        sorted[sorted.len() / 2],
        sorted[(sorted.len() * 9).div_ceil(10) - 1]
    );
}

fn main() -> AnyResult<()> {
    let context = GpuContext::with_timestamps()?;
    let runtime = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    let frozen_kernel = Kernel::new(
        &context.device,
        "frozen dot sum baseline",
        include_str!("reduction_candidates/baseline.wgsl"),
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
    )?;
    println!(
        "backend={} timestamp_period_ns={} 200ms warmup per mode,31 rotated samples; resident full arrays; host mode has no timestamp queries and reads4bytes; GPU mode includes all compute stages only; preparation and uploads excluded",
        context.backend_label(),
        timer.timestamp_period_ns()
    );
    for n in [1_usize, 4097, 1_000_003, 4_000_003] {
        let a: Vec<f32> = (0..n).map(|i| (i % 127) as f32 / 128.0 - 0.4).collect();
        let b: Vec<f32> = (0..n).map(|i| (i % 31) as f32 / 32.0 - 0.1).collect();
        let expected: f64 = a
            .iter()
            .zip(&b)
            .map(|(&x, &y)| f64::from(x) * f64::from(y))
            .sum();
        let magnitude: f64 = a
            .iter()
            .zip(&b)
            .map(|(&x, &y)| (f64::from(x) * f64::from(y)).abs())
            .sum();
        let a = runtime.upload(&a)?;
        let b = runtime.upload(&b)?;
        let mut map = runtime.program();
        let products = map.binary(BinaryOp::Multiply, &a, &b)?;
        let frozen_output = runtime.zeros::<f32>(1)?;
        // Freeze the previous schedule as well as its shader so later changes
        // to the public Reduction helper cannot silently alter this baseline.
        let mut frozen_sum = ComputeBatch::new();
        let mut source = products.view().raw().clone();
        let mut count = n as u32;
        loop {
            let groups = count.div_ceil(256).clamp(1, 65535);
            let destination = if groups == 1 {
                frozen_output.view().raw().clone()
            } else {
                context.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("frozen sum partials"),
                    size: u64::from(groups) * 4,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            };
            let params = uniform_f32(
                &context.device,
                &context.queue,
                &[f32::from_bits(count), f32::from_bits(groups), 0.0, 0.0],
            );
            let bindings =
                frozen_kernel.create_bind_group(&context.device, &[&params, &source, &destination]);
            frozen_sum.push(&frozen_kernel, &bindings, groups);
            if groups == 1 {
                break;
            }
            source = destination;
            count = groups;
        }
        let mut current_sum = runtime.program();
        let current_output = current_sum.sum(&products)?;
        let started = Instant::now();
        let mut dot = runtime.program();
        let dot_output = dot.dot(&a, &b)?;
        println!(
            "n={n} dot_prepare_ms={:.6} first_dot={} removed_products_bytes={}",
            started.elapsed().as_secs_f64() * 1e3,
            n == 1,
            n * 4
        );
        let outputs = [&frozen_output, &current_output, &dot_output];
        let names = ["map_frozen_sum", "map_current_sum", "default_dot"];
        let record = |path: usize, pass: &mut wgpu::ComputePass<'_>| match path {
            0 => {
                map.record_in_pass(pass);
                frozen_sum.record_in_pass(pass);
            }
            1 => {
                map.record_in_pass(pass);
                current_sum.record_in_pass(pass);
            }
            _ => dot.record_in_pass(pass),
        };
        let run = |path: usize, profiled: bool| -> AnyResult<f64> {
            let started = Instant::now();
            let mut encoder = context.device.create_command_encoder(&Default::default());
            let mut timing = if profiled {
                Some(timer.record_compute(&mut encoder, names[path], |pass| record(path, pass))?)
            } else {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                record(path, &mut pass);
                None
            };
            let mut read = runtime.record_read(&mut encoder, outputs[path])?;
            let commands = if profiled {
                timer.finish(encoder)?
            } else {
                encoder.finish()
            };
            let submission = context.queue.submit([commands]);
            if let Some(ticket) = &mut timing {
                ticket.submitted(submission.clone());
            }
            read.submitted(submission);
            let result = read.wait(Duration::from_secs(20))?[0];
            let wall_ms = started.elapsed().as_secs_f64() * 1e3;
            assert!(
                (f64::from(result) - expected).abs() <= 3e-6 * magnitude.max(1.0),
                "{} n={n}: {result} != {expected}",
                names[path]
            );
            Ok(if let Some(ticket) = timing {
                ticket.wait(Duration::from_secs(20))?.elapsed_ns / 1e6
            } else {
                wall_ms
            })
        };
        for profiled in [false, true] {
            let warmup = Instant::now();
            while warmup.elapsed() < Duration::from_millis(200) {
                for path in 0..3 {
                    run(path, profiled)?;
                }
            }
            let mut samples: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::new());
            for i in 0..31 {
                for j in 0..3 {
                    let path = (i + j) % 3;
                    samples[path].push(run(path, profiled)?);
                }
            }
            for path in 0..3 {
                report(
                    names[path],
                    if profiled { "gpu" } else { "host" },
                    &samples[path],
                );
            }
        }
    }
    Ok(())
}
