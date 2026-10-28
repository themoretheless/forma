use wgpu::{Buffer, Device, Queue};

fn storage_usage(device: &Device) -> wgpu::BufferUsages {
    let base =
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
    if gpu_compute::device_memory_architecture(device) == gpu_compute::MemoryArchitecture::Uma {
        base | wgpu::BufferUsages::MAP_READ
    } else {
        base
    }
}

/// Uploads f32 values into fresh storage, directly mapped on Apple Silicon.
pub fn storage_f32(device: &Device, _queue: &Queue, data: &[f32]) -> Buffer {
    let bytes = gpu_compute::f32_le_bytes(data);
    initialized(
        device,
        "compute storage f32",
        &bytes,
        4,
        storage_usage(device),
    )
}

fn initialized(
    device: &Device,
    label: &str,
    bytes: &[u8],
    minimum_size: usize,
    usage: wgpu::BufferUsages,
) -> Buffer {
    gpu_compute::create_buffer_initialized(
        device,
        &wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes.len().max(minimum_size) as u64,
            usage,
            mapped_at_creation: true,
        },
        |mapping| mapping.into_slice(..bytes.len()).copy_from_slice(bytes),
    )
    .expect("GPU buffer initialization failed")
}

/// Zero-initialized f32 storage buffer (read_write outputs).
pub fn storage_f32_zeroed(device: &Device, _queue: &Queue, len: usize) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("compute zeroed storage f32"),
        size: (len * 4).max(4) as u64,
        usage: storage_usage(device),
        mapped_at_creation: false,
    })
}

/// Same packing for u32 payloads (indices, counters).
pub fn storage_u32(device: &Device, _queue: &Queue, data: &[u32]) -> Buffer {
    let bytes = gpu_compute::u32_le_bytes(data);
    initialized(
        device,
        "compute storage u32",
        &bytes,
        4,
        storage_usage(device),
    )
}

/// Uniform buffer from raw float payloads (callers pack structs with
/// [`gpu_compute::pack_f32`] semantics: 16-byte alignment is the caller's job).
pub fn uniform_f32(device: &Device, _queue: &Queue, floats: &[f32]) -> Buffer {
    let bytes = gpu_compute::f32_le_bytes(floats);
    initialized(
        device,
        "compute uniform",
        &bytes,
        16,
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    )
}

/// Snapshot readback through staging. The copy preserves independent results
/// when the source is reused while earlier readback tickets remain pending.
fn readback<R>(
    device: &Device,
    queue: &Queue,
    buffer: &Buffer,
    size: usize,
    consume: impl FnOnce(&[u8]) -> R,
) -> Result<R, gpu_compute::ReadbackError> {
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut ticket =
        gpu_compute::ByteReadback::copy_buffer(device, &mut encoder, buffer, 0, size as u64)?;
    ticket.submitted(queue.submit([encoder.finish()]));
    ticket.wait_with(std::time::Duration::from_secs(30), consume)
}

pub fn try_read_f32(
    device: &Device,
    queue: &Queue,
    buffer: &Buffer,
    count: usize,
) -> Result<Vec<f32>, gpu_compute::ReadbackError> {
    readback(
        device,
        queue,
        buffer,
        count
            .checked_mul(4)
            .ok_or(gpu_compute::ReadbackError::InvalidRange)?,
        crate::array::decode_elements::<f32>,
    )
}

pub fn try_read_u32(
    device: &Device,
    queue: &Queue,
    buffer: &Buffer,
    count: usize,
) -> Result<Vec<u32>, gpu_compute::ReadbackError> {
    readback(
        device,
        queue,
        buffer,
        count
            .checked_mul(4)
            .ok_or(gpu_compute::ReadbackError::InvalidRange)?,
        crate::array::decode_elements::<u32>,
    )
}

/// Compatibility convenience: panics on transport failure, never fabricates an
/// empty success. Use `try_read_f32` for recovery/fallback.
pub fn read_f32(device: &Device, queue: &Queue, buffer: &Buffer, count: usize) -> Vec<f32> {
    try_read_f32(device, queue, buffer, count).expect("GPU f32 readback failed")
}
pub fn read_u32(device: &Device, queue: &Queue, buffer: &Buffer, count: usize) -> Vec<u32> {
    try_read_u32(device, queue, buffer, count).expect("GPU u32 readback failed")
}
