use crate::{ComputeRuntime, Kernel, read_f32, uniform_f32};
use wgpu::{Buffer, Device, Queue};

#[derive(Clone, Copy)]
struct Schedule {
    values_per_lane: u32,
    max_groups: u32,
}

const RAW_SCHEDULE: Schedule = Schedule {
    values_per_lane: 1,
    max_groups: 65535,
};

fn runtime_schedule(backend: wgpu::Backend, count: u32) -> Schedule {
    // Local paired measurements justify reducing partials for larger Metal
    // workloads. Small inputs and unmeasured backends retain the old schedule.
    if backend == wgpu::Backend::Metal && count > 65536 {
        Schedule {
            values_per_lane: 16,
            max_groups: 256,
        }
    } else {
        RAW_SCHEDULE
    }
}

/// Full reduction of `input[0..count]` to a scalar via repeated
/// [`crate::shaders::BLOCK_SUM_WGSL`] passes: each pass runs a grid-strided dispatch
/// (up to 65535 workgroups), feeding the partials back as the next pass's
/// input until a single partial remains. Lengths are arbitrary — the 65535
/// per-dispatch group limit only shapes the pass schedule, not the total.
pub fn reduce_f32(device: &Device, queue: &Queue, sum: &Kernel, input: &Buffer, count: u32) -> f32 {
    let reduction = Reduction::new(device, queue, sum, input, count);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("compute reduction"),
    });
    reduction.record(&mut encoder);
    queue.submit([encoder.finish()]);
    read_f32(device, queue, reduction.output(), 1)[0]
}

/// Reusable sum reduction for a fixed input buffer and element count.
///
/// Construct with a `BLOCK_SUM_WGSL` kernel. Each pass owns separate immutable
/// parameters: queue writes to one shared uniform would make every pass in a
/// single submission see the last parameters. Bind groups retain all buffers.
/// Update the input contents and record again to reuse the allocations. Record
/// producer kernels first and consumers of `output()` afterwards on the same
/// encoder; no intermediate CPU readback or queue submission is needed.
/// The input and output must not be destroyed while this plan is in use.
pub struct Reduction<'a> {
    kernel: &'a Kernel,
    passes: Vec<(wgpu::BindGroup, u32)>,
    output: Buffer,
}

impl<'a> Reduction<'a> {
    pub fn new(
        device: &Device,
        queue: &Queue,
        kernel: &'a Kernel,
        input: &Buffer,
        count: u32,
    ) -> Self {
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("reduction output"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self::with_output(device, queue, kernel, input, count, &output)
    }

    /// Record a reduction into caller-owned storage. Raw handles must belong to
    /// the supplied device; typed callers validate ownership before this call.
    pub fn with_output(
        device: &Device,
        queue: &Queue,
        kernel: &'a Kernel,
        input: &Buffer,
        count: u32,
        destination: &Buffer,
    ) -> Self {
        Self::with_schedule(
            device,
            queue,
            kernel,
            input,
            count,
            destination,
            RAW_SCHEDULE,
        )
    }

    pub(crate) fn for_runtime(
        runtime: &'a ComputeRuntime,
        input: &Buffer,
        count: u32,
        destination: &Buffer,
    ) -> Self {
        Self::with_schedule(
            runtime.device(),
            runtime.queue(),
            &runtime.sum,
            input,
            count,
            destination,
            runtime_schedule(runtime.context().backend, count),
        )
    }

    fn with_schedule(
        device: &Device,
        queue: &Queue,
        kernel: &'a Kernel,
        input: &Buffer,
        mut count: u32,
        destination: &Buffer,
        schedule: Schedule,
    ) -> Self {
        assert!(destination.size() >= 4);
        assert!(
            u64::from(count) * 4 <= input.size(),
            "reduction input is too small"
        );
        let mut source = input.clone();
        let mut passes = Vec::new();
        let output;
        loop {
            // At WG=1 each group must consume at least two elements to make
            // progress. The grid-strided shader supports this schedule.
            let groups = count
                .div_ceil((kernel.workgroup_size() * schedule.values_per_lane).max(2))
                .clamp(1, schedule.max_groups);
            let params = uniform_f32(
                device,
                queue,
                &[f32::from_bits(count), f32::from_bits(groups), 0.0, 0.0],
            );
            let partials = if groups == 1 {
                destination.clone()
            } else {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("reduction partials"),
                    size: u64::from(groups) * 4,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            };
            let bindings = kernel.create_bind_group(device, &[&params, &source, &partials]);
            passes.push((bindings, groups));
            source = partials.clone();
            if groups == 1 {
                output = partials;
                break;
            }
            count = groups;
        }
        Self {
            kernel,
            passes,
            output,
        }
    }

    pub(crate) fn append_to(&self, batch: &mut crate::ComputeBatch<'a>) {
        for (bindings, groups) in &self.passes {
            batch.push(self.kernel, bindings, *groups);
        }
    }

    /// Appends all reduction passes without submitting or reading back.
    pub fn record(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("sum reduction"),
            timestamp_writes: None,
        });
        self.record_in_pass(&mut pass);
    }

    /// Appends all dependent dispatches to an existing compute pass. This also
    /// permits one timestamp interval around the complete reduction.
    pub fn record_in_pass(&self, pass: &mut wgpu::ComputePass<'_>) {
        for (bindings, groups) in &self.passes {
            self.kernel.record_in_pass(pass, bindings, *groups);
        }
    }

    /// Single-f32 storage buffer, ready after the recorded commands execute.
    pub fn output(&self) -> &Buffer {
        &self.output
    }
}
