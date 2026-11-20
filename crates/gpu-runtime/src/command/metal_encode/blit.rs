//! Metal blit, buffer/texture copy, and timestamp resolution encoding.

use super::super::*;
use super::metal_copy_strides;
use std::ffi::c_void;
use std::ptr;

/// The corner of a copy inside one mip level of a texture.
struct TextureRegion<'a> {
    texture: &'a Texture,
    mip_level: u32,
    origin: Origin3d,
}

/// Where the rows of a texture copy live in a buffer.
struct BufferRows<'a> {
    buffer: &'a Buffer,
    offset: BufferAddress,
    bytes_per_row: u32,
    rows_per_image: Option<u32>,
}

/// # Safety
///
/// `cmd_buf` must be an uncommitted `MTLCommandBuffer` belonging to `metal` with no active
/// command encoder currently open on it.
pub(crate) unsafe fn encode_non_compute_op(
    metal: &MetalDevice,
    cmd_buf: *mut c_void,
    op: RecordedOp,
) {
    match op {
        RecordedOp::CopyBufferToBuffer {
            src,
            src_offset,
            dst,
            dst_offset,
            size,
        } => {
            // SAFETY: Caller guarantees `cmd_buf` is an open uncommitted command buffer.
            unsafe {
                encode_buffer_copy(cmd_buf, &src, src_offset, &dst, dst_offset, size);
            }
        }
        RecordedOp::ClearBuffer { dst, offset, size } => {
            // SAFETY: Caller guarantees `cmd_buf` is an open uncommitted command buffer.
            unsafe {
                encode_buffer_clear(cmd_buf, &dst, offset, size);
            }
        }
        RecordedOp::CopyTextureToBuffer {
            src_tex,
            mip_level,
            origin,
            dst_buf,
            dst_offset,
            bytes_per_row,
            rows_per_image,
            size,
        } => {
            // SAFETY: Caller guarantees `cmd_buf` is an open uncommitted command buffer.
            unsafe {
                encode_texture_to_buffer(
                    cmd_buf,
                    TextureRegion {
                        texture: &src_tex,
                        mip_level,
                        origin,
                    },
                    BufferRows {
                        buffer: &dst_buf,
                        offset: dst_offset,
                        bytes_per_row,
                        rows_per_image,
                    },
                    size,
                );
            }
        }
        RecordedOp::CopyBufferToTexture {
            src_buf,
            src_offset,
            bytes_per_row,
            rows_per_image,
            dst_tex,
            dst_mip,
            dst_origin,
            size,
        } => {
            // SAFETY: Caller guarantees `cmd_buf` is an open uncommitted command buffer.
            unsafe {
                encode_buffer_to_texture(
                    cmd_buf,
                    BufferRows {
                        buffer: &src_buf,
                        offset: src_offset,
                        bytes_per_row,
                        rows_per_image,
                    },
                    TextureRegion {
                        texture: &dst_tex,
                        mip_level: dst_mip,
                        origin: dst_origin,
                    },
                    size,
                );
            }
        }
        RecordedOp::CopyTextureToTexture {
            src_tex,
            src_mip,
            src_origin,
            dst_tex,
            dst_mip,
            dst_origin,
            size,
        } => {
            // SAFETY: Caller guarantees `cmd_buf` is an open uncommitted command buffer.
            unsafe {
                encode_texture_to_texture(
                    cmd_buf,
                    TextureRegion {
                        texture: &src_tex,
                        mip_level: src_mip,
                        origin: src_origin,
                    },
                    TextureRegion {
                        texture: &dst_tex,
                        mip_level: dst_mip,
                        origin: dst_origin,
                    },
                    size,
                );
            }
        }
        RecordedOp::WriteTimestamp { query_set, index } => record_timestamp(&query_set, index),
        RecordedOp::ResolveQuerySet {
            query_set,
            first_query,
            query_count,
            destination,
            destination_offset,
        } => resolve_timestamps(
            &query_set,
            first_query,
            query_count,
            &destination,
            destination_offset,
        ),
        RecordedOp::RenderPass(rp) => {
            // SAFETY: Caller guarantees `cmd_buf` is open and compatible with `metal`.
            unsafe {
                super::encode_render_pass(metal, cmd_buf, rp);
            }
        }
        RecordedOp::ComputePass { .. } => {}
    }
}

