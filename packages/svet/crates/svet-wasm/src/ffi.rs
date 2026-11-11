#[cfg(target_arch = "wasm32")]
use std::cell::{Cell, UnsafeCell};

use crate::core::Core;
use svet_gpu_arena::{MAX_GPU_ARENA_FLOATS, ViewportPickQuery, perspective_matrix};

#[cfg(target_arch = "wasm32")]
struct GlobalCore {
    borrowed: Cell<bool>,
    core: UnsafeCell<Core>,
}

// The shipped WASM module is single-threaded; each WebAssembly.Instance owns its own linear memory.
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for GlobalCore {}

#[cfg(target_arch = "wasm32")]
static CORE: GlobalCore = GlobalCore {
    borrowed: Cell::new(false),
    core: UnsafeCell::new(Core::new()),
};

#[cfg(target_arch = "wasm32")]
struct BorrowGuard;

#[cfg(target_arch = "wasm32")]
impl Drop for BorrowGuard {
    #[inline]
    fn drop(&mut self) {
        CORE.borrowed.set(false);
    }
}

#[cfg(target_arch = "wasm32")]
#[inline]
pub(crate) fn with_core<R>(f: impl FnOnce(&mut Core) -> R) -> R {
    assert!(!CORE.borrowed.replace(true), "reentrant svet-wasm borrow");
    let _guard = BorrowGuard;
    // SAFETY: `CORE.borrowed` guarantees exclusive mutable access for this call frame.
    unsafe { f(&mut *CORE.core.get()) }
}

