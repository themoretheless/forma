#[cfg(target_arch = "wasm32")]
use std::cell::UnsafeCell;

use crate::gpu_math::{MAX_GPU_ARENA_FLOATS, perspective_matrix};
use crate::graph::{KIND_COMPUTED, KIND_EFFECT, ReactiveGraph};

#[cfg(target_arch = "wasm32")]
struct GlobalGraph(UnsafeCell<ReactiveGraph>);
// The shipped WASM module is single-threaded; each instance owns its memory.
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for GlobalGraph {}

#[cfg(target_arch = "wasm32")]
static GRAPH: GlobalGraph = GlobalGraph(UnsafeCell::new(ReactiveGraph::new()));

#[cfg(target_arch = "wasm32")]
#[inline]
fn with_graph<R>(f: impl FnOnce(&mut ReactiveGraph) -> R) -> R {
    unsafe { f(&mut *GRAPH.0.get()) }
}

// Native ABI contexts follow the calling thread, never an unsynchronized
// process-wide graph. Independent windows on one thread should own ReactiveGraph
// values directly. Returned native buffer pointers belong to this thread and
// remain valid only until a subsequent mutation of that context.
#[cfg(not(target_arch = "wasm32"))]
std::thread_local! {
    static NATIVE_GRAPH: std::cell::RefCell<ReactiveGraph> =
        const { std::cell::RefCell::new(ReactiveGraph::new()) };
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn with_graph<R>(f: impl FnOnce(&mut ReactiveGraph) -> R) -> R {
    NATIVE_GRAPH.with(|graph| f(&mut graph.borrow_mut()))
}

// ============================================================================
// C-ABI Exports for WebAssembly (Zero-Dependency, Ultralight Binary)
// ============================================================================

#[no_mangle]
pub extern "C" fn svet_reset() {
    with_graph(|g| g.reset());
}

#[no_mangle]
pub extern "C" fn svet_create_node(kind: u8) -> u32 {
    with_graph(|g| g.create_node(kind))
}

#[no_mangle]
pub extern "C" fn svet_dispose_node(id: u32) {
    with_graph(|g| g.dispose_node(id));
}

#[no_mangle]
pub extern "C" fn svet_begin_tracking(target: u32) {
    with_graph(|g| g.begin_tracking(target));
}

#[no_mangle]
pub extern "C" fn svet_end_tracking() {
    with_graph(|g| g.end_tracking());
}

#[no_mangle]
pub extern "C" fn svet_pause_tracking() {
    with_graph(|g| g.pause_tracking());
}

#[no_mangle]
pub extern "C" fn svet_resume_tracking() {
    with_graph(|g| g.resume_tracking());
}

#[no_mangle]
pub extern "C" fn svet_track_read(source: u32) {
    with_graph(|g| g.track_read(source));
}

#[no_mangle]
pub extern "C" fn svet_trigger_write(source: u32) {
    with_graph(|g| g.trigger_write(source));
}

#[no_mangle]
pub extern "C" fn svet_trigger_and_flush(source: u32) -> i32 {
    with_graph(|g| g.trigger_and_flush_fast(source))
}

#[no_mangle]
pub extern "C" fn svet_get_state(id: u32) -> u8 {
    with_graph(|g| g.get_state(id))
}

#[no_mangle]
pub extern "C" fn svet_set_state(id: u32, state: u8) {
    with_graph(|g| g.set_state(id, state));
}

#[no_mangle]
pub extern "C" fn svet_batch_start() {
    with_graph(|g| {
        g.batch_depth += 1;
    });
}

#[no_mangle]
pub extern "C" fn svet_batch_end() -> u32 {
    with_graph(|g| {
        if g.batch_depth > 0 {
            g.batch_depth -= 1;
        }
        g.batch_depth
    })
}

#[no_mangle]
pub extern "C" fn svet_prepare_flush() -> usize {
    with_graph(|g| g.prepare_flush())
}

#[no_mangle]
pub extern "C" fn svet_get_flush_ptr() -> *const u32 {
    with_graph(|g| g.flush_buffer.as_ptr())
}

#[no_mangle]
pub extern "C" fn svet_get_states_ptr() -> *const u8 {
    with_graph(|g| g.node_states.as_ptr())
}

#[no_mangle]
pub extern "C" fn svet_get_node_count() -> u32 {
    with_graph(|g| g.active_count)
}

#[no_mangle]
pub extern "C" fn svet_get_capacity() -> u32 {
    with_graph(|g| g.nodes.len() as u32)
}

#[no_mangle]
pub extern "C" fn svet_get_edge_count() -> u32 {
    with_graph(|g| g.subscribers.iter().map(|s| s.len() as u32).sum())
}

#[no_mangle]
pub extern "C" fn svet_alloc_u32(len: usize) -> *mut u32 {
    let mut vec = Vec::<u32>::with_capacity(len);
    let ptr = vec.as_mut_ptr();
    std::mem::forget(vec);
    ptr
}

#[no_mangle]
pub extern "C" fn svet_free_u32(ptr: *mut u32, len: usize) {
    if !ptr.is_null() && len > 0 {
        unsafe {
            let _ = Vec::from_raw_parts(ptr, 0, len);
        }
    }
}

#[no_mangle]
pub extern "C" fn svet_compute_lis(ptr: *const u32, len: usize) -> usize {
    if ptr.is_null() || len == 0 {
        return 0;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    with_graph(|g| g.compute_lis(slice))
}

#[no_mangle]
pub extern "C" fn svet_get_lis_ptr() -> *const u32 {
    with_graph(|g| g.lis_buffer.as_ptr())
}

#[no_mangle]
pub extern "C" fn svet_prepare_lis_input(len: usize) -> *mut u32 {
    with_graph(|g| {
        if g.lis_input_buffer.len() < len {
            g.lis_input_buffer.resize(len, 0);
        }
        g.lis_input_buffer.as_mut_ptr()
    })
}

#[no_mangle]
pub extern "C" fn svet_compute_lis_buffered(len: usize) -> usize {
    with_graph(|g| {
        if len == 0 || len > g.lis_input_buffer.len() {
            return 0;
        }
        let ptr = g.lis_input_buffer.as_ptr();
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        g.compute_lis(slice)
    })
}

#[no_mangle]
pub extern "C" fn svet_sync_sources_buffered(target: u32, len: usize) {
    with_graph(|g| {
        if len > g.lis_input_buffer.len() {
            return;
        }
        let ptr = g.lis_input_buffer.as_ptr();
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        g.sync_sources(target, slice);
    });
}

#[no_mangle]
pub extern "C" fn svet_link_1(target: u32, s0: u32) {
    with_graph(|g| {
        g.sync_sources(target, &[s0]);
    });
}

#[no_mangle]
pub extern "C" fn svet_link_2(target: u32, s0: u32, s1: u32) {
    with_graph(|g| {
        g.sync_sources(target, &[s0, s1]);
    });
}

#[no_mangle]
pub extern "C" fn svet_is_tracking() -> u32 {
    with_graph(|g| {
        g.tracking_stack
            .last()
            .is_some_and(|&(target, _, lifetime)| {
                g.nodes.get(target as usize).is_some_and(|node| {
                    node.alive
                        && node.lifetime == lifetime
                        && (node.kind == KIND_COMPUTED || node.kind == KIND_EFFECT)
                })
            }) as u32
    })
}

#[no_mangle]
pub extern "C" fn svet_gpu_get_arena_limit_f32() -> u32 {
    MAX_GPU_ARENA_FLOATS
}

#[no_mangle]
pub extern "C" fn svet_gpu_alloc_f32(len: u32) -> u32 {
    with_graph(|g| g.gpu_alloc_f32(len))
}

#[no_mangle]
pub extern "C" fn svet_gpu_free_f32(offset: u32, len: u32) {
    with_graph(|g| g.gpu_free_f32(offset, len));
}

#[no_mangle]
pub extern "C" fn svet_gpu_get_arena_ptr() -> *mut f32 {
    with_graph(|g| g.gpu_f32_arena.as_mut_ptr())
}

#[no_mangle]
pub extern "C" fn svet_gpu_get_arena_len() -> u32 {
    with_graph(|g| g.gpu_f32_arena.len() as u32)
}

#[no_mangle]
pub extern "C" fn svet_gpu_mark_dirty(offset: u32, len: u32) {
    with_graph(|g| g.gpu_mark_dirty(offset, len));
}

#[no_mangle]
pub extern "C" fn svet_gpu_get_dirty_min() -> u32 {
    with_graph(|g| g.gpu_dirty_min)
}

#[no_mangle]
pub extern "C" fn svet_gpu_get_dirty_max() -> u32 {
    with_graph(|g| g.gpu_dirty_max)
}

#[no_mangle]
pub extern "C" fn svet_gpu_clear_dirty() {
    with_graph(|g| g.gpu_clear_dirty());
}

#[no_mangle]
pub extern "C" fn svet_gpu_collect_dirty_spans() -> usize {
    with_graph(|g| g.gpu_collect_dirty_spans())
}

#[no_mangle]
pub extern "C" fn svet_gpu_vector_fma(
    src_offset: u32,
    dst_offset: u32,
    len: u32,
    scale: f32,
    bias: f32,
) {
    with_graph(|g| g.gpu_vector_fma(src_offset, dst_offset, len, scale, bias));
}

#[no_mangle]
pub extern "C" fn svet_gpu_mat4_mul(a_offset: u32, b_offset: u32, dst_offset: u32) {
    with_graph(|g| g.gpu_mat4_mul(a_offset, b_offset, dst_offset));
}

#[no_mangle]
pub extern "C" fn svet_depends_on(target: u32, source: u32) -> u32 {
    with_graph(|g| g.depends_on(target, source) as u32)
}

#[no_mangle]
pub extern "C" fn svet_pending_effect_entry_count() -> u32 {
    with_graph(|g| g.pending_effects.len() as u32)
}

#[no_mangle]
pub extern "C" fn svet_pending_effect_count() -> u32 {
    with_graph(|g| g.pending_effect_count() as u32)
}

#[no_mangle]
pub extern "C" fn svet_gpu_valid_perspective(
    fov_y_rad: f32,
    aspect: f32,
    near: f32,
    far: f32,
) -> u32 {
    perspective_matrix(fov_y_rad, aspect, near, far).is_some() as u32
}

#[no_mangle]
pub extern "C" fn svet_gpu_mat4_perspective(
    dst_offset: u32,
    fov_y_rad: f32,
    aspect: f32,
    near: f32,
    far: f32,
) {
    with_graph(|g| g.gpu_mat4_perspective(dst_offset, fov_y_rad, aspect, near, far));
}

#[no_mangle]
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
    with_graph(|g| g.gpu_mat4_compose_trs(dst_offset, tx, ty, tz, rx, ry, rz, sx, sy, sz));
}

#[no_mangle]
pub extern "C" fn svet_gpu_transform_positions_vec4(
    mat_offset: u32,
    verts_offset: u32,
    dst_offset: u32,
    vertex_count: u32,
) {
    with_graph(|g| {
        g.gpu_transform_positions_vec4(mat_offset, verts_offset, dst_offset, vertex_count)
    });
}

#[no_mangle]
pub extern "C" fn svet_gpu_pick_nearest_vertex_3d(
    clip_verts_offset: u32,
    vertex_count: u32,
    viewport_w: f32,
    viewport_h: f32,
    click_x: f32,
    click_y: f32,
    max_radius_px: f32,
) -> i32 {
    with_graph(|g| {
        g.gpu_pick_nearest_vertex_3d(
            clip_verts_offset,
            vertex_count,
            viewport_w,
            viewport_h,
            click_x,
            click_y,
            max_radius_px,
        )
    })
}

#[no_mangle]
pub extern "C" fn svet_prepare_gpu_pick_vertices(len: usize) -> *mut f32 {
    if len > 67_108_864 {
        return std::ptr::null_mut();
    }
    with_graph(|g| {
        g.gpu_pick_vertices.resize(len, 0.0);
        g.gpu_pick_vertices.as_mut_ptr()
    })
}
#[no_mangle]
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
    with_graph(|g| g.gpu_pick_snapshot_3d(count, indices, occlude, vw, vh, x, y, radius))
}

#[no_mangle]
pub extern "C" fn svet_prepare_gpu_pick_indices(len: usize) -> *mut u32 {
    with_graph(|g| {
        g.gpu_pick_indices.resize(len, 0);
        g.gpu_pick_indices.as_mut_ptr()
    })
}
#[no_mangle]
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
    with_graph(|g| g.gpu_pick_visible_vertex_3d(offset, count, indices, vw, vh, x, y, radius))
}

/// Internal scratch result consumed synchronously by Canvas2D transport.
#[no_mangle]
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
        return with_graph(|g| {
            g.gpu_clip_output.clear();
            0
        });
    }
    with_graph(|g| g.gpu_clip_primitive_3d(offset, count, &[a, b, c, d][..size as usize]))
}
#[no_mangle]
pub extern "C" fn svet_gpu_clip_output_ptr() -> *const f32 {
    with_graph(|g| g.gpu_clip_output.as_ptr())
}