/// # Safety
///
/// `cmd_buf` must be an uncommitted `MTLCommandBuffer` and `src`/`dst` must contain valid
/// `MTLBuffer` objects for the encoder-validated ranges.
unsafe fn encode_buffer_copy(
    cmd_buf: *mut c_void,
    src: &Buffer,
    src_offset: BufferAddress,
    dst: &Buffer,
    dst_offset: BufferAddress,
    size: BufferAddress,
) {
    if size == 0 {
        return;
    }
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    let f_copy: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        usize,
        *mut c_void,
        usize,
        usize,
    ) = msg_send();
    // SAFETY: `cmd_buf` is valid; the created blit encoder is ended before returning.
    unsafe {
        let blit = f_obj(cmd_buf, s.blit_command_encoder);
        f_copy(
            blit,
            s.copy_from_buffer,
            src.inner.raw.as_ptr(),
            src_offset as usize,
            dst.inner.raw.as_ptr(),
            dst_offset as usize,
            size as usize,
        );
        f_void(blit, s.end_encoding);
    }
}

/// # Safety
///
/// `cmd_buf` must be an uncommitted `MTLCommandBuffer` and `dst` must hold a valid `MTLBuffer`.
unsafe fn encode_buffer_clear(
    cmd_buf: *mut c_void,
    dst: &Buffer,
    offset: BufferAddress,
    size: BufferAddress,
) {
    if size == 0 {
        return;
    }
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    let f_fill: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, NSRange, u8) =
        msg_send();
    // SAFETY: `cmd_buf` is valid and `offset..offset + size` was range-validated by the encoder.
    unsafe {
        let blit = f_obj(cmd_buf, s.blit_command_encoder);
        f_fill(
            blit,
            s.fill_buffer,
            dst.inner.raw.as_ptr(),
            NSRange {
                location: offset as usize,
                length: size as usize,
            },
            0,
        );
        f_void(blit, s.end_encoding);
    }
}

/// # Safety
///
/// `cmd_buf` must be an uncommitted `MTLCommandBuffer` and `source`/`destination` must hold live
/// Metal objects with validated bounds.
unsafe fn encode_texture_to_buffer(
    cmd_buf: *mut c_void,
    source: TextureRegion<'_>,
    destination: BufferRows<'_>,
    size: Extent3d,
) {
    let TextureRegion {
        texture: src_tex,
        mip_level,
        origin,
    } = source;
    let BufferRows {
        buffer: dst_buf,
        offset: dst_offset,
        bytes_per_row,
        rows_per_image,
    } = destination;
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    let f_t2b: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        usize,
        usize,
        MTLOrigin,
        MTLSize,
        *mut c_void,
        usize,
        usize,
        usize,
    ) = msg_send();
    let (bpr, bpi) = metal_copy_strides(bytes_per_row, rows_per_image, size, src_tex.format, false);
    // SAFETY: `cmd_buf`, `src_tex.raw`, and `dst_buf.inner.raw` are valid Metal handles.
    unsafe {
        let blit = f_obj(cmd_buf, s.blit_command_encoder);
        f_t2b(
            blit,
            s.copy_from_texture_to_buffer,
            src_tex.raw.as_ptr(),
            origin.z as usize,
            mip_level as usize,
            MTLOrigin {
                x: origin.x as usize,
                y: origin.y as usize,
                z: 0,
            },
            MTLSize {
                width: size.width as usize,
                height: size.height as usize,
                depth: 1,
            },
            dst_buf.inner.raw.as_ptr(),
            dst_offset as usize,
            bpr,
            bpi,
        );
        f_void(blit, s.end_encoding);
    }
}

/// # Safety
///
/// `cmd_buf` must be an uncommitted `MTLCommandBuffer` and `source`/`destination` must hold live
/// Metal objects with validated bounds.
unsafe fn encode_buffer_to_texture(
    cmd_buf: *mut c_void,
    source: BufferRows<'_>,
    destination: TextureRegion<'_>,
    size: Extent3d,
) {
    let BufferRows {
        buffer: src_buf,
        offset: src_offset,
        bytes_per_row,
        rows_per_image,
    } = source;
    let TextureRegion {
        texture: dst_tex,
        mip_level: dst_mip,
        origin: dst_origin,
    } = destination;
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    let f_b2t: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        usize,
        usize,
        usize,
        MTLSize,
        *mut c_void,
        usize,
        usize,
        MTLOrigin,
    ) = msg_send();
    let slices = crate::texture::CopySlices::new(
        dst_tex.dimension,
        dst_origin.z,
        size.depth_or_array_layers,
    );
    let (bpr, bpi) = metal_copy_strides(
        bytes_per_row,
        rows_per_image,
        size,
        dst_tex.format,
        slices.is_3d() || size.depth_or_array_layers > 1,
    );
    // SAFETY: `cmd_buf`, `src_buf.inner.raw`, and `dst_tex.raw` are valid Metal handles.
    unsafe {
        let blit = f_obj(cmd_buf, s.blit_command_encoder);
        for slice in 0..slices.count {
            f_b2t(
                blit,
                s.copy_from_buffer_to_texture,
                src_buf.inner.raw.as_ptr(),
                src_offset as usize + slice * bpi,
                bpr,
                bpi,
                MTLSize {
                    width: size.width as usize,
                    height: size.height as usize,
                    depth: slices.depth,
                },
                dst_tex.raw.as_ptr(),
                slices.array_slice(slice),
                dst_mip as usize,
                MTLOrigin {
                    x: dst_origin.x as usize,
                    y: dst_origin.y as usize,
                    z: slices.region_z(),
                },
            );
        }
        f_void(blit, s.end_encoding);
    }
}

