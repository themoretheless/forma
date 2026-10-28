use crate::{
    Binding, ComputeBatch, ComputeError, ComputeProgram, GpuArray, GpuElement, Kernel, Readback,
    gpu_compute::GpuContext, shaders, wgpu,
};
use std::sync::{Arc, OnceLock};

/// Typed array runtime sharing an existing GPU device and queue. Pipelines are
/// compiled once. Arrays are tied to this runtime; cloned arrays share storage.
pub struct ComputeRuntime {
    context: GpuContext,
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) owner: Arc<()>,
    pub(crate) affine: Kernel,
    pub(crate) unary: Kernel,
    pub(crate) binary: Kernel,
    pub(crate) compare: Kernel,
    pub(crate) matmul: crate::matrix::MatmulKernels,
    pub(crate) sum: Kernel,
    pub(crate) tensor: OnceLock<crate::tensor::TensorKernels>,
    pub(crate) tensor_index: OnceLock<crate::tensor::TensorIndexKernels>,
    pub(crate) tensor_scatter: OnceLock<crate::tensor::TensorScatterKernels>,
    pub(crate) tensor_low: OnceLock<crate::tensor::LowKernels>,
    pub(crate) tensor_low_stats: OnceLock<crate::tensor::StatsKernels>,
    pub(crate) tensor_stats: OnceLock<crate::tensor::StatsKernels>,
    pub(crate) tensor_low_attention: OnceLock<crate::tensor::AttentionKernels>,
    pub(crate) tensor_attention: OnceLock<crate::tensor::AttentionKernels>,
    dot: OnceLock<crate::FusedSumKernel>,
    pub(crate) scan: crate::scan::ScanKernels,
    pub(crate) selection: crate::selection::SelectionKernels,
}

impl ComputeRuntime {
    pub fn new(context: &GpuContext) -> Result<Self, ComputeError> {
        let device = &context.device;
        let unary = [
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ];
        Ok(Self {
            context: context.clone(),
            device: device.clone(),
            queue: context.queue.clone(),
            owner: Arc::new(()),
            unary: Kernel::new(device, "array unary", shaders::UNARY_WGSL, "main", &unary)?,
            affine: Kernel::new(device, "array affine", shaders::AFFINE_WGSL, "main", &unary)?,
            binary: Kernel::new(
                device,
                "array binary",
                shaders::BINARY_WGSL,
                "main",
                &[
                    Binding::Uniform,
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                ],
            )?,
            compare: Kernel::new(
                device,
                "array compare",
                shaders::COMPARE_WGSL,
                "main",
                &[
                    Binding::Uniform,
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                ],
            )?,
            matmul: crate::matrix::MatmulKernels::new(device)?,
            sum: Kernel::new(device, "array sum", shaders::BLOCK_SUM_WGSL, "main", &unary)?,
            tensor: OnceLock::new(),
            tensor_index: OnceLock::new(),
            tensor_scatter: OnceLock::new(),
            tensor_low: OnceLock::new(),
            tensor_stats: OnceLock::new(),
            tensor_low_stats: OnceLock::new(),
            tensor_attention: OnceLock::new(),
            tensor_low_attention: OnceLock::new(),
            dot: OnceLock::new(),
            scan: crate::scan::ScanKernels::new(device)?,
            selection: crate::selection::SelectionKernels::new(device)?,
        })
    }

    /// Allocates zero-initialized storage without constructing or uploading a
    /// host-side zero vector. wgpu initializes new buffers before first use.
    pub fn zeros<T: GpuElement>(&self, len: usize) -> Result<GpuArray<T>, ComputeError> {
        let bytes = (len as u64).saturating_mul(4).max(4);
        let limits = self.device.limits();
        let limit = limits
            .max_buffer_size
            .min(limits.max_storage_buffer_binding_size)
            .min(u64::from(u32::MAX) * 4);
        if bytes > limit {
            return Err(ComputeError::TooLarge { bytes, limit });
        }
        let buffer = gpu_compute::GpuBuffer::new(
            &self.context,
            bytes,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        )?;
        Ok(GpuArray::new(buffer, self.owner.clone(), len as u32))
    }

    pub fn upload<T: GpuElement>(&self, values: &[T]) -> Result<GpuArray<T>, ComputeError> {
        let array = self.zeros(values.len())?;
        self.write(&array, 0, values)?;
        Ok(array)
    }

    /// Appends a validated readback to a caller-owned encoder. After submitting
    /// that encoder, mark the ticket with `Readback::submitted`.
    pub fn record_read<T: GpuElement>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        array: &GpuArray<T>,
    ) -> Result<Readback<T>, ComputeError> {
        self.check(array)?;
        Readback::record(&self.device, encoder, array)
    }

    pub fn context(&self) -> &GpuContext {
        &self.context
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Imports storage with an explicit scalar interpretation. Ownership, usage
    /// and length are checked; cloned handles retain alias identity.
    pub fn import_buffer<T: GpuElement>(
        &self,
        buffer: gpu_compute::GpuBuffer,
        len: usize,
    ) -> Result<GpuArray<T>, ComputeError> {
        let size = (len as u64)
            .checked_mul(4)
            .ok_or(ComputeError::OutOfBounds)?;
        if len > u32::MAX as usize {
            return Err(ComputeError::OutOfBounds);
        }
        buffer.view(0..size)?.validate(
            &self.context,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        )?;
        Ok(GpuArray::new(buffer, self.owner.clone(), len as u32))
    }

    /// Updates a range before the next submission. Existing prepared programs
    /// retain the same buffer. Writes are ordered before the next queued work,
    /// not between individual steps of a program.
    pub fn write<T: GpuElement>(
        &self,
        array: &GpuArray<T>,
        offset: usize,
        values: &[T],
    ) -> Result<(), ComputeError> {
        self.check(array)?;
        if offset
            .checked_add(values.len())
            .is_none_or(|end| end > array.len())
        {
            return Err(ComputeError::OutOfBounds);
        }
        if !values.is_empty() {
            let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_bytes()).collect();
            self.queue
                .write_buffer(array.buffer(), offset as u64 * 4, &bytes);
        }
        Ok(())
    }

    pub fn program(&self) -> ComputeProgram<'_> {
        ComputeProgram {
            runtime: self,
            batch: ComputeBatch::new(),
        }
    }

    pub(crate) fn dot_kernel(&self) -> Result<&crate::FusedSumKernel, ComputeError> {
        if let Some(kernel) = self.dot.get() {
            return Ok(kernel);
        }
        let kernel = crate::FusedSumKernel::dot(&self.context)?;
        // Concurrent first calls may compile twice; retain the first successful
        // pipeline without changing the runtime's Send/Sync properties.
        let _ = self.dot.set(kernel);
        Ok(self.dot.get().expect("dot pipeline was initialized"))
    }

    /// Read an existing array without running a program.
    pub fn read<T: GpuElement>(&self, array: &GpuArray<T>) -> Result<Readback<T>, ComputeError> {
        self.program().submit_read(array)
    }

    pub(crate) fn check<T: GpuElement>(&self, array: &GpuArray<T>) -> Result<(), ComputeError> {
        if !Arc::ptr_eq(&self.owner, &array.allocation.owner) {
            return Err(ComputeError::ForeignArray);
        }
        Ok(())
    }
}
