//! Portable reduction study: compute-pass timestamps and separately unprofiled
//! host execution with a four-byte scalar readback. Raw samples are retained.
use compute_core::{
    Binding, ComputeProgram, ComputeRuntime, GpuArray, Kernel,
    gpu_compute::{GpuContext, GpuTimer},
    uniform_f32, wgpu,
};
use std::time::{Duration, Instant};

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;
struct Candidate {
    name: String,
    kernel: Kernel,
    items: u32,
    cap: u32,
}
struct Plan<'a> {
    candidate: &'a Candidate,
    stages: Vec<(wgpu::BindGroup, u32)>,
    output: GpuArray<f32>,
    program: Option<ComputeProgram<'a>>,
}
impl Plan<'_> {
    fn record(&self, pass: &mut wgpu::ComputePass<'_>) {
        if let Some(program) = &self.program {
            program.record_in_pass(pass);
            return;
        }
        for (bindings, groups) in &self.stages {
            self.candidate
                .kernel
                .record_in_pass(pass, bindings, *groups);
        }
    }
}
fn make_plan<'a>(
    rt: &'a ComputeRuntime,
    candidate: &'a Candidate,
    input: &GpuArray<f32>,
    mut count: u32,
) -> AnyResult<Plan<'a>> {
    let output = rt.zeros::<f32>(1)?;
    if candidate.name == "production" {
        let mut program = rt.program();
        program.sum_into(&input.prefix(count as usize)?, &output)?;
        return Ok(Plan {
            candidate,
            stages: Vec::new(),
            output,
            program: Some(program),
        });
    }
    let mut source = input.view().raw().clone();
    let mut stages = Vec::new();
    loop {
        let groups = count
            .div_ceil(candidate.kernel.workgroup_size() * candidate.items)
            .clamp(1, candidate.cap);
        let destination = if groups == 1 {
            output.view().raw().clone()
        } else {
            rt.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("padded reduction intermediate"),
                size: u64::from(groups).div_ceil(4) * 16,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let params = uniform_f32(
            rt.device(),
            rt.queue(),
            &[f32::from_bits(count), f32::from_bits(groups), 0.0, 0.0],
        );
        let bindings = candidate
            .kernel
            .create_bind_group(rt.device(), &[&params, &source, &destination]);
        stages.push((bindings, groups));
        if groups == 1 {
            break;
        }
        source = destination;
        count = groups;
    }
    Ok(Plan {
        candidate,
        stages,
        output,
        program: None,
    })
}
fn check(actual: f32, expected: f64) {
    assert!(
        (f64::from(actual) - expected).abs() <= 2e-6 * expected.abs().max(1.0),
        "{actual} != {expected}"
    );
}
fn report(name: &str, mode: &str, values: &[f64]) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    println!(
        "{name} {mode} median_ms={:.6} p90_ms={:.6} samples_ms={values:?}",
        sorted[sorted.len() / 2],
        sorted[(sorted.len() * 9).div_ceil(10) - 1]
    );
}
fn main() -> AnyResult<()> {
    let context = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    let bindings = [
        Binding::Uniform,
        Binding::StorageRead,
        Binding::StorageReadWrite,
    ];
    let mut candidates = Vec::new();
    let mut add = |name: String, source: String, wg: u32, items: u32, cap: u32| -> AnyResult<()> {
        let source = source.replacen("const WG: u32 = 256;", &format!("const WG: u32 = {wg};"), 1);
        candidates.push(Candidate {
            kernel: Kernel::new(&context.device, &name, &source, "main", &bindings)?,
            name,
            items,
            cap,
        });
        Ok(())
    };
    add(
        "baseline_wg256".into(),
        include_str!("reduction_candidates/baseline.wgsl").into(),
        256,
        1,
        65535,
    )?;
    let production = std::env::args().any(|x| x == "--production");
    let focused = std::env::args().any(|x| x == "--focused");
    if production {
        add(
            "production".into(),
            include_str!("reduction_candidates/baseline.wgsl").into(),
            256,
            1,
            65535,
        )?;
        for cap in [256, 1024] {
            add(
                format!("baseline_wg256_grain16_cap{cap}"),
                include_str!("reduction_candidates/baseline.wgsl").into(),
                256,
                16,
                cap,
            )?;
        }
        add(
            "scalar_vector_wg128_i8_cap256".into(),
            include_str!("reduction_candidates/scalar_vector.wgsl")
                .replace("const VECTORS: u32 = 1;", "const VECTORS: u32 = 2;"),
            128,
            8,
            256,
        )?;
    } else if !focused {
        for wg in [64, 128, 256] {
            for items in [1, 4, 8, 16] {
                let source = include_str!("reduction_candidates/chunked.wgsl").replace(
                    "const ITEMS: u32 = 4;",
                    &format!("const ITEMS: u32 = {items};"),
                );
                add(format!("scalar_wg{wg}_i{items}"), source, wg, items, 65535)?;
            }
            for items in [4, 8, 16] {
                let source = include_str!("reduction_candidates/vector.wgsl").replace(
                    "const VECTORS: u32 = 1;",
                    &format!("const VECTORS: u32 = {};", items / 4),
                );
                add(format!("vector_wg{wg}_i{items}"), source, wg, items, 65535)?;
            }
        }
        for cap in [256, 1024, 4096] {
            add(
                format!("scalar_wg128_i8_cap{cap}"),
                include_str!("reduction_candidates/chunked.wgsl")
                    .replace("const ITEMS: u32 = 4;", "const ITEMS: u32 = 8;"),
                128,
                8,
                cap,
            )?;
            add(
                format!("vector_wg256_i16_cap{cap}"),
                include_str!("reduction_candidates/vector.wgsl")
                    .replace("const VECTORS: u32 = 1;", "const VECTORS: u32 = 4;"),
                256,
                16,
                cap,
            )?;
        }
    } else {
        for (wg, items, cap) in [
            (256, 16, 256),
            (256, 16, 1024),
            (256, 4, 1024),
            (128, 8, 256),
        ] {
            add(
                format!("baseline_wg{wg}_grain{items}_cap{cap}"),
                include_str!("reduction_candidates/baseline.wgsl").into(),
                wg,
                items,
                cap,
            )?;
        }
        for (wg, items, cap) in [
            (256, 16, 256),
            (256, 16, 1024),
            (128, 8, 256),
            (64, 16, 256),
            (256, 4, 256),
        ] {
            add(
                format!("scalar_vector_wg{wg}_i{items}_cap{cap}"),
                include_str!("reduction_candidates/scalar_vector.wgsl").replace(
                    "const VECTORS: u32 = 1;",
                    &format!("const VECTORS: u32 = {};", items / 4),
                ),
                wg,
                items,
                cap,
            )?;
        }
        add(
            "vector_wg256_i16_cap256".into(),
            include_str!("reduction_candidates/vector.wgsl")
                .replace("const VECTORS: u32 = 1;", "const VECTORS: u32 = 4;"),
            256,
            16,
            256,
        )?;
        add(
            "scalar_wg128_i8_cap1024".into(),
            include_str!("reduction_candidates/chunked.wgsl")
                .replace("const ITEMS: u32 = 4;", "const ITEMS: u32 = 8;"),
            128,
            8,
            1024,
        )?;
    }
    println!(
        "backend={} period_ns={} candidates={}; 200ms held warmup,rotated samples; host mode has no query writes/resolution and reads4bytes; GPU mode measures all stages in one compute pass; both use timestamp-capable device; vec4 candidates require padded input/intermediates",
        context.backend_label(),
        timer.timestamp_period_ns(),
        candidates.len()
    );
    let counts: &[usize] = if production {
        &[
            0, 1, 3, 255, 4095, 4097, 16384, 65535, 65536, 65537, 262145, 1_000_003, 4_000_003,
            16_000_003,
        ]
    } else {
        &[0, 1, 3, 4097, 1_000_003, 4_000_003, 16_000_003]
    };
    let samples = if production { 31 } else { 21 };
    println!("samples={samples} production_policy={production}");
    for &n in counts {
        let mut values: Vec<f32> = (0..n).map(|i| (i % 127) as f32 / 128.0).collect();
        let expected = values.iter().map(|&x| f64::from(x)).sum::<f64>();
        values.resize(n.div_ceil(4).max(1) * 4, 123456.0);
        let input = rt.upload(&values)?;
        let plans = candidates
            .iter()
            .map(|c| make_plan(&rt, c, &input, n as u32))
            .collect::<AnyResult<Vec<_>>>()?;
        let execute = |path: usize, profiled: bool| -> AnyResult<f64> {
            let plan = &plans[path];
            let start = Instant::now();
            let mut encoder = context.device.create_command_encoder(&Default::default());
            let mut timestamp = if profiled {
                Some(
                    timer.record_compute(&mut encoder, "reduction candidates", |pass| {
                        plan.record(pass)
                    })?,
                )
            } else {
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    plan.record(&mut pass);
                }
                None
            };
            let mut read = rt.record_read(&mut encoder, &plan.output)?;
            let submission = context.queue.submit([if profiled {
                timer.finish(encoder)?
            } else {
                encoder.finish()
            }]);
            read.submitted(submission.clone());
            if let Some(timestamp) = timestamp.as_mut() {
                timestamp.submitted(submission);
            }
            let value = read.wait(Duration::from_secs(20))?[0];
            let gpu_ms = if let Some(timestamp) = timestamp {
                Some(timestamp.wait(Duration::from_secs(20))?.elapsed_ns / 1e6)
            } else {
                None
            };
            let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
            check(value, expected);
            if let Some(gpu_ms) = gpu_ms {
                assert!(
                    gpu_ms <= wall_ms + 0.1,
                    "GPU timestamp {gpu_ms}ms exceeds total profiled wall {wall_ms}ms"
                );
                Ok(gpu_ms)
            } else {
                Ok(wall_ms)
            }
        };
        for path in 0..plans.len() {
            execute(path, false)?;
            execute(path, true)?;
        }
        let warm = Instant::now();
        while warm.elapsed() < Duration::from_millis(200) {
            for path in 0..plans.len() {
                execute(path, false)?;
            }
        }
        let mut host = vec![Vec::new(); plans.len()];
        let mut gpu = vec![Vec::new(); plans.len()];
        for iteration in 0..samples {
            for offset in 0..plans.len() {
                let path = (iteration + offset) % plans.len();
                if (iteration + path) % 2 == 0 {
                    host[path].push(execute(path, false)?);
                    gpu[path].push(execute(path, true)?);
                } else {
                    gpu[path].push(execute(path, true)?);
                    host[path].push(execute(path, false)?);
                }
            }
        }
        println!("n={n} expected_f64={expected:.9}");
        for (path, plan) in plans.iter().enumerate() {
            println!(
                "{} stage_groups={:?}",
                plan.candidate.name,
                plan.stages.iter().map(|x| x.1).collect::<Vec<_>>()
            );
            report(&plan.candidate.name, "host", &host[path]);
            report(&plan.candidate.name, "gpu", &gpu[path]);
        }
    }
    Ok(())
}
