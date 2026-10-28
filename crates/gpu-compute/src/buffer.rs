use std::borrow::Cow;

/// Encode words directly in queue staging memory. This preserves write_buffer
/// ordering before the next submission, without mapping an in-flight resource
/// or allocating a host packing Vec. Dropping the view schedules the write.
/// As with write_buffer_with, a panicking iterator may schedule a partial write.
pub fn write_buffer_words(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    offset: u64,
    words: impl ExactSizeIterator<Item = [u8; 4]>,
) -> Result<(), crate::BufferError> {
    let bytes = (words.len() as u64)
        .checked_mul(4)
        .ok_or(crate::BufferError::TooLarge)?;
    if !offset.is_multiple_of(4)
        || offset
            .checked_add(bytes)
            .is_none_or(|end| end > buffer.size())
    {
        return Err(crate::BufferError::InvalidRange);
    }
    if !buffer.usage().contains(wgpu::BufferUsages::COPY_DST) {
        return Err(crate::BufferError::MissingUsage);
    }
    let Some(size) = std::num::NonZeroU64::new(bytes) else {
        return Ok(());
    };
    let mut staging = queue
        .write_buffer_with(buffer, offset, size)
        .ok_or_else(|| crate::BufferError::Device("queue staging allocation failed".into()))?;
    staging.slice(..).into_chunks::<4>().0.write_iter(words);
    Ok(())
}

/// Join contiguous byte slices into one immediate queue write, up to 4 KiB.
/// Validates the entire range before mutation. No submit, map or GPU wait.
/// Packing is stack-local; a single nonempty part is borrowed directly.
pub fn write_small_parts<'a>(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    offset: u64,
    parts: impl Iterator<Item = &'a [u8]> + Clone,
) -> Result<(), crate::BufferError> {
    let size = parts
        .clone()
        .try_fold(0usize, |n, p| n.checked_add(p.len()))
        .ok_or(crate::BufferError::TooLarge)?;
    if size > 4096 {
        return Err(crate::BufferError::TooLarge);
    }
    if !offset.is_multiple_of(4)
        || !size.is_multiple_of(4)
        || offset
            .checked_add(size as u64)
            .is_none_or(|end| end > buffer.size())
    {
        return Err(crate::BufferError::InvalidRange);
    }
    if !buffer.usage().contains(wgpu::BufferUsages::COPY_DST) {
        return Err(crate::BufferError::MissingUsage);
    }
    if size == 0 {
        return Ok(());
    }
    if let Some(part) = parts.clone().find(|p| p.len() == size) {
        queue.write_buffer(buffer, offset, part);
        return Ok(());
    }
    let mut packed = [0u8; 4096];
    let mut cursor = 0;
    for part in parts {
        packed[cursor..cursor + part.len()].copy_from_slice(part);
        cursor += part.len();
    }
    queue.write_buffer(buffer, offset, &packed[..size]);
    Ok(())
}

/// Storage-buffer binding layout entry used by all GPU stages.
pub fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Blocking readback of a copy-destination buffer into a fresh Vec.
/// Returns an empty Vec on map failure, letting callers treat it as unavailability.
/// This function owns mapping and unmapping; callers must not unmap afterwards.
/// Prefer `try_read_buffer` to distinguish transport failure from empty data.
pub fn read_buffer(device: &wgpu::Device, buffer: &wgpu::Buffer, size: usize) -> Vec<u8> {
    crate::try_read_buffer(device, buffer, size).unwrap_or_default()
}

/// Packs values into little-endian bytes for GPU buffers (LE is the WGSL wire
/// order; on the little-endian target platforms this equals `to_ne_bytes`).
pub fn pack_f32(values: &[f32]) -> Vec<u8> {
    f32_le_bytes(values).into_owned()
}

/// Borrow the native representation on LE hosts; encode only on BE hosts.
pub fn f32_le_bytes(values: &[f32]) -> Cow<'_, [u8]> {
    #[cfg(target_endian = "little")]
    {
        // SAFETY: f32 has no padding or uninitialized bytes, and u8 has alignment 1.
        let bytes = unsafe {
            std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
        };
        Cow::Borrowed(bytes)
    }
    #[cfg(not(target_endian = "little"))]
    {
        let mut out = Vec::with_capacity(values.len() * 4);
        for value in values {
            out.extend_from_slice(&value.to_le_bytes());
        }
        Cow::Owned(out)
    }
}

/// Same packing for u32 payloads (indices, aux records).
pub fn pack_u32(values: &[u32]) -> Vec<u8> {
    u32_le_bytes(values).into_owned()
}

/// Borrow the native representation on LE hosts; encode only on BE hosts.
pub fn u32_le_bytes(values: &[u32]) -> Cow<'_, [u8]> {
    #[cfg(target_endian = "little")]
    {
        // SAFETY: u32 has no padding or uninitialized bytes, and u8 has alignment 1.
        let bytes = unsafe {
            std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
        };
        Cow::Borrowed(bytes)
    }
    #[cfg(not(target_endian = "little"))]
    {
        let mut out = Vec::with_capacity(values.len() * 4);
        for value in values {
            out.extend_from_slice(&value.to_le_bytes());
        }
        Cow::Owned(out)
    }
}