/// # Safety
///
/// `cmd_buf` must be an uncommitted `MTLCommandBuffer` and `source`/`destination` must hold live
/// `MTLTexture` objects with validated bounds.
unsafe fn encode_texture_to_texture(
    cmd_buf: *mut c_void,
    source: TextureRegion<'_>,
    destination: TextureRegion<'_>,
    size: Extent3d,
) {
    let TextureRegion {
        texture: src_tex,
        mip_level: src_mip,
        origin: src_origin,
    } = source;
    let TextureRegion {
        texture: dst_tex,
        mip_level: dst_mip,
        origin: dst_origin,
    } = destination;
    let s = metal_selectors();
    let f_obj: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = msg_send();
    let f_void: unsafe extern "C" fn(*mut c_void, *mut c_void) = msg_send();
    let f_t2t: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        usize,
        usize,
        MTLOrigin,
        MTLSize,
        *mut c_void,
        usize,
        usize,
        MTLOrigin,
    ) = msg_send();
    let slices = size.depth_or_array_layers.max(1) as usize;
    // SAFETY: `cmd_buf`, `src_tex.raw`, and `dst_tex.raw` are valid Metal handles.
    unsafe {
        let blit = f_obj(cmd_buf, s.blit_command_encoder);
        for slice_i in 0..slices {
            f_t2t(
                blit,
                s.copy_from_texture_to_texture,
                src_tex.raw.as_ptr(),
                src_origin.z as usize + slice_i,
                src_mip as usize,
                MTLOrigin {
                    x: src_origin.x as usize,
                    y: src_origin.y as usize,
                    z: 0,
                },
                MTLSize {
                    width: size.width as usize,
                    height: size.height as usize,
                    depth: 1,
                },
                dst_tex.raw.as_ptr(),
                dst_origin.z as usize + slice_i,
                dst_mip as usize,
                MTLOrigin {
                    x: dst_origin.x as usize,
                    y: dst_origin.y as usize,
                    z: 0,
                },
            );
        }
        f_void(blit, s.end_encoding);
    }
}

/// Timestamps are taken on the CPU when the command is encoded.
fn record_timestamp(query_set: &QuerySet, index: u32) {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    if let Ok(mut ts) = query_set.timestamps_ns.lock() {
        if (index as usize) < ts.len() {
            ts[index as usize] = now_ns;
        }
    }
}

fn resolve_timestamps(
    query_set: &QuerySet,
    first_query: u32,
    query_count: u32,
    destination: &Buffer,
    destination_offset: BufferAddress,
) {
    if let Ok(ts) = query_set.timestamps_ns.lock() {
        let start = first_query as usize;
        let end = (start + query_count as usize).min(ts.len());
        if start < end && !destination.inner.ptr.is_null() {
            let slice = &ts[start..end];
            let bytes = slice.len() * 8;
            // `resolve_query_set` refuses a range that does not fit; a query set shorter
            // than the range only makes the write smaller.
            let fits = usize::try_from(destination_offset)
                .ok()
                .and_then(|offset| offset.checked_add(bytes).map(|end| (offset, end)))
                .filter(|&(_, end)| end as u64 <= destination.inner.size);
            if let Some((offset, _)) = fits {
                // SAFETY: `offset + bytes` is inside the destination allocation, and the
                // timestamps are a separate allocation.
                unsafe {
                    ptr::copy_nonoverlapping(
                        slice.as_ptr() as *const u8,
                        destination.inner.ptr.add(offset),
                        bytes,
                    );
                }
            }
        }
    }
}
