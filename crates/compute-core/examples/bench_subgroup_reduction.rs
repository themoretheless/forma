//! Native subgroup reduction study; production reduction is unchanged.
//! cargo run --release -p compute-core --example bench_subgroup_reduction
use compute_core::{
    Binding, Kernel,
    gpu_compute::{ByteReadback, GpuContext, GpuTimer, wgpu},
    storage_f32, storage_f32_zeroed, uniform_f32,
};
use std::time::{Duration, Instant};

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;
const BINDINGS: [Binding; 3] = [
    Binding::Uniform,
    Binding::StorageRead,
    Binding::StorageReadWrite,
];
const WAIT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug)]
enum Kind {
    Tree,
    Add,
    Shuffle,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Tree => "tree",
            Self::Add => "add",
            Self::Shuffle => "shuffle",
        }
    }
}
fn source(kind: Kind, wg: u32, items: u32) -> String {
    let (inputs, helpers, reducer) = match kind {
        Kind::Tree => ("", "", include_str!("subgroup_reduction/tree.wgsl")),
        Kind::Add | Kind::Shuffle => (
            "@builtin(subgroup_invocation_id) lane: u32,
             @builtin(subgroup_id) subgroup: u32,
             @builtin(subgroup_size) subgroup_width: u32,
             @builtin(num_subgroups) subgroup_count: u32,",
            if matches!(kind, Kind::Shuffle) {
                include_str!("subgroup_reduction/shuffle_helpers.wgsl")
            } else {
                ""
            },
            if matches!(kind, Kind::Shuffle) {
                include_str!("subgroup_reduction/shuffle.wgsl")
            } else {
                include_str!("subgroup_reduction/add.wgsl")
            },
        ),
    };
    include_str!("subgroup_reduction/common.wgsl")
        .replace("const WG: u32 = 256;", &format!("const WG: u32 = {wg};"))
        .replace(
            "const ITEMS: u32 = 8;",
            &format!("const ITEMS: u32 = {items};"),
        )
        .replace("// SUBGROUP INPUTS", inputs)
        .replace("// HELPERS", helpers)
        .replace("// REDUCE", reducer)
}

