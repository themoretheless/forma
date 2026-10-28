use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BufferError {
    InvalidRange,
    InvalidUsage,
    MissingUsage,
    ForeignDevice,
    TooLarge,
    UnalignedBinding,
    Device(String),
}
impl std::fmt::Display for BufferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GPU buffer: {self:?}")
    }
}
impl std::error::Error for BufferError {}

/// Storage created on a known device. There is intentionally no unchecked
/// constructor from a raw buffer and a caller-asserted device identity.
#[derive(Clone)]
pub struct GpuBuffer {
    context: crate::GpuContext,
    buffer: wgpu::Buffer,
}
impl GpuBuffer {
    pub fn new(
        context: &crate::GpuContext,
        bytes: u64,
        usage: wgpu::BufferUsages,
    ) -> Result<Self, BufferError> {
        let bytes = validate_allocation(context, bytes, usage)?;
        let buffer = create_buffer_checked(
            &context.device,
            &wgpu::BufferDescriptor {
                label: Some("owned GPU buffer"),
                size: bytes,
                usage,
                mapped_at_creation: false,
            },
        )?;
        Ok(Self {
            context: context.clone(),
            buffer,
        })
    }

    /// Initializes fresh storage before exposing it to GPU work. On Apple
    /// Silicon Metal the callback writes directly into the final shared buffer;
    /// elsewhere wgpu provides its portable mapped-at-creation staging path.
    /// The callback cannot retain the mapping, and the buffer is unmapped before
    /// return. Existing/aliased buffers must still be updated through the queue.
    pub fn new_initialized(
        context: &crate::GpuContext,
        bytes: u64,
        usage: wgpu::BufferUsages,
        initialize: impl FnOnce(wgpu::WriteOnly<'_, [u8]>),
    ) -> Result<Self, BufferError> {
        let bytes = validate_allocation(context, bytes, usage)?;
        let buffer = create_buffer_initialized(
            &context.device,
            &wgpu::BufferDescriptor {
                label: Some("initialized GPU buffer"),
                size: bytes,
                usage,
                mapped_at_creation: true,
            },
            initialize,
        )?;
        Ok(Self {
            context: context.clone(),
            buffer,
        })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.context.device
    }
    pub fn raw(&self) -> &wgpu::Buffer {
        &self.buffer
    }
    pub fn size(&self) -> u64 {
        self.buffer.size()
    }
    pub fn view(&self, range: Range<u64>) -> Result<GpuBufferView<'_>, BufferError> {
        if range.start > range.end
            || range.end > self.size()
            || !range.start.is_multiple_of(4)
            || !range.end.is_multiple_of(4)
        {
            return Err(BufferError::InvalidRange);
        }
        Ok(GpuBufferView {
            buffer: self,
            offset: range.start,
            bytes: range.end - range.start,
        })
    }
}

fn validate_allocation(
    context: &crate::GpuContext,
    bytes: u64,
    usage: wgpu::BufferUsages,
) -> Result<u64, BufferError> {
    if usage.is_empty()
        || (usage.intersects(wgpu::BufferUsages::BLAS_INPUT | wgpu::BufferUsages::TLAS_INPUT)
            && !context
                .enabled_features()
                .contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY))
        || (!context
            .enabled_features()
            .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
            && ((usage.contains(wgpu::BufferUsages::MAP_READ)
                && !(usage - wgpu::BufferUsages::MAP_READ - wgpu::BufferUsages::COPY_DST)
                    .is_empty())
                || (usage.contains(wgpu::BufferUsages::MAP_WRITE)
                    && !(usage - wgpu::BufferUsages::MAP_WRITE - wgpu::BufferUsages::COPY_SRC)
                        .is_empty())))
    {
        return Err(BufferError::InvalidUsage);
    }
    let bytes = bytes.max(4);
    let limits = &context.limits;
    if bytes > limits.max_buffer_size
        || (usage.contains(wgpu::BufferUsages::STORAGE)
            && bytes > limits.max_storage_buffer_binding_size)
    {
        return Err(BufferError::TooLarge);
    }
    if !bytes.is_multiple_of(4) {
        return Err(BufferError::InvalidRange);
    }
    Ok(bytes)
}

/// Classifies the active wgpu device into [`crate::MemoryArchitecture::Uma`]
/// (unified physical CPU/GPU RAM with `MAPPABLE_PRIMARY_BUFFERS` on Apple Silicon Metal)
/// or [`crate::MemoryArchitecture::Duma`] (discrete/decoupled VRAM requiring staging transfers).
pub fn device_memory_architecture(device: &wgpu::Device) -> crate::MemoryArchitecture {
    if cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && device
            .features()
            .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
        && device.adapter_info().backend == wgpu::Backend::Metal
    {
        crate::MemoryArchitecture::Uma
    } else {
        crate::MemoryArchitecture::Duma
    }
}

