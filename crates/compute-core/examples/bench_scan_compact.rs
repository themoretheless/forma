//! Frozen pre-optimization scan/scatter versus current production compaction.
//! Same buffers, one compute pass/submission, stable output capacity/count and
//! (for pipeline mode) the same current compare and sum kernels on both paths.
use compute_core::{
    Binding, CompareOp, ComputeBatch, ComputeProgram, ComputeRuntime, GpuArray, Kernel, Reduction,
    gpu_compute::{GpuContext, wgpu},
    shaders, uniform_f32,
};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
const TIMEOUT: Duration = Duration::from_secs(30);
struct Baseline {
    scan: Kernel,
    add: Kernel,
    scatter: Kernel,
    finish: Kernel,
    compare: Kernel,
    sum: Kernel,
}
fn build(context: &GpuContext, name: &str, source: &str, bindings: &[Binding]) -> Kernel {
    Kernel::new(&context.device, name, source, "main", bindings).unwrap()
}
impl Baseline {
    fn new(c: &GpuContext) -> Self {
        use Binding::*;
        Self {
            scan: build(
                c,
                "old scan",
                include_str!("scan_baseline/scan_blocks.wgsl"),
                &[Uniform, StorageRead, StorageReadWrite, StorageReadWrite],
            ),
            add: build(
                c,
                "old scan add",
                include_str!("scan_baseline/scan_add.wgsl"),
                &[Uniform, StorageRead, StorageReadWrite],
            ),
            scatter: build(
                c,
                "old scatter",
                include_str!("scan_baseline/compact_scatter.wgsl"),
                &[
                    Uniform,
                    StorageRead,
                    StorageRead,
                    StorageRead,
                    StorageReadWrite,
                ],
            ),
            finish: build(
                c,
                "old finish",
                include_str!("scan_baseline/compact_finish.wgsl"),
                &[
                    Uniform,
                    StorageRead,
                    StorageRead,
                    StorageReadWrite,
                    StorageReadWrite,
                ],
            ),
            compare: build(
                c,
                "matched comparison",
                shaders::COMPARE_WGSL,
                &[Uniform, StorageRead, StorageRead, StorageReadWrite],
            ),
            sum: build(
                c,
                "matched sum",
                shaders::BLOCK_SUM_WGSL,
                &[Uniform, StorageRead, StorageReadWrite],
            ),
        }
    }
    fn scan<'a>(
        &'a self,
        rt: &ComputeRuntime,
        batch: &mut ComputeBatch<'a>,
        input: &GpuArray<u32>,
        output: &GpuArray<u32>,
        normalize: bool,
    ) {
        if input.is_empty() {
            return;
        }
        let blocks = self.scan.workgroup_count(input.len() as u32);
        let groups = blocks.min(65535);
        let totals = rt.zeros::<u32>(blocks as usize).unwrap();
        let params = uniform(rt, [input.len() as u32, groups, u32::from(normalize), 0]);
        let bind = self.scan.create_bind_group(
            rt.device(),
            &[
                &params,
                input.view().raw(),
                output.view().raw(),
                totals.view().raw(),
            ],
        );
        batch.push(&self.scan, &bind, groups);
        if blocks > 1 {
            let offsets = rt.zeros::<u32>(blocks as usize).unwrap();
            self.scan(rt, batch, &totals, &offsets, false);
            let groups = self.add.workgroup_count(input.len() as u32).min(65535);
            let params = uniform(
                rt,
                [input.len() as u32, groups, self.scan.workgroup_size(), 0],
            );
            let bind = self.add.create_bind_group(
                rt.device(),
                &[&params, offsets.view().raw(), output.view().raw()],
            );
            batch.push(&self.add, &bind, groups);
        }
    }
    fn case<'a>(
        &'a self,
        rt: &ComputeRuntime,
        input: &GpuArray<f32>,
        keep: &GpuArray<u32>,
        threshold: &GpuArray<f32>,
        pipeline: bool,
    ) -> Case<'a> {
        let n = input.len();
        let mut batch = ComputeBatch::new();
        if pipeline {
            let p = uniform(rt, [n as u32, 4, 1, 0]);
            let bind = self.compare.create_bind_group(
                rt.device(),
                &[
                    &p,
                    input.view().raw(),
                    threshold.view().raw(),
                    keep.view().raw(),
                ],
            );
            batch.push(
                &self.compare,
                &bind,
                self.compare.workgroup_count(n as u32).clamp(1, 65535),
            );
        }
        let output = rt.zeros::<f32>(n).unwrap();
        let count = rt.zeros::<u32>(1).unwrap();
        let offsets = rt.zeros::<u32>(n).unwrap();
        self.scan(rt, &mut batch, keep, &offsets, true);
        let groups = self.scatter.workgroup_count(n as u32).clamp(1, 65535);
        let p = uniform(rt, [n as u32, groups, 0, 0]);
        let bind = self.scatter.create_bind_group(
            rt.device(),
            &[
                &p,
                input.view().raw(),
                keep.view().raw(),
                offsets.view().raw(),
                output.view().raw(),
            ],
        );
        batch.push(&self.scatter, &bind, groups);
        let bind = self.finish.create_bind_group(
            rt.device(),
            &[
                &p,
                keep.view().raw(),
                offsets.view().raw(),
                output.view().raw(),
                count.view().raw(),
            ],
        );
        batch.push(&self.finish, &bind, groups);
        let sum = if pipeline {
            let sum = rt.zeros::<f32>(1).unwrap();
            let reduction = Reduction::with_output(
                rt.device(),
                rt.queue(),
                &self.sum,
                output.view().raw(),
                n as u32,
                sum.view().raw(),
            );
            batch.push_reduction(&reduction);
            Some(sum)
        } else {
            None
        };
        Case {
            name: "baseline",
            plan: Plan::Raw(batch),
            output,
            count,
            sum,
        }
    }
}
fn uniform(rt: &ComputeRuntime, v: [u32; 4]) -> wgpu::Buffer {
    uniform_f32(rt.device(), rt.queue(), &v.map(f32::from_bits))
}
enum Plan<'a> {
    Raw(ComputeBatch<'a>),
    Production(ComputeProgram<'a>),
}
struct Case<'a> {
    name: &'static str,
    plan: Plan<'a>,
    output: GpuArray<f32>,
    count: GpuArray<u32>,
    sum: Option<GpuArray<f32>>,
}
impl<'a> Case<'a> {
    fn production(
        rt: &'a ComputeRuntime,
        input: &GpuArray<f32>,
        keep: &GpuArray<u32>,
        threshold: &GpuArray<f32>,
        pipeline: bool,
    ) -> Self {
        let mut plan = rt.program();
        let mask = if pipeline {
            plan.compare(CompareOp::Greater, input, threshold).unwrap()
        } else {
            keep.clone()
        };
        let selected = plan.compact(input, &mask).unwrap();
        let sum = if pipeline {
            Some(plan.sum(selected.values()).unwrap())
        } else {
            None
        };
        Self {
            name: "production",
            plan: Plan::Production(plan),
            output: selected.values().clone(),
            count: selected.count().clone(),
            sum,
        }
    }
    fn run(&self, c: &GpuContext, rt: &ComputeRuntime, full: bool) -> (Vec<f32>, u32) {
        let mut encoder = c.device.create_command_encoder(&Default::default());
        match &self.plan {
            Plan::Raw(batch) => batch.record(&mut encoder),
            Plan::Production(plan) => plan.record(&mut encoder),
        }
        let values = if full {
            &self.output
        } else {
            self.sum.as_ref().unwrap_or(&self.output)
        };
        let mut read = rt.record_read(&mut encoder, values).unwrap();
        let mut count = rt.record_read(&mut encoder, &self.count).unwrap();
        let submission = c.queue.submit([encoder.finish()]);
        read.submitted(submission.clone());
        count.submitted(submission);
        (read.wait(TIMEOUT).unwrap(), count.wait(TIMEOUT).unwrap()[0])
    }
}
fn summary(samples: &mut [f64]) -> (f64, f64) {
    samples.sort_by(f64::total_cmp);
    (
        samples[samples.len() / 2],
        samples[(samples.len() - 1) * 9 / 10],
    )
}
fn main() {
    let c = GpuContext::new().expect("GPU required; no fallback");
    let rt = ComputeRuntime::new(&c).unwrap();
    let baseline = Baseline::new(&c);
    let repeats = std::env::var("SCAN_BENCH_REPEATS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(15);
    eprintln!(
        "backend={:?};3 warmups;{repeats} rotated samples;one submission;wall ms includes readback",
        c.backend
    );
    println!("elements,mask_percent,workload,implementation,median_ms,p90_ms,readback_bytes");
    for n in [4096, 1_048_576, 4_194_304] {
        let values: Vec<f32> = (0..n).map(|i| (i % 32) as f32 - 15.5).collect();
        let input = rt.upload(&values).unwrap();
        let keep = rt.zeros::<u32>(n).unwrap();
        let threshold = rt.zeros::<f32>(1).unwrap();
        for pipeline in [false, true] {
            let cases = [
                baseline.case(&rt, &input, &keep, &threshold, pipeline),
                Case::production(&rt, &input, &keep, &threshold, pipeline),
            ];
            for (percent, cutoff) in [(0, 32.), (50, 0.), (100, -32.)] {
                let mask: Vec<u32> = values
                    .iter()
                    .map(|v| if *v > cutoff { 0x80000000 } else { 0 })
                    .collect();
                rt.write(&keep, 0, &mask).unwrap();
                rt.write(&threshold, 0, &[cutoff]).unwrap();
                let expected: Vec<f32> = values.iter().copied().filter(|v| *v > cutoff).collect();
                let sum = expected.iter().map(|&v| v as f64).sum::<f64>() as f32;
                for case in &cases {
                    let (actual, count) = case.run(&c, &rt, true);
                    assert_eq!(count as usize, expected.len());
                    assert_eq!(&actual[..expected.len()], expected);
                    assert!(actual[expected.len()..].iter().all(|v| *v == 0.));
                    if pipeline {
                        let (actual, count) = case.run(&c, &rt, false);
                        assert_eq!(count as usize, expected.len());
                        assert!((actual[0] - sum).abs() < 1e-6 * sum.abs().max(1.));
                    }
                }
                let mut times = [Vec::new(), Vec::new()];
                for iteration in 0..repeats + 3 {
                    for offset in 0..2 {
                        let index = (iteration + offset) % 2;
                        let start = Instant::now();
                        let (actual, count) = cases[index].run(&c, &rt, false);
                        let ms = start.elapsed().as_secs_f64() * 1000.;
                        black_box(&actual);
                        assert_eq!(count as usize, expected.len());
                        if iteration >= 3 {
                            times[index].push(ms);
                        }
                    }
                }
                for (case, times) in cases.iter().zip(times.iter_mut()) {
                    let (median, p90) = summary(times);
                    let mode = if pipeline {
                        "compare_compact_sum"
                    } else {
                        "compact_full"
                    };
                    let bytes = if pipeline { 8 } else { n * 4 + 4 };
                    println!(
                        "{n},{percent},{mode},{},{median:.6},{p90:.6},{bytes}",
                        case.name
                    );
                }
            }
        }
    }
}