struct Candidate {
    kind: Kind,
    wg: u32,
    items: u32,
    kernel: Kernel,
}
impl Candidate {
    fn new(context: &GpuContext, kind: Kind, wg: u32, items: u32) -> AnyResult<Self> {
        let kernel = Kernel::new(
            &context.device,
            kind.name(),
            &source(kind, wg, items),
            "main",
            &BINDINGS,
        )?;
        Ok(Self {
            kind,
            wg,
            items,
            kernel,
        })
    }
}
struct Prepared<'a> {
    candidate: &'a Candidate,
    cap: u32,
    passes: Vec<(wgpu::BindGroup, u32)>,
    output: wgpu::Buffer,
}
impl<'a> Prepared<'a> {
    fn new(
        context: &GpuContext,
        candidate: &'a Candidate,
        input: &wgpu::Buffer,
        n: usize,
        cap: u32,
    ) -> Self {
        let mut count = n as u32;
        let mut source = input.clone();
        let mut passes = Vec::new();
        let output = loop {
            let groups = count.div_ceil(candidate.wg * candidate.items).clamp(1, cap);
            let params = uniform_f32(
                &context.device,
                &context.queue,
                &[f32::from_bits(count), f32::from_bits(groups), 0.0, 0.0],
            );
            let output = storage_f32_zeroed(&context.device, &context.queue, groups as usize);
            let bindings = candidate
                .kernel
                .create_bind_group(&context.device, &[&params, &source, &output]);
            passes.push((bindings, groups));
            if groups == 1 {
                break output;
            }
            source = output;
            count = groups;
        };
        Self {
            candidate,
            cap,
            passes,
            output,
        }
    }
    fn record(&self, pass: &mut wgpu::ComputePass<'_>) {
        for (bindings, groups) in &self.passes {
            self.candidate
                .kernel
                .record_in_pass(pass, bindings, *groups);
        }
    }
    fn run(&self, context: &GpuContext, timer: Option<&GpuTimer>) -> AnyResult<(f32, f64, f64)> {
        let started = Instant::now();
        let mut encoder = context.device.create_command_encoder(&Default::default());
        let mut timestamp = if let Some(timer) = timer {
            Some(
                timer.record_compute(&mut encoder, "subgroup reduction chain", |pass| {
                    self.record(pass)
                })?,
            )
        } else {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            self.record(&mut pass);
            None
        };
        let mut result =
            ByteReadback::copy_buffer(&context.device, &mut encoder, &self.output, 0, 4)?;
        let commands = if let Some(timer) = timer {
            timer.finish(encoder)?
        } else {
            encoder.finish()
        };
        let submission = context.queue.submit([commands]);
        result.submitted(submission.clone());
        if let Some(time) = timestamp.as_mut() {
            time.submitted(submission);
        }
        let bytes = result.wait(WAIT)?;
        let gpu_ns = timestamp
            .map(|time| time.wait(WAIT).map(|time| time.elapsed_ns))
            .transpose()?
            .unwrap_or(0.0);
        let wall_ns = started.elapsed().as_secs_f64() * 1e9;
        assert!(
            gpu_ns <= wall_ns + 10_000.0,
            "GPU interval must fit enclosing wall interval"
        );
        Ok((
            f32::from_le_bytes(bytes.try_into().unwrap()),
            gpu_ns / 1000.0,
            wall_ns / 1000.0,
        ))
    }
}
fn check(value: f32, expected: f64, bound: f64) -> f64 {
    let error = (f64::from(value) - expected).abs();
    assert!(
        value.is_finite() && error <= bound,
        "value={value} reference={expected} error={error} bound={bound}"
    );
    error
}
fn percentile(values: &[f64], p: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[(p * sorted.len() as f64).ceil() as usize - 1]
}
fn correctness(context: &GpuContext, candidates: &[Candidate]) -> AnyResult<()> {
    for n in [0, 1, 3, 15, 17, 31, 33, 63, 65, 127, 129, 257, 4097] {
        // Exact binary fractions and balanced cancellation expose missing lanes,
        // duplicate loads, stale empty outputs and inactive tail contributions.
        for pattern in 0..2 {
            let values: Vec<f32> = (0..n)
                .map(|i| {
                    if pattern == 0 {
                        ((i * 17 % 257) as f32 - 128.0) / 128.0
                    } else {
                        let magnitude = 10000.0 + (i / 2 % 8) as f32 * 0.5;
                        if i % 2 == 0 { magnitude } else { -magnitude }
                    }
                })
                .collect();
            let expected = values.iter().map(|&v| f64::from(v)).sum::<f64>();
            let input = storage_f32(&context.device, &context.queue, &values);
            for candidate in candidates {
                let prepared = Prepared::new(context, candidate, &input, n, 1024);
                context
                    .queue
                    .write_buffer(&prepared.output, 0, &1234.0_f32.to_le_bytes());
                let (actual, _, _) = prepared.run(context, None)?;
                check(actual, expected, 1e-6);
            }
        }
    }
    println!(
        "correctness=passed odd/empty/balanced-cancellation/tail cases across {} kernels",
        candidates.len()
    );
    Ok(())
}

