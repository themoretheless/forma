//! Metal compute pass command encoding.

use super::super::*;
use std::ffi::c_void;
use std::ptr;

/// # Safety
///
/// `enc` must be an active, unended `MTLComputeCommandEncoder` pointer for the current command
/// buffer, and every pipeline, buffer, texture, and sampler inside `dispatches` must belong to the
/// encoder's device.
pub(crate) unsafe fn encode_compute_dispatches(enc: *mut c_void, dispatches: &[ComputeDispatchOp]) {
    let s = metal_selectors();
    let f_set_pso: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) = msg_send();
    let f_set_bytes: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, usize, usize) =
        msg_send();
    let f_dispatch: unsafe extern "C" fn(*mut c_void, *mut c_void, MTLSize, MTLSize) = msg_send();

    let mut last_pso: *mut c_void = ptr::null_mut();
    for d in dispatches {
        let Some(pso) = &d.pipeline.pso else {
            continue;
        };
        let pso_ptr = pso.as_ptr();
        if pso_ptr != last_pso {
            // SAFETY: `enc` is an active compute command encoder and `pso_ptr` is a valid
            // `MTLComputePipelineState`.
            unsafe {
                f_set_pso(enc, s.set_compute_pipeline_state, pso_ptr);
            }
            last_pso = pso_ptr;
        }

        // SAFETY: `enc` is an active compute command encoder and `d` owns valid Metal handles.
        let (buffer_sizes, any_buf_sizes) = unsafe { bind_compute_dispatch_resources(enc, s, d) };
        if any_buf_sizes {
            // SAFETY: `buffer_sizes` points to `[u32; 32]` valid for the duration of `setBytes:`.
            unsafe {
                f_set_bytes(
                    enc,
                    s.set_bytes,
                    buffer_sizes.as_ptr() as *const c_void,
                    std::mem::size_of_val(&buffer_sizes),
                    MSL_ARRAY_LENGTHS_BUFFER_SLOT as usize,
                );
            }
        }

        if !d.immediates.is_empty() {
            // SAFETY: `d.immediates` slice is valid for `d.immediates.len()` bytes.
            unsafe {
                f_set_bytes(
                    enc,
                    s.set_bytes,
                    d.immediates.as_ptr() as *const c_void,
                    d.immediates.len(),
                    MSL_IMMEDIATE_BUFFER_SLOT as usize,
                );
            }
        }

        let grid = MTLSize {
            width: d.groups[0].max(1) as usize,
            height: d.groups[1].max(1) as usize,
            depth: d.groups[2].max(1) as usize,
        };
        let tg = MTLSize {
            width: d.pipeline.workgroup_size[0].max(1) as usize,
            height: d.pipeline.workgroup_size[1].max(1) as usize,
            depth: d.pipeline.workgroup_size[2].max(1) as usize,
        };
        // SAFETY: Pipeline state and resources for `grid` / `tg` are bound on `enc`.
        unsafe {
            f_dispatch(enc, s.dispatch_threadgroups, grid, tg);
        }
    }
}

/// # Safety
///
/// `enc` must be a valid active `MTLComputeCommandEncoder` pointer compatible with the handles in `d`.
unsafe fn bind_compute_dispatch_resources(
    enc: *mut c_void,
    s: &crate::metal::Selectors,
    d: &ComputeDispatchOp,
) -> ([u32; 32], bool) {
    let f_set_buf: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize, usize) =
        msg_send();
    let f_set_tex: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize) = msg_send();
    let f_set_samp: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize) = msg_send();

    let mut buffer_sizes = [0u32; 32];
    let any_buf_sizes = !d.pipeline.bindings.array_length_slots.is_empty();
    let map = &d.pipeline.bindings;

    for (group_idx, bg_opt, dyn_offsets) in &d.bind_groups {
        let Some(bg) = bg_opt else { continue };
        for (b_idx, (binding, buf, base_off, byte_size)) in bg.buffers.iter().enumerate() {
            let slot = msl_buffer_slot(*group_idx, *binding) as usize;
            let dyn_off = dyn_offsets.get(b_idx).unwrap_or(0) as u64;
            let final_off = (*base_off + dyn_off) as usize;
            // SAFETY: `buf.inner.raw` is a live `MTLBuffer` referenced for the duration of the batch.
            unsafe {
                f_set_buf(enc, s.set_buffer, buf.inner.raw.as_ptr(), final_off, slot);
            }
            if slot < 32 {
                let rem = (*byte_size).saturating_sub(dyn_off);
                buffer_sizes[slot] = rem as u32;
            }
        }
        for (binding, tex) in bg.textures.iter() {
            if let Some(&slot) = map.textures.get(&(*group_idx, *binding)) {
                // SAFETY: `tex.raw` is a live `MTLTexture` retained by `bg`.
                unsafe {
                    f_set_tex(enc, s.set_texture, tex.raw.as_ptr(), slot as usize);
                }
            }
        }
        for (binding, samp) in bg.samplers.iter() {
            if let Some(&slot) = map.samplers.get(&(*group_idx, *binding)) {
                // SAFETY: `samp.raw` is a live `MTLSamplerState` retained by `bg`.
                unsafe {
                    f_set_samp(enc, s.set_sampler_state, samp.raw.as_ptr(), slot as usize);
                }
            }
        }
    }

    (buffer_sizes, any_buf_sizes)
}
