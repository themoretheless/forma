//! Controlled GEMM candidate study. Raw samples are printed for every path.
//! cargo run --release --offline -p compute-core --example bench_matmul_candidates
use compute_core::{
    Binding, ComputeRuntime, Kernel, MatrixView, gpu_compute::GpuContext, uniform_f32,
};
use std::time::{Duration, Instant};

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Copy)]
enum Schedule {
    Tiles,
    Scalar,
    Vector,
    Dot,
}
struct Candidate {
    name: &'static str,
    kernel: Kernel,
    schedule: Schedule,
    aligned: bool,
}

fn reference(a: &[f32], b: &[f32], m: usize, k: usize, n: usize) -> Vec<f64> {
    let mut output = vec![0.0; m * n];
    for row in 0..m {
        for inner in 0..k {
            let av = f64::from(a[row * k + inner]);
            for (out, &bv) in output[row * n..(row + 1) * n]
                .iter_mut()
                .zip(&b[inner * n..(inner + 1) * n])
            {
                *out += av * f64::from(bv);
            }
        }
    }
    output
}

fn check(actual: &[f32], expected: &[f64], k: usize) -> f64 {
    assert_eq!(actual.len(), expected.len());
    let error = actual
        .iter()
        .zip(expected)
        .map(|(&a, &b)| (f64::from(a) - b).abs())
        .fold(0.0, f64::max);
    assert!(error < 2e-5 * k.max(1) as f64, "max absolute error {error}");
    error
}

