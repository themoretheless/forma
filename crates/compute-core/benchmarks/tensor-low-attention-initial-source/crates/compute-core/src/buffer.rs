use wgpu::{Buffer, Device, Queue};

/// Uploads `data` as a storage buffer (STORAGE | COPY_DST).
pub fn storage_f32(device: &Device, queue: &Queue, data: &[f32]) -> Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("compute storage f32"),
        size: (data.len() * 4).max(4) as u64,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, &gpu_compute::pack_f32(data));
    buffer
}

/// Zero-initialized f32 storage buffer (read_write outputs).
pub fn storage_f32_zeroed(device: &Device, _queue: &Queue, len: usize) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("compute zeroed storage f32"),
        size: (len * 4).max(4) as u64,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

/// Same packing for u32 payloads (indices, counters).
pub fn storage_u32(device: &Device, queue: &Queue, data: &[u32]) -> Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("compute storage u32"),
        size: (data.len() * 4).max(4) as u64,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, &gpu_compute::pack_u32(data));
    buffer
}

/// Uniform buffer from raw float payloads (callers pack structs with
/// [`gpu_compute::pack_f32`] semantics: 16-byte alignment is the caller's job).
pub fn uniform_f32(device: &Device, queue: &Queue, floats: &[f32]) -> Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("compute uniform"),
        size: (floats.len() * 4).max(16) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, &gpu_compute::pack_f32(floats));
    buffer
}

/// Copies `buffer[0..size]` into a MAP_READ staging buffer and maps it.
/// Storage buffers cannot hold MAP_READ (wgpu restricts it to COPY_DST
/// companions), so readback goes through an explicit copy.
fn readback(
    device: &Device,
    queue: &Queue,
    buffer: &Buffer,
    size: usize,
) -> Result<Vec<u8>, gpu_compute::ReadbackError> {
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut ticket =
        gpu_compute::ByteReadback::copy_buffer(device, &mut encoder, buffer, 0, size as u64)?;
    ticket.submitted(queue.submit([encoder.finish()]));
    ticket.wait(std::time::Duration::from_secs(30))
}

pub fn try_read_f32(
    device: &Device,
    queue: &Queue,
    buffer: &Buffer,
    count: usize,
) -> Result<Vec<f32>, gpu_compute::ReadbackError> {
    let bytes = readback(
        device,
        queue,
        buffer,
        count
            .checked_mul(4)
            .ok_or(gpu_compute::ReadbackError::InvalidRange)?,
    )?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect())
}
pub fn try_read_u32(
    device: &Device,
    queue: &Queue,
    buffer: &Buffer,
    count: usize,
) -> Result<Vec<u32>, gpu_compute::ReadbackError> {
    let bytes = readback(
        device,
        queue,
        buffer,
        count
            .checked_mul(4)
            .ok_or(gpu_compute::ReadbackError::InvalidRange)?,
    )?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| u32::from_le_bytes(*chunk))
        .collect())
}

/// Compatibility convenience: panics on transport failure, never fabricates an
/// empty success. Use `try_read_f32` for recovery/fallback.
pub fn read_f32(device: &Device, queue: &Queue, buffer: &Buffer, count: usize) -> Vec<f32> {
    try_read_f32(device, queue, buffer, count).expect("GPU f32 readback failed")
}
pub fn read_u32(device: &Device, queue: &Queue, buffer: &Buffer, count: usize) -> Vec<u32> {
    try_read_u32(device, queue, buffer, count).expect("GPU u32 readback failed")
}
