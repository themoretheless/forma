//! cargo run --release --offline -p compute-core --example bench_matmul
//! Host-observed times include full output readback; no GPU timestamp claims.
use compute_core::{
    Binding, ComputeRuntime, Kernel, MatrixView, gpu_compute::GpuContext, uniform_f32,
};
use std::time::{Duration, Instant};

const NAIVE: &str = r#"
struct Params { rows: u32, inner: u32, columns: u32, pad: u32 };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    for (var i = gid.x; i < p.rows * p.columns; i += groups.x * WG) {
        let row = i / p.columns;
        let col = i % p.columns;
        var value = 0.0;
        for (var k = 0u; k < p.inner; k++) {
            value = fma(a[row * p.inner + k], b[k * p.columns + col], value);
        }
        out[i] = value;
    }
}
"#;

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

// Single-threaded, cache-blocked f32 baseline with contiguous inner column loops.
// This is portable Rust, not a vendor BLAS or a many-core CPU implementation.
fn cpu_matmul(a: &[f32], b: &[f32], m: usize, k: usize, n: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; m * n];
    for ib in (0..m).step_by(32) {
        for kb in (0..k).step_by(32) {
            for jb in (0..n).step_by(64) {
                let end = (jb + 64).min(n);
                for i in ib..(ib + 32).min(m) {
                    for inner in kb..(kb + 32).min(k) {
                        let value = a[i * k + inner];
                        let out_row = &mut out[i * n + jb..i * n + end];
                        let b_row = &b[inner * n + jb..inner * n + end];
                        for (out, &b) in out_row.iter_mut().zip(b_row) {
                            *out += value * b;
                        }
                    }
                }
            }
        }
    }
    out
}

fn reference(a: &[f32], b: &[f32], m: usize, k: usize, n: usize) -> Vec<f64> {
    let mut out = vec![0.0; m * n];
    for row in 0..m {
        for inner in 0..k {
            let a = f64::from(a[row * k + inner]);
            for (out, &b) in out[row * n..(row + 1) * n]
                .iter_mut()
                .zip(&b[inner * n..(inner + 1) * n])
            {
                *out += a * f64::from(b);
            }
        }
    }
    out
}

fn check(actual: &[f32], expected: &[f64], k: usize) -> f64 {
    assert_eq!(actual.len(), expected.len());
    let max_error = actual
        .iter()
        .zip(expected)
        .map(|(&a, &b)| (f64::from(a) - b).abs())
        .fold(0.0, f64::max);
    // Inputs are bounded by roughly 2.2; this absolute bound also covers cancellation.
    assert!(
        max_error < 2e-5 * k.max(1) as f64,
        "matrix error {max_error}"
    );
    max_error
}

fn median_ms(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2] * 1000.0
}

fn main() -> AnyResult<()> {
    let context = GpuContext::new().ok_or("GPU adapter required")?;
    let runtime = ComputeRuntime::new(&context)?;
    let naive = Kernel::new(
        &context.device,
        "naive matrix baseline",
        NAIVE,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
    )?;
    println!(
        "backend: {}; 4 warmups, 17 interleaved samples, median; GPU includes submission/wait/full readback; CPU single-thread blocked f32; f64 parity outside timing",
        context.backend_label()
    );
    for (m, k, n) in [
        (64, 64, 64),
        (127, 259, 193),
        (512, 512, 512),
        (1024, 1024, 1024),
        (4096, 256, 8),
        (8, 2048, 8),
    ] {
        let av: Vec<f32> = (0..m * k)
            .map(|i| ((i * 17 % 73) as f32 - 36.0) / 17.0)
            .collect();
        let bv: Vec<f32> = (0..k * n)
            .map(|i| ((i * 7 % 41) as f32 - 20.0) / 13.0)
            .collect();
        let expected = reference(&av, &bv, m, k, n);
        let a = runtime.upload(&av)?;
        let b = runtime.upload(&bv)?;
        let prepare = Instant::now();
        let mut tiled = runtime.program();
        let product = tiled.matmul(MatrixView::new(&a, m, k)?, MatrixView::new(&b, k, n)?)?;
        let prepare_ms = prepare.elapsed().as_secs_f64() * 1000.0;
        let naive_output = runtime.zeros::<f32>(m * n)?;
        let params = uniform_f32(
            &context.device,
            &context.queue,
            &[
                f32::from_bits(m as u32),
                f32::from_bits(k as u32),
                f32::from_bits(n as u32),
                0.0,
            ],
        );
        let bindings = naive.create_bind_group(
            &context.device,
            &[
                &params,
                a.view().raw(),
                b.view().raw(),
                naive_output.view().raw(),
            ],
        );
        let tiled_run = || -> AnyResult<Vec<f32>> {
            Ok(tiled
                .submit_read(product.values())?
                .wait(Duration::from_secs(30))?)
        };
        let naive_run = || -> AnyResult<Vec<f32>> {
            let mut encoder = context.device.create_command_encoder(&Default::default());
            naive.record_dispatch(
                &mut encoder,
                &bindings,
                naive.workgroup_count((m * n) as u32).min(65535),
            );
            let mut ticket = runtime.record_read(&mut encoder, &naive_output)?;
            ticket.submitted(context.queue.submit([encoder.finish()]));
            Ok(ticket.wait(Duration::from_secs(30))?)
        };
        for _ in 0..4 {
            check(&tiled_run()?, &expected, k);
            check(&naive_run()?, &expected, k);
            check(&cpu_matmul(&av, &bv, m, k, n), &expected, k);
        }
        let mut samples = [Vec::new(), Vec::new(), Vec::new()];
        let mut error = [0.0f64; 3];
        for iteration in 0..17 {
            for offset in 0..3 {
                let path = (iteration + offset) % 3;
                let start = Instant::now();
                let actual = match path {
                    0 => tiled_run()?,
                    1 => naive_run()?,
                    _ => cpu_matmul(
                        std::hint::black_box(&av),
                        std::hint::black_box(&bv),
                        m,
                        k,
                        n,
                    ),
                };
                samples[path].push(start.elapsed().as_secs_f64());
                error[path] = error[path].max(check(&actual, &expected, k));
                std::hint::black_box(actual);
            }
        }
        let [mut tiled_ms, mut naive_ms, mut cpu_ms] = samples;
        println!(
            "{m}x{k} @ {k}x{n}: plan {prepare_ms:.3} ms; selected GPU {:.3} ms; naive GPU {:.3} ms; CPU {:.3} ms; readback {} bytes; max absolute error tiled={:.3e} naive={:.3e} cpu={:.3e}",
            median_ms(&mut tiled_ms),
            median_ms(&mut naive_ms),
            median_ms(&mut cpu_ms),
            m * n * 4,
            error[0],
            error[1],
            error[2]
        );
    }
    Ok(())
}