fn main() -> AnyResult<()> {
    let context =
        GpuContext::with_features(wgpu::Features::SUBGROUP | wgpu::Features::TIMESTAMP_QUERY)?;
    let timer = GpuTimer::new(&context)?;
    println!(
        "backend={} enabled={:?} subgroup_range={}..{} timestamp_period_ns={}",
        context.backend_label(),
        context.enabled_features(),
        context.subgroup_min_size,
        context.subgroup_max_size,
        timer.timestamp_period_ns()
    );
    let mut candidates = Vec::new();
    for wg in [64, 128, 256] {
        for items in [1, 4, 8, 16] {
            for kind in [Kind::Tree, Kind::Add, Kind::Shuffle] {
                candidates.push(Candidate::new(&context, kind, wg, items)?);
            }
        }
    }
    correctness(&context, &candidates)?;
    // WG16 checks smaller workgroups; a subgroup wider than 16 has inactive lanes.
    let partial_subgroup = [
        Candidate::new(&context, Kind::Add, 16, 1)?,
        Candidate::new(&context, Kind::Shuffle, 16, 1)?,
    ];
    correctness(&context, &partial_subgroup)?;
    if std::env::args().any(|arg| arg == "--validate-only") {
        return Ok(());
    }
    println!(
        "timing: prepared full reductions; 200ms warmup; 17 rotated samples; GPU queries and unprofiled host measured in separate runs; host includes encoding/submission/4-byte readback; input upload, pipeline compile, uniforms and partial allocation excluded; timestamp resolve occurs after completion"
    );
    for n in [4097, 1_000_003, 4_000_003, 16_000_003] {
        let values: Vec<f32> = (0..n)
            .map(|i| 0.25 + ((i * 17 % 257) as f32 - 128.0) / 1024.0)
            .collect();
        let expected = values.iter().map(|&v| f64::from(v)).sum::<f64>();
        let input = storage_f32(&context.device, &context.queue, &values);
        let mut prepared = Vec::new();
        for candidate in &candidates {
            for cap in [256, 1024, 4096, 65535] {
                prepared.push(Prepared::new(&context, candidate, &input, n, cap));
            }
        }
        let warm = Instant::now();
        while warm.elapsed() < Duration::from_millis(200) {
            for plan in &prepared {
                check(
                    plan.run(&context, None)?.0,
                    expected,
                    1e-6 * expected.max(1.0),
                );
            }
        }
        let mut host = vec![Vec::new(); prepared.len()];
        let mut gpu = vec![Vec::new(); prepared.len()];
        let mut errors = vec![0.0_f64; prepared.len()];
        for sample in 0..17 {
            for mode in 0..2 {
                for offset in 0..prepared.len() {
                    let index = (sample * 37 + offset) % prepared.len();
                    let (value, gpu_us, wall_us) =
                        prepared[index].run(&context, (mode == 1).then_some(&timer))?;
                    errors[index] =
                        errors[index].max(check(value, expected, 1e-6 * expected.max(1.0)));
                    if mode == 0 {
                        host[index].push(wall_us);
                    } else {
                        gpu[index].push(gpu_us);
                    }
                }
            }
        }
        for (index, plan) in prepared.iter().enumerate() {
            let c = plan.candidate;
            println!(
                "n={n} kind={} wg={} items={} cap={} passes={} gpu_median_us={:.3} gpu_p90_us={:.3} host_median_us={:.3} host_p90_us={:.3} max_abs_error={:.8}",
                c.kind.name(),
                c.wg,
                c.items,
                plan.cap,
                plan.passes.len(),
                percentile(&gpu[index], 0.5),
                percentile(&gpu[index], 0.9),
                percentile(&host[index], 0.5),
                percentile(&host[index], 0.9),
                errors[index]
            );
            println!(
                "gpu_samples_us={:?}\nhost_samples_us={:?}",
                gpu[index], host[index]
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_native_subgroup_candidates_validate_without_a_device() {
        for wg in [16, 64, 128, 256] {
            for items in [1, 4, 8, 16] {
                for kind in [Kind::Tree, Kind::Add, Kind::Shuffle] {
                    let module = naga::front::wgsl::parse_str(&source(kind, wg, items)).unwrap();
                    naga::valid::Validator::new(
                        naga::valid::ValidationFlags::all(),
                        naga::valid::Capabilities::all(),
                    )
                    .validate(&module)
                    .unwrap();
                }
            }
        }
    }
}
