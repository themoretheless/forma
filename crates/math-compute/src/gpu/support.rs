#[cfg(target_arch = "wasm32")]
pub(super) use crate::M3;
pub(super) use crate::V3;
#[cfg(target_arch = "wasm32")]
pub(super) use compute_core::gpu_compute::pack_f32;
pub(super) use compute_core::gpu_compute::wgpu;
pub(super) use compute_core::gpu_compute::{GpuContext, pack_u32};
pub(super) use compute_core::{
    Binding, Kernel, try_read_f32 as read_f32, try_read_u32 as read_u32,
};
pub(super) use wgpu::{BindGroup, Buffer, BufferUsages, Device};

pub(super) const WG_METAL: u32 = 256;
pub(super) const WG_DEFAULT: u32 = 256;

pub(super) fn mk(device: &Device, label: &str, size: u64, usage: BufferUsages) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(4),
        usage,
        mapped_at_creation: false,
    })
}

#[cfg(target_arch = "wasm32")]
pub(super) fn push_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// Uniform + read storage + write storage bindings, the common shape.
#[cfg(target_arch = "wasm32")]
pub(super) const UNIFORM_STORAGE2: [Binding; 4] = [
    Binding::Uniform,
    Binding::StorageRead,
    Binding::StorageReadWrite,
    Binding::StorageReadWrite,
];

/// Uniform + two read-only inputs + two read-write outputs (nearest-neighbor).
pub(super) const NN_BINDINGS: [Binding; 5] = [
    Binding::Uniform,
    Binding::StorageRead,
    Binding::StorageRead,
    Binding::StorageReadWrite,
    Binding::StorageReadWrite,
];

/// Uniform + two read storages + one write storage (pairwise kernels).
#[cfg(target_arch = "wasm32")]
pub(super) const UNIFORM_PAIR: [Binding; 4] = [
    Binding::Uniform,
    Binding::StorageRead,
    Binding::StorageRead,
    Binding::StorageReadWrite,
];

/// Uniform + read points + one partial-output reduction.
#[cfg(target_arch = "wasm32")]
pub(super) const UNIFORM_REDUCE1: [Binding; 3] = [
    Binding::Uniform,
    Binding::StorageRead,
    Binding::StorageReadWrite,
];

/// Convert xyz directly into queue staging instead of a float Vec + byte Vec.
pub(super) fn write_points(
    queue: &wgpu::Queue,
    buffer: &Buffer,
    points: &[V3],
) -> Result<(), gpu_compute::ReadbackError> {
    gpu_compute::write_buffer_words(
        queue,
        buffer,
        0,
        points
            .as_flattened()
            .iter()
            .map(|&v| (v as f32).to_le_bytes()),
    )
    .map_err(|error| gpu_compute::ReadbackError::Device(error.to_string()))
}
