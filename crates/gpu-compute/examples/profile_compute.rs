//! Compare device pass time with encode/submit/wait wall time.
//! cargo run --release -p gpu-compute --example profile_compute
use gpu_compute::{ByteReadback, GpuBuffer, GpuContext, GpuTimer, wgpu};
use std::time::{Duration, Instant};

const ELEMENTS: u32 = 1_000_003;
const ROUNDS: u32 = 64;

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = GpuContext::with_timestamps()?;
    let timer = GpuTimer::new(&context)?;
    let values = GpuBuffer::new(
        &context,
        u64::from(ELEMENTS) * 4,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    )?;
    let module = context
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("profile compute"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "@group(0) @binding(0) var<storage, read_write> values: array<u32>;
             @compute @workgroup_size(64)
             fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
                 if (id.x >= arrayLength(&values)) {{ return; }}
                 var v = id.x;
                 for (var i = 0u; i < {ROUNDS}u; i++) {{
                     v = (v ^ (v >> 13u)) * 1664525u + 1013904223u;
                 }}
                 values[id.x] = v;
             }}"
                )
                .into(),
            ),
        });
    let pipeline = context
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("profile compute"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bindings = context
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: values.raw().as_entire_binding(),
            }],
        });
    let mut gpu_us = Vec::new();
    let mut wall_us = Vec::new();
    for sample in 0..18 {
        let started = Instant::now();
        let mut encoder = context.device.create_command_encoder(&Default::default());
        let mut timing = timer.record_compute(&mut encoder, "integer recurrence", |pass| {
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(ELEMENTS.div_ceil(64), 1, 1);
        })?;
        // Read a small output sample outside the timestamped compute pass.
        let mut output =
            ByteReadback::copy_buffer(&context.device, &mut encoder, values.raw(), 0, 128)?;
        let submission = context.queue.submit([timer.finish(encoder)?]);
        timing.submitted(submission.clone());
        output.submitted(submission);
        let measured = timing.wait(Duration::from_secs(30))?;
        let output = output.wait(Duration::from_secs(30))?;
        let elapsed = started.elapsed();
        for (index, bytes) in output.as_chunks::<4>().0.iter().enumerate() {
            let mut expected = index as u32;
            for _ in 0..ROUNDS {
                expected = (expected ^ (expected >> 13))
                    .wrapping_mul(1664525)
                    .wrapping_add(1013904223);
            }
            assert_eq!(u32::from_le_bytes(*bytes), expected);
        }
        if sample >= 3 {
            gpu_us.push(measured.elapsed_ns / 1000.0);
            wall_us.push(elapsed.as_secs_f64() * 1_000_000.0);
        }
    }
    println!(
        "backend={} timestamp_period_ns={} elements={ELEMENTS} rounds={ROUNDS} warmup=3 samples=15",
        context.backend_label(),
        timer.timestamp_period_ns()
    );
    println!(
        "gpu_pass_median_us={:.3} wall_encode_submit_readback_median_us={:.3}",
        median(&mut gpu_us),
        median(&mut wall_us)
    );
    println!(
        "device interval=compute pass; wall interval=encode, submit, completion-before-resolve, timestamp and 128-byte output readback; workload allocation/compilation excluded; profiling/readback allocation included; upload=none"
    );
    Ok(())
}