/// Whether fresh mapped uploads use the final Metal UMA shared allocation.
pub fn shared_memory_uploads(device: &wgpu::Device) -> bool {
    device_memory_architecture(device) == crate::MemoryArchitecture::Uma
}

/// Fill a new buffer without a host packing Vec. MAP_WRITE is essential here:
/// without it wgpu implements mapped_at_creation using a staging allocation,
/// even on Metal. Keep GPU-only outputs private by using ordinary allocation.
pub fn create_buffer_initialized(
    device: &wgpu::Device,
    descriptor: &wgpu::BufferDescriptor<'_>,
    initialize: impl FnOnce(wgpu::WriteOnly<'_, [u8]>),
) -> Result<wgpu::Buffer, BufferError> {
    if descriptor.usage.is_empty() {
        return Err(BufferError::InvalidUsage);
    }
    if descriptor.size == 0 || !descriptor.size.is_multiple_of(4) {
        return Err(BufferError::InvalidRange);
    }
    let mut descriptor = descriptor.clone();
    descriptor.mapped_at_creation = true;
    if shared_memory_uploads(device) {
        descriptor.usage |= wgpu::BufferUsages::MAP_WRITE;
    }
    // Browser error-scope futures require the JS event loop; do not block on
    // them from this synchronous compatibility API.
    #[cfg(target_arch = "wasm32")]
    let buffer = device.create_buffer(&descriptor);
    #[cfg(not(target_arch = "wasm32"))]
    let buffer = create_buffer_checked(device, &descriptor)?;
    let result = buffer
        .get_mapped_range_mut(..)
        .map(|mut mapping| initialize(mapping.slice(..)));
    buffer.unmap();
    result.map_err(|error| BufferError::Device(error.to_string()))?;
    Ok(buffer)
}

fn create_buffer_checked(
    device: &wgpu::Device,
    descriptor: &wgpu::BufferDescriptor<'_>,
) -> Result<wgpu::Buffer, BufferError> {
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let buffer = device.create_buffer(descriptor);
    // Pop every scope in reverse order before returning, including when an
    // earlier scope captured an error. Invalid handles must never escape in Ok.
    let errors = [
        crate::block_on(validation.pop()),
        crate::block_on(internal.pop()),
        crate::block_on(oom.pop()),
    ];
    if let Some(error) = errors.into_iter().flatten().next() {
        return Err(BufferError::Device(error.to_string()));
    }
    Ok(buffer)
}

#[derive(Clone, Copy)]
pub struct GpuBufferView<'a> {
    buffer: &'a GpuBuffer,
    offset: u64,
    bytes: u64,
}
impl<'a> GpuBufferView<'a> {
    pub fn validate(
        &self,
        context: &crate::GpuContext,
        usage: wgpu::BufferUsages,
    ) -> Result<(), BufferError> {
        if !self.buffer.context.same_device(context) {
            return Err(BufferError::ForeignDevice);
        }
        if !self.buffer.raw().usage().contains(usage) {
            return Err(BufferError::MissingUsage);
        }
        Ok(())
    }
    pub fn len_bytes(&self) -> u64 {
        self.bytes
    }
    pub fn is_empty(&self) -> bool {
        self.bytes == 0
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }
    pub fn raw(&self) -> &'a wgpu::Buffer {
        self.buffer.raw()
    }
    pub fn storage_binding(
        &self,
        context: &crate::GpuContext,
    ) -> Result<wgpu::BindingResource<'a>, BufferError> {
        self.validate(context, wgpu::BufferUsages::STORAGE)?;
        if !self.offset.is_multiple_of(u64::from(
            context.limits.min_storage_buffer_offset_alignment,
        )) {
            return Err(BufferError::UnalignedBinding);
        }
        let size = std::num::NonZeroU64::new(self.bytes).ok_or(BufferError::InvalidRange)?;
        Ok(wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: self.raw(),
            offset: self.offset,
            size: Some(size),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_allocation_validation_is_recoverable_and_scopes_are_balanced() {
        let Some(context) = crate::GpuContext::new() else {
            assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
            return;
        };
        let descriptor = wgpu::BufferDescriptor {
            label: Some("allocation validation fixture"),
            size: 16,
            usage: wgpu::BufferUsages::empty(),
            mapped_at_creation: false,
        };
        // Exercise the backend error path directly: public construction rejects
        // this descriptor earlier, while OOM cannot be forced safely in a test.
        let outer = context
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let result = create_buffer_checked(&context.device, &descriptor);
        assert!(matches!(result, Err(BufferError::Device(message)) if !message.is_empty()));
        assert!(crate::block_on(outer.pop()).is_none());

        let valid = GpuBuffer::new(&context, 16, wgpu::BufferUsages::STORAGE).unwrap();
        assert_eq!(valid.size(), 16);
    }
}
