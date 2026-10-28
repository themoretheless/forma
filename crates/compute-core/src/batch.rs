use crate::{Kernel, Reduction};
use wgpu::{Device, Queue};

/// Reusable sequence of GPU dispatches. Binding groups and intermediate buffers
/// are retained between runs; each submission creates only a command encoder.
#[derive(Default)]
pub struct ComputeBatch<'a> {
    steps: Vec<(&'a Kernel, wgpu::BindGroup, u32)>,
}

impl<'a> ComputeBatch<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            steps: Vec::with_capacity(capacity),
        }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn clear(&mut self) {
        self.steps.clear();
    }

    /// Number of pipeline switches that will actually occur when recording this batch
    /// after consecutive-kernel deduplication.
    pub fn pipeline_switch_count(&self) -> usize {
        let mut switches = 0;
        let mut current_kernel: Option<*const Kernel> = None;
        for (kernel, _, _) in &self.steps {
            let ptr = *kernel as *const Kernel;
            if current_kernel != Some(ptr) {
                switches += 1;
                current_kernel = Some(ptr);
            }
        }
        switches
    }

    /// Adds a dispatch using a cached binding group. Zero groups are a no-op.
    pub fn push(&mut self, kernel: &'a Kernel, bindings: &wgpu::BindGroup, groups: u32) {
        assert!(groups <= 65535, "batch dispatch exceeds workgroup limit");
        if groups != 0 {
            self.steps.push((kernel, bindings.clone(), groups));
        }
    }

    /// Adds every pass of a prepared reduction in dependency order.
    pub fn push_reduction(&mut self, reduction: &Reduction<'a>) {
        reduction.append_to(self);
    }

    /// Commits a fully prepared operation without submitting intermediate work.
    pub(crate) fn append(&mut self, other: Self) {
        self.steps.reserve(other.steps.len());
        self.steps.extend(other.steps);
    }

    /// Appends one compute pass, allowing copies or other work around it.
    /// Dispatch order and wgpu's per-dispatch barriers preserve dependencies.
    pub fn record(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.steps.is_empty() {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("compute batch"),
            timestamp_writes: None,
        });
        self.record_in_pass(&mut pass);
    }

    /// Appends ordered dispatches to a caller-owned compute pass, for example
    /// one whose beginning/end timestamps are supplied by a GPU profiler.
    pub fn record_in_pass(&self, pass: &mut wgpu::ComputePass<'_>) {
        let mut current_kernel: Option<*const Kernel> = None;
        for (kernel, bindings, groups) in &self.steps {
            let ptr = *kernel as *const Kernel;
            let same_pipeline = current_kernel == Some(ptr);
            kernel.record_in_pass_with_pipeline_state(pass, bindings, *groups, !same_pipeline);
            current_kernel = Some(ptr);
        }
    }

    /// Submits all steps once. Read back only the final outputs when needed.
    /// Queue writes made before this call are visible to all steps. Use distinct
    /// uniform buffers when different steps require different parameters.
    pub fn submit(&self, device: &Device, queue: &Queue) -> wgpu::SubmissionIndex {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("compute batch"),
        });
        self.record(&mut encoder);
        queue.submit([encoder.finish()])
    }
}
