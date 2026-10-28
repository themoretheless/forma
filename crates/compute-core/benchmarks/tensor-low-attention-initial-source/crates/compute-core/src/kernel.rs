mod contract;
use contract::reflect;
pub use contract::{BindingInfo, KernelBindingError};
use gpu_compute::block_on;
use wgpu::{BindGroupLayout, Buffer, ComputePipeline, Device, Queue};

/// The binding a kernel expects at `group(0)`, sequential from binding 0:
/// uniforms and read-only/read-write storage buffers in declaration order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Binding {
    Uniform,
    StorageRead,
    StorageReadWrite,
}

/// Why a kernel failed to build. Construction validates eagerly (via a
/// device error scope) so a broken WGSL source is a `Result`, not a panic.
#[derive(Clone, Debug)]
pub struct KernelError {
    pub message: String,
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "kernel build failed: {}", self.message)
    }
}

impl std::error::Error for KernelError {}

/// Textual anchor every tunable kernel source must declare.
const WG_ANCHOR: &str = "const WG: u32 = 256;";

/// A compiled compute kernel: shader module, binding layout and cached
/// pipeline, ready for repeated [`dispatch`](Kernel::dispatch).
pub struct Kernel {
    label: String,
    source: String,
    entry: String,
    bindings: Vec<Binding>,
    pipeline: ComputePipeline,
    bgl: BindGroupLayout,
    workgroup_size: u32,
    max_groups: u32,
    binding_info: Vec<BindingInfo>,
    context: Option<gpu_compute::GpuContext>,
}

impl Kernel {
    /// Compiles a one-dimensional compute entry point. Dispatch sizing follows
    /// its actual WGSL workgroup size, including sources without a `WG` anchor.
    pub fn new(
        device: &Device,
        label: &str,
        wgsl: &str,
        entry: &str,
        bindings: &[Binding],
    ) -> Result<Self, KernelError> {
        Self::compile(device, label, wgsl.to_owned(), entry, bindings, None)
    }

    /// Compiles `wgsl` after substituting the `WG` anchor with `workgroup_size`
    /// (must be a power of two — tree-reduction kernels halve the stride).
    pub fn with_workgroup_size(
        device: &Device,
        label: &str,
        wgsl: &str,
        entry: &str,
        bindings: &[Binding],
        workgroup_size: u32,
    ) -> Result<Self, KernelError> {
        let (source, selected) = Self::tuned_source(device, wgsl, workgroup_size)?;
        Self::compile(device, label, source, entry, bindings, Some(selected))
    }

    pub(crate) fn tuned_source(
        device: &Device,
        wgsl: &str,
        requested: u32,
    ) -> Result<(String, u32), KernelError> {
        if !requested.is_power_of_two() {
            return Err(KernelError {
                message: format!("workgroup size {requested} is not a power of two"),
            });
        }
        let limits = device.limits();
        let max_size = limits
            .max_compute_invocations_per_workgroup
            .min(limits.max_compute_workgroup_size_x)
            .max(1);
        let mut selected = requested;
        while selected > max_size {
            selected /= 2;
        }
        let source = if selected == 256 {
            wgsl.to_owned()
        } else {
            if !wgsl.contains(WG_ANCHOR) {
                return Err(KernelError {
                    message: format!("WG anchor ({WG_ANCHOR}) missing from kernel source"),
                });
            }
            wgsl.replacen(WG_ANCHOR, &format!("const WG: u32 = {selected};"), 1)
        };
        Ok((source, selected))
    }

    /// Compiles on a tracked context, enabling checked borrowed-buffer bindings.
    pub fn from_context(
        context: &gpu_compute::GpuContext,
        label: &str,
        wgsl: &str,
        entry: &str,
        bindings: &[Binding],
    ) -> Result<Self, KernelError> {
        let mut kernel = Self::new(&context.device, label, wgsl, entry, bindings)?;
        kernel.context = Some(context.clone());
        Ok(kernel)
    }