fn main() -> AnyResult<()> {
    let context = GpuContext::new().ok_or("GPU adapter required")?;
    let runtime = ComputeRuntime::new(&context)?;
    let bindings = [
        Binding::Uniform,
        Binding::StorageRead,
        Binding::StorageRead,
        Binding::StorageReadWrite,
    ];
    let candidates = [
        (
            "baseline32",
            include_str!("matmul_candidates/baseline32.wgsl"),
            Schedule::Tiles,
            false,
        ),
        (
            "direct",
            include_str!("matmul_candidates/naive.wgsl"),
            Schedule::Scalar,
            false,
        ),
        (
            "aligned32",
            include_str!("matmul_candidates/aligned32.wgsl"),
            Schedule::Tiles,
            true,
        ),
        (
            "vector32",
            include_str!("matmul_candidates/vector32.wgsl"),
            Schedule::Tiles,
            true,
        ),
        (
            "vector_direct",
            include_str!("matmul_candidates/vector_direct.wgsl"),
            Schedule::Vector,
            false,
        ),
        (
            "cooperative_dot",
            include_str!("matmul_candidates/cooperative_dot.wgsl"),
            Schedule::Dot,
            false,
        ),
    ]
    .into_iter()
    .map(|(name, source, schedule, aligned)| {
        Ok(Candidate {
            name,
            kernel: Kernel::new(&context.device, name, source, "main", &bindings)?,
            schedule,
            aligned,
        })
    })
    .collect::<Result<Vec<_>, compute_core::KernelError>>()?;
    println!(
        "backend={}; 4 warmups plus at least 250ms steady work, 17 rotated samples; each GPU path uses one recorded dispatch + full output readback + wait; setup/uploads excluded; independent f64 parity before timing; milliseconds; p90=nearest-rank",
        context.backend_label()
    );
    let extended = std::env::args().any(|arg| arg == "--extended");
    let shapes = if extended {
        vec![
            (32, 32, 32),
            (64, 64, 64),
            (127, 259, 193),
            (255, 129, 63),
            (511, 63, 257),
            (256, 256, 256),
            (512, 512, 512),
            (1024, 1024, 1024),
            (256, 1024, 256),
            (1024, 64, 256),
            (4096, 256, 32),
            (4096, 16, 16),
            (4096, 256, 8),
            (16, 512, 4096),
            (65537, 1, 1),
            (1, 512, 1),
            (8, 256, 8),
            (16, 256, 16),
            (32, 256, 32),
            (32, 1024, 32),
            (16, 2048, 64),
            (64, 256, 64),
            (8, 2048, 8),
        ]
    } else {
        vec![
            (64usize, 64usize, 64usize),
            (127, 259, 193),
            (256, 256, 256),
            (512, 512, 512),
            (1024, 1024, 1024),
            (4096, 16, 16),
            (4096, 256, 8),
            (16, 512, 4096),
            (8, 2048, 8),
        ]
    };
    for (m, k, n) in shapes {
        let av: Vec<f32> = (0..m * k)
            .map(|i| ((i * 17 % 73) as f32 - 36.0) / 17.0)
            .collect();
        let bv: Vec<f32> = (0..k * n)
            .map(|i| ((i * 7 % 41) as f32 - 20.0) / 13.0)
            .collect();
        let expected = reference(&av, &bv, m, k, n);
        let a = runtime.upload(&av)?;
        let b = runtime.upload(&bv)?;
        let mut plans = Vec::new();
        for candidate in &candidates {
            if candidate.aligned
                && !(m.is_multiple_of(32) && k.is_multiple_of(32) && n.is_multiple_of(32))
            {
                continue;
            }
            if matches!(candidate.schedule, Schedule::Vector) && !n.is_multiple_of(4) {
                continue;
            }
            if matches!(candidate.schedule, Schedule::Dot) && m * n > 65536 {
                continue;
            }
            let count = match candidate.schedule {
                Schedule::Tiles => m.div_ceil(32) * n.div_ceil(32),
                Schedule::Scalar => (m * n).div_ceil(candidate.kernel.workgroup_size() as usize),
                Schedule::Vector => {
                    (m * n / 4).div_ceil(candidate.kernel.workgroup_size() as usize)
                }
                Schedule::Dot => m * n,
            };
            let groups = count.min(65535) as u32;
            let params = uniform_f32(
                &context.device,
                &context.queue,
                &[
                    f32::from_bits(m as u32),
                    f32::from_bits(k as u32),
                    f32::from_bits(n as u32),
                    f32::from_bits(groups),
                ],
            );
            let output = runtime.zeros::<f32>(m * n)?;
            let group = candidate.kernel.create_bind_group(
                &context.device,
                &[&params, a.view().raw(), b.view().raw(), output.view().raw()],
            );
            plans.push((candidate, group, groups, output));
        }
        let mut production = runtime.program();
        let production_output =
            production.matmul(MatrixView::new(&a, m, k)?, MatrixView::new(&b, k, n)?)?;
        let paths = plans.len() + 1;
        let execute = |path: usize| -> AnyResult<Vec<f32>> {
            if path == plans.len() {
                return Ok(production
                    .submit_read(production_output.values())?
                    .wait(Duration::from_secs(30))?);
            }
            let (candidate, bindings, groups, output) = &plans[path];
            let mut encoder = context.device.create_command_encoder(&Default::default());
            candidate
                .kernel
                .record_dispatch(&mut encoder, bindings, *groups);
            let mut read = runtime.record_read(&mut encoder, output)?;
            read.submitted(context.queue.submit([encoder.finish()]));
            Ok(read.wait(Duration::from_secs(30))?)
        };
        let mut errors = vec![0.0f64; paths];
        for _ in 0..4 {
            for (path, error) in errors.iter_mut().enumerate() {
                *error = error.max(check(&execute(path)?, &expected, k));
            }
        }
        let steady = Instant::now();
        while steady.elapsed() < Duration::from_millis(250) {
            for path in 0..paths {
                std::hint::black_box(execute(path)?);
            }
        }
        let mut samples = vec![Vec::new(); paths];
        for iteration in 0..17 {
            for offset in 0..paths {
                let path = (iteration + offset) % paths;
                let start = Instant::now();
                let output = execute(path)?;
                samples[path].push(start.elapsed().as_secs_f64() * 1000.0);
                errors[path] = errors[path].max(check(&output, &expected, k));
                std::hint::black_box(output);
            }
        }
        println!("shape={m},{k},{n}; readback_bytes={}", m * n * 4);
        for (path, times) in samples.iter().enumerate() {
            let mut sorted = times.clone();
            sorted.sort_by(f64::total_cmp);
            println!(
                "{} median_ms={:.6} p90_ms={:.6} min_ms={:.6} max_error={:.6e} samples_ms={:?}",
                if path == plans.len() {
                    "production"
                } else {
                    plans[path].0.name
                },
                sorted[sorted.len() / 2],
                sorted[(sorted.len() * 9).div_ceil(10) - 1],
                sorted[0],
                errors[path],
                times
            );
        }
    }
    Ok(())
}