// Native C-ABI calls are isolated per OS thread via RefCell (preventing both cross-thread races
// and reentrant `&mut` aliasing). Independent windows on one thread use `svet_graph::Context`.
#[cfg(not(target_arch = "wasm32"))]
std::thread_local! {
    static NATIVE_CORE: std::cell::RefCell<Core> = const { std::cell::RefCell::new(Core::new()) };
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub(crate) fn with_core<R>(f: impl FnOnce(&mut Core) -> R) -> R {
    NATIVE_CORE.with(|core| f(&mut core.borrow_mut()))
}

// ============================================================================
// C-ABI Exports for WebAssembly (Zero-Dependency, Ultralight Binary)
// ============================================================================

#[unsafe(no_mangle)]
pub extern "C" fn svet_reset() {
    with_core(|c| c.reset());
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_create_node(kind: u8) -> u32 {
    with_core(|c| c.graph.create_node(kind))
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_dispose_node(id: u32) {
    with_core(|c| c.graph.dispose_node(id));
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_begin_tracking(target: u32) {
    with_core(|c| c.graph.begin_tracking(target));
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_end_tracking() {
    with_core(|c| c.graph.end_tracking());
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_pause_tracking() {
    with_core(|c| c.graph.pause_tracking());
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_resume_tracking() {
    with_core(|c| c.graph.resume_tracking());
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_track_read(source: u32) {
    with_core(|c| c.graph.track_read(source));
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_trigger_write(source: u32) {
    with_core(|c| c.graph.trigger_write(source));
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_trigger_and_flush(source: u32) -> i32 {
    with_core(|c| c.graph.trigger_and_flush_fast(source))
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_get_state(id: u32) -> u8 {
    with_core(|c| c.graph.get_state(id))
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_set_state(id: u32, state: u8) {
    with_core(|c| c.graph.set_state(id, state));
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_batch_start() {
    with_core(|c| {
        c.graph.batch_depth += 1;
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_batch_end() -> u32 {
    with_core(|c| {
        if c.graph.batch_depth > 0 {
            c.graph.batch_depth -= 1;
        }
        c.graph.batch_depth
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_prepare_flush() -> usize {
    with_core(|c| c.graph.prepare_flush())
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_get_flush_ptr() -> *const u32 {
    with_core(|c| c.graph.flush_buffer.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_get_states_ptr() -> *const u8 {
    with_core(|c| c.graph.node_states.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_get_node_count() -> u32 {
    with_core(|c| c.graph.active_count)
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_get_capacity() -> u32 {
    with_core(|c| c.graph.nodes.len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_get_edge_count() -> u32 {
    with_core(|c| c.graph.edge_count() as u32)
}

/// Allocates a zero-initialized `Box<[u32]>` of exact length `len` so `svet_free_u32`
/// reconstructs the exact allocator layout without `Vec` excess-capacity mismatch.
#[unsafe(no_mangle)]
pub extern "C" fn svet_alloc_u32(len: usize) -> *mut u32 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    let boxed: Box<[u32]> = vec![0u32; len].into_boxed_slice();
    Box::into_raw(boxed).cast::<u32>()
}

/// # Safety
/// `ptr` must be null or have been allocated by `svet_alloc_u32(len)` with the exact same `len`
/// and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn svet_free_u32(ptr: *mut u32, len: usize) {
    if !ptr.is_null() && len > 0 {
        // SAFETY: Upheld by caller; `svet_alloc_u32` produces a `Box<[u32]>` of exact length `len`.
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
        }
    }
}

/// # Safety
/// When `len > 0` and `!ptr.is_null()`, `ptr` must point to `len` initialized `u32` values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn svet_compute_lis(ptr: *const u32, len: usize) -> usize {
    if ptr.is_null() || len == 0 {
        return 0;
    }
    // SAFETY: Caller guarantees `ptr..ptr + len` is valid for reads during this call.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    with_core(|c| c.lis.compute_into(slice, &mut c.scratch_out))
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_get_lis_ptr() -> *const u32 {
    with_core(|c| c.scratch_out.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_prepare_lis_input(len: usize) -> *mut u32 {
    with_core(|c| {
        if c.scratch_in.len() < len {
            c.scratch_in.resize(len, 0);
        }
        c.scratch_in.as_mut_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_compute_lis_buffered(len: usize) -> usize {
    with_core(|c| {
        if len == 0 || len > c.scratch_in.len() {
            return 0;
        }
        c.lis.compute_into(&c.scratch_in[..len], &mut c.scratch_out)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_sync_sources_buffered(target: u32, len: usize) {
    with_core(|c| {
        if len > c.scratch_in.len() {
            return;
        }
        c.graph.sync_sources(target, &c.scratch_in[..len]);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_link_1(target: u32, s0: u32) {
    with_core(|c| {
        c.graph.sync_sources(target, &[s0]);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_link_2(target: u32, s0: u32, s1: u32) {
    with_core(|c| {
        c.graph.sync_sources(target, &[s0, s1]);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_is_tracking() -> u32 {
    with_core(|c| u32::from(c.graph.is_tracking_active()))
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_get_arena_limit_f32() -> u32 {
    MAX_GPU_ARENA_FLOATS
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_alloc_f32(len: u32) -> u32 {
    with_core(|c| c.gpu.alloc_f32(len))
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_free_f32(offset: u32, len: u32) {
    with_core(|c| c.gpu.free_f32(offset, len));
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_get_arena_ptr() -> *mut f32 {
    with_core(|c| c.gpu.f32_arena.as_mut_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_get_arena_len() -> u32 {
    with_core(|c| c.gpu.f32_arena.len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_mark_dirty(offset: u32, len: u32) {
    with_core(|c| c.gpu.mark_dirty(offset, len));
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_get_dirty_min() -> u32 {
    with_core(|c| c.gpu.dirty_min)
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_get_dirty_max() -> u32 {
    with_core(|c| c.gpu.dirty_max)
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_clear_dirty() {
    with_core(|c| c.gpu.clear_dirty());
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_collect_dirty_spans() -> usize {
    with_core(|c| c.gpu.collect_dirty_spans_into(&mut c.scratch_out))
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_vector_fma(
    src_offset: u32,
    dst_offset: u32,
    len: u32,
    scale: f32,
    bias: f32,
) {
    with_core(|c| c.gpu.vector_fma(src_offset, dst_offset, len, scale, bias));
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_mat4_mul(a_offset: u32, b_offset: u32, dst_offset: u32) {
    with_core(|c| c.gpu.mat4_mul(a_offset, b_offset, dst_offset));
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_depends_on(target: u32, source: u32) -> u32 {
    with_core(|c| u32::from(c.graph.depends_on(target, source)))
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_pending_effect_entry_count() -> u32 {
    with_core(|c| c.graph.pending_effects.len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_pending_effect_count() -> u32 {
    with_core(|c| c.graph.pending_effect_count() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_valid_perspective(
    fov_y_rad: f32,
    aspect: f32,
    near: f32,
    far: f32,
) -> u32 {
    u32::from(perspective_matrix(fov_y_rad, aspect, near, far).is_some())
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_mat4_perspective(
    dst_offset: u32,
    fov_y_rad: f32,
    aspect: f32,
    near: f32,
    far: f32,
) {
    with_core(|c| {
        c.gpu
            .mat4_perspective(dst_offset, fov_y_rad, aspect, near, far);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_mat4_compose_trs(
    dst_offset: u32,
    tx: f32,
    ty: f32,
    tz: f32,
    rx: f32,
    ry: f32,
    rz: f32,
    sx: f32,
    sy: f32,
    sz: f32,
) {
    with_core(|c| {
        c.gpu
            .mat4_compose_trs(dst_offset, [tx, ty, tz, rx, ry, rz, sx, sy, sz]);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_transform_positions_vec4(
    mat_offset: u32,
    verts_offset: u32,
    dst_offset: u32,
    vertex_count: u32,
) {
    with_core(|c| {
        c.gpu
            .transform_positions_vec4(mat_offset, verts_offset, dst_offset, vertex_count);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_pick_nearest_vertex_3d(
    clip_verts_offset: u32,
    vertex_count: u32,
    viewport_w: f32,
    viewport_h: f32,
    click_x: f32,
    click_y: f32,
    max_radius_px: f32,
) -> i32 {
    with_core(|c| {
        c.gpu.pick_nearest_vertex_3d(
            clip_verts_offset,
            vertex_count,
            ViewportPickQuery::new(viewport_w, viewport_h, click_x, click_y, max_radius_px),
        )
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_prepare_gpu_pick_vertices(len: usize) -> *mut f32 {
    if len > MAX_GPU_ARENA_FLOATS as usize {
        return std::ptr::null_mut();
    }
    with_core(|c| {
        c.gpu.pick_vertices.resize(len, 0.0);
        c.gpu.pick_vertices.as_mut_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_pick_snapshot_3d(
    count: u32,
    indices: u32,
    occlude: u32,
    vw: f32,
    vh: f32,
    x: f32,
    y: f32,
    radius: f32,
) -> i32 {
    with_core(|c| {
        c.gpu.pick_snapshot_3d(
            count,
            indices,
            occlude,
            ViewportPickQuery::new(vw, vh, x, y, radius),
        )
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_prepare_gpu_pick_indices(len: usize) -> *mut u32 {
    with_core(|c| {
        c.gpu.pick_indices.resize(len, 0);
        c.gpu.pick_indices.as_mut_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_pick_visible_vertex_3d(
    offset: u32,
    count: u32,
    indices: u32,
    vw: f32,
    vh: f32,
    x: f32,
    y: f32,
    radius: f32,
) -> i32 {
    with_core(|c| {
        c.gpu.pick_visible_vertex_3d(
            offset,
            count,
            indices,
            ViewportPickQuery::new(vw, vh, x, y, radius),
        )
    })
}

/// Internal scratch result consumed synchronously by Canvas2D transport.
#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_clip_primitive_3d(
    offset: u32,
    count: u32,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    size: u32,
) -> u32 {
    if !(2..=4).contains(&size) {
        return with_core(|core| {
            core.gpu.clip_output.clear();
            0
        });
    }
    with_core(|core| {
        core.gpu
            .clip_primitive_3d(offset, count, &[a, b, c, d][..size as usize])
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn svet_gpu_clip_output_ptr() -> *const f32 {
    with_core(|c| c.gpu.clip_output.as_ptr())
}