    fn compile(
        device: &Device,
        label: &str,
        source: String,
        entry: &str,
        bindings: &[Binding],
        selected: Option<u32>,
    ) -> Result<Self, KernelError> {
        let (workgroup_size, binding_info) = reflect(&source, entry, bindings)?;
        if selected.is_some_and(|selected| selected != workgroup_size) {
            return Err(KernelError {
                message: format!(
                    "requested WG {} but entry {entry} declares {workgroup_size}; the tuning anchor must control the entry point",
                    selected.unwrap()
                ),
            });
        }
        let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.clone().into()),
        });
        let entries: Vec<wgpu::BindGroupLayoutEntry> = bindings
            .iter()
            .enumerate()
            // Entry points may share one group while using different subsets.
            // Keep layouts structural; `bind` enforces reflected minimum sizes.
            .map(|(index, binding)| match binding {
                Binding::Uniform => gpu_compute::uniform_entry(index as u32),
                Binding::StorageRead => gpu_compute::storage_entry(index as u32, true),
                Binding::StorageReadWrite => gpu_compute::storage_entry(index as u32, false),
            })
            .collect();
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let errors = [
            block_on(scope.pop()),
            block_on(internal.pop()),
            block_on(oom.pop()),
        ];
        if let Some(error) = errors.into_iter().flatten().next() {
            return Err(KernelError {
                message: error.to_string(),
            });
        }
        Ok(Self {
            label: label.to_string(),
            source,
            entry: entry.to_string(),
            bindings: bindings.to_vec(),
            pipeline,
            bgl,
            workgroup_size,
            max_groups: device.limits().max_compute_workgroups_per_dimension,
            binding_info,
            context: None,
        })
    }

    /// Compiles with a workgroup size tuned per backend via
    /// [`gpu_compute::tuned_workgroup_size`]: smaller groups on Metal
    /// (tile-based GPUs), larger elsewhere.
    pub fn tuned(
        context: &gpu_compute::GpuContext,
        label: &str,
        wgsl: &str,
        entry: &str,
        bindings: &[Binding],
        metal_size: u32,
        default_size: u32,
    ) -> Result<Self, KernelError> {
        let size = gpu_compute::tuned_workgroup_size(context.backend, metal_size, default_size);
        Self::with_workgroup_size(&context.device, label, wgsl, entry, bindings, size)
    }

    pub fn workgroup_size(&self) -> u32 {
        self.workgroup_size
    }

    /// The WGSL source after `WG` substitution, as compiled.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The shader entry point this pipeline binds.
    pub fn entry_point(&self) -> &str {
        &self.entry
    }

    /// The bind group layout buffers must match to be dispatched — lets
    /// callers cache bind groups alongside their grow-only buffer pools and
    /// dispatch them via [`dispatch_bind_group`](Kernel::dispatch_bind_group).
    pub fn bind_group_layout(&self) -> &BindGroupLayout {
        &self.bgl
    }

    /// Number of workgroups needed to cover `invocations` threads.
    pub fn workgroup_count(&self, invocations: u32) -> u32 {
        invocations.div_ceil(self.workgroup_size)
    }

    /// Largest invocation count a single dispatch covers: the device's
    /// workgroup-count limit times the actual WGSL workgroup size.
    /// Larger workloads are the caller's job to chunk.
    pub fn max_dispatch_invocations(&self) -> u32 {
        self.max_groups.saturating_mul(self.workgroup_size)
    }

    /// Dispatches over `buffers` (one per declared [`Binding`], binding 0..n),
    /// covering `invocations` threads.
    pub fn dispatch(&self, device: &Device, queue: &Queue, buffers: &[&Buffer], invocations: u32) {
        assert!(
            invocations <= self.max_dispatch_invocations(),
            "{}: {} invocations exceed the single-dispatch limit of {} (chunk the workload)",
            self.label,
            invocations,
            self.max_dispatch_invocations()
        );
        self.dispatch_groups(device, queue, buffers, self.workgroup_count(invocations))
    }

    /// Dispatches exactly `groups` workgroups — for grid-strided kernels
    /// (e.g. [`crate::shaders::BLOCK_SUM_WGSL`]) that cover arbitrary lengths with a
    /// fixed grid. `groups` must be within the 65535 per-dimension limit.
    pub fn dispatch_groups(
        &self,
        device: &Device,
        queue: &Queue,
        buffers: &[&Buffer],
        groups: u32,
    ) {
        let bind_group = self.create_bind_group(device, buffers);
        self.dispatch_bind_group(device, queue, &bind_group, groups);
    }

    /// Creates bindings once for repeated dispatches or recorded command chains.
    pub fn create_bind_group(&self, device: &Device, buffers: &[&Buffer]) -> wgpu::BindGroup {
        let resources: Vec<_> = buffers
            .iter()
            .map(|buffer| buffer.as_entire_binding())
            .collect();
        self.create_bind_group_resources(device, &resources)
    }

    /// Binds validated subranges supplied by domain adapters. Resources must
    /// match the declared layout; use GpuBufferView to validate owners and usage.
    pub fn create_bind_group_resources(
        &self,
        device: &Device,
        resources: &[wgpu::BindingResource<'_>],
    ) -> wgpu::BindGroup {
        assert_eq!(
            resources.len(),
            self.bindings.len(),
            "{}: wrong buffer count",
            self.label
        );
        let entries: Vec<_> = resources
            .iter()
            .enumerate()
            .map(|(index, resource)| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: resource.clone(),
            })
            .collect();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&self.label),
            layout: &self.bgl,
            entries: &entries,
        })
    }

    /// Dispatches with a caller-cached bind group (built against
    /// [`bind_group_layout`](Kernel::bind_group_layout)) — the fast path for
    /// repeated dispatches over grow-only buffer pools.
    pub fn dispatch_bind_group(
        &self,
        device: &Device,
        queue: &Queue,
        bind_group: &wgpu::BindGroup,
        groups: u32,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some(&self.label),
        });
        self.record_dispatch(&mut encoder, bind_group, groups);
        queue.submit([encoder.finish()]);
    }

    /// Records a dispatch into a caller-owned encoder — the building block
    /// for multi-kernel command buffers that submit once (batch pipelines),
    /// where each dispatch may bind different buffers.
    pub fn record_dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind_group: &wgpu::BindGroup,
        groups: u32,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(&self.label),
            timestamp_writes: None,
        });
        self.record_in_pass(&mut pass, bind_group, groups);
    }

    /// Appends a dispatch to an existing compute pass. wgpu tracks resource
    /// usage per dispatch and inserts barriers between dependent dispatches,
    /// including when a previous output becomes the next input.
    pub fn record_in_pass(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        bind_group: &wgpu::BindGroup,
        groups: u32,
    ) {
        assert!(
            (1..=self.max_groups).contains(&groups),
            "{}: workgroup count {} outside 1..={}",
            self.label,
            groups,
            self.max_groups
        );
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(groups, 1, 1);
    }
}
