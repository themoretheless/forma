//! # Svet Engine — WASM Reactive Core (`svet-wasm`)
//!
//! Solves fundamental flaws of React, Vue, Svelte, and Blazor:
//! 1. **Zero GC Pressure + Slot Recycling (`free_list`)**: Graph edges, node states, and
//!    topological dirty queues live in linear WASM memory. Disposed nodes return their ID
//!    to `free_list` for O(1) memory reuse without unbounded vector growth in long-running SPAs.
//! 2. **Glitch-Free Push-Pull Hybrid DAG**:
//!    - *Push phase*: Marks downstream computed & effect nodes as `DIRTY` / `CHECK` in linear time.
//!    - *Pull phase*: Evaluates computed nodes lazily and executes effects only once per batch,
//!      eliminating diamond dependency glitches.
//! 3. **Zero JS<->WASM Marshaling Bottleneck (unlike Blazor)**:
//!    Instead of single-call interop per node, flushed effect IDs and LIS reconciliation patches
//!    are written directly to linear WASM memory buffers read as `Uint32Array` views by TypeScript.
//! 4. **O(n log n) LIS Keyed List Reconciler**:
//!    Computes the Longest Increasing Subsequence in WASM to produce the mathematically minimal
//!    number of DOM moves for keyed lists.

use std::cell::UnsafeCell;

fn perspective_matrix(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> Option<[f32; 16]> {
    if ![fov_y_rad, aspect, near, far].iter().all(|v| v.is_finite())
        || fov_y_rad <= 0.0 || fov_y_rad >= std::f32::consts::PI
        || aspect <= 0.0 || near <= 0.0 || far <= near {
        return None;
    }
    let f = 1.0 / (0.5 * fov_y_rad).tan();
    let z = far / (near - far);
    // Avoid the overflowing near*far intermediate of near*far/(near-far).
    let m = [f / aspect, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0,
        0.0, 0.0, z, -1.0, 0.0, 0.0, near * z, 0.0];
    if m.iter().all(|v| v.is_finite()) && m[0] > 0.0 && m[5] > 0.0 && m[14] < 0.0 {
        Some(m)
    } else { None }
}

/// Maximum per-context GPU arena storage: 256 MiB of float32 values.
pub const MAX_GPU_ARENA_FLOATS: u32 = 67_108_864;

/// Grow the arena within a caller-supplied element limit (small limits aid tests).
fn grow_gpu_arena(arena: &mut Vec<f32>, new_len: usize, limit: usize) -> bool {
    if new_len > limit || new_len < arena.len() || arena.capacity() > limit { return false; }
    if new_len > arena.capacity() {
        let capacity = arena.capacity().saturating_mul(2).max(new_len).min(limit);
        if arena.try_reserve_exact(capacity - arena.len()).is_err() { return false; }
    }
    arena.resize(new_len, 0.0);
    true
}

pub const KIND_SIGNAL: u8 = 0;
pub const KIND_COMPUTED: u8 = 1;
pub const KIND_EFFECT: u8 = 2;

pub const STATE_CLEAN: u8 = 0;
pub const STATE_CHECK: u8 = 1;
pub const STATE_DIRTY: u8 = 2;
pub const STATE_DISPOSED: u8 = 255;

#[derive(Clone, Copy)]
pub struct Node {
    pub kind: u8,
    pub in_queue: bool,
    pub tracking: bool,
    /// Identity of this slot allocation, independent of reactive write epochs.
    pub lifetime: u32,
    pub alive: bool,
    pub epoch: u32,
}

#[derive(Clone, Copy)]
pub struct ScheduledEffect {
    pub id: u32,
    pub lifetime: u32,
}

pub struct ReactiveGraph {
    pub nodes: Vec<Node>,
    /// Forward edges: `subscribers[source_id]` -> list of target node IDs
    pub subscribers: Vec<Vec<u32>>,
    /// Backward edges: `sources[target_id]` -> list of source node IDs
    pub sources: Vec<Vec<u32>>,
    /// Contiguous byte array of node states (`STATE_CLEAN`, `STATE_CHECK`, `STATE_DIRTY`, `STATE_DISPOSED`)
    /// exposed directly to JS via zero-copy `Uint8Array` view so JS can check `state === STATE_CLEAN` without FFI!
    pub node_states: Vec<u8>,
    /// Cached terminal effect (`u32::MAX` = multiple/dirty, `u32::MAX - 1` = none, `< u32::MAX - 1` = unique downstream effect ID)
    pub single_terminal_effect: Vec<u32>,
    /// Cached downstream computed nodes for `source_id` so `trigger_and_flush_fast` can mark them in a flat slice loop
    pub downstream_computeds: Vec<Vec<u32>>,
    /// Epoch of the topology structure (incremented whenever `link`, `clear_sources`, or `dispose_node` mutates edges)
    pub topology_epoch: u32,
    /// Epoch when `single_terminal_effect[source]` was last validated
    pub terminal_cache_epoch: Vec<u32>,
    /// Recycled node IDs for O(1) slot reuse without memory growth
    pub free_list: Vec<u32>,
    /// Count of currently active (non-disposed) nodes
    pub active_count: u32,
    /// Discovery frames `(observer, cursor, slot_lifetime)` for computed/effect evaluation
    pub tracking_stack: Vec<(u32, usize, u32)>,
    /// Scheduled effect identities; native IDs alone can be reused before flush
    pub pending_effects: Vec<ScheduledEffect>,
    /// Output buffer exposed to JS via zero-copy `Uint32Array` view
    pub flush_buffer: Vec<u32>,
    /// Batch depth counter (allows nested `batch(() => ...)` without intermediate flushes)
    pub batch_depth: u32,
    /// Global monotonic version clock
    pub global_epoch: u32,
    /// Output buffer for LIS (Longest Increasing Subsequence) keyed list diffing
    pub lis_buffer: Vec<u32>,
    /// Reusable input scratch buffer for LIS and batch operations (avoids per-call alloc/free)
    pub lis_input_buffer: Vec<u32>,
    /// Reusable traversal stack for `trigger_write` (avoids per-write Vec allocation)
    pub propagate_stack: Vec<(u32, u8)>,
    dependency_stack: Vec<u32>,
    dependency_seen: Vec<u32>,
    dependency_epoch: u32,
    /// Contiguous 16-byte-aligned f32 page for UMA/DUMA GPU Tensor Signals
    pub gpu_f32_arena: Vec<f32>,
    /// Homogeneous clipping result, valid until the next clipping call.
    pub gpu_clip_output: Vec<f32>,
    pub gpu_pick_indices: Vec<u32>,
    pub gpu_pick_vertices: Vec<f32>,
    /// Dirty bitset (1 bit per 32 floats / 128-byte cache line) for partial DUMA DMA transfers
    pub gpu_dirty_blocks: Vec<u32>,
    /// Free blocks `(offset, aligned_len)` in `gpu_f32_arena` for O(1) GPU tensor memory recycling
    pub gpu_free_blocks: Vec<(u32, u32)>,
    /// Minimum dirty float index in the current frame
    pub gpu_dirty_min: u32,
    /// Maximum dirty float index (exclusive) in the current frame
    pub gpu_dirty_max: u32,
}

impl ReactiveGraph {
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            subscribers: Vec::new(),
            sources: Vec::new(),
            node_states: Vec::new(),
            single_terminal_effect: Vec::new(),
            downstream_computeds: Vec::new(),
            topology_epoch: 1,
            terminal_cache_epoch: Vec::new(),
            free_list: Vec::new(),
            active_count: 0,
            tracking_stack: Vec::new(),
            pending_effects: Vec::new(),
            flush_buffer: Vec::new(),
            batch_depth: 0,
            global_epoch: 1,
            lis_buffer: Vec::new(),
            lis_input_buffer: Vec::new(),
            propagate_stack: Vec::new(),
            dependency_stack: Vec::new(), dependency_seen: Vec::new(), dependency_epoch: 0,
            gpu_f32_arena: Vec::new(),
            gpu_clip_output: Vec::new(),
            gpu_pick_indices: Vec::new(),
            gpu_pick_vertices: Vec::new(),
            gpu_dirty_blocks: Vec::new(),
            gpu_free_blocks: Vec::new(),
            gpu_dirty_min: u32::MAX,
            gpu_dirty_max: 0,
        }
    }

    pub fn reset(&mut self) {
        self.nodes.clear();
        self.subscribers.clear();
        self.sources.clear();
        self.node_states.clear();
        self.single_terminal_effect.clear();
        self.downstream_computeds.clear();
        self.topology_epoch = 1;
        self.terminal_cache_epoch.clear();
        self.free_list.clear();
        self.active_count = 0;
        self.tracking_stack.clear();
        self.pending_effects.clear();
        self.flush_buffer.clear();
        self.batch_depth = 0;
        self.global_epoch = 1;
        self.lis_buffer.clear();
        self.lis_input_buffer.clear();
        self.propagate_stack.clear();
        self.dependency_stack.clear(); self.dependency_seen.clear(); self.dependency_epoch = 0;
        self.gpu_f32_arena.clear();
        self.gpu_clip_output.clear();
        self.gpu_pick_indices.clear();
        self.gpu_pick_vertices.clear();
        self.gpu_dirty_blocks.clear();
        self.gpu_free_blocks.clear();
        self.gpu_dirty_min = u32::MAX;
        self.gpu_dirty_max = 0;
    }

    /// Allocates a 4-float (16-byte WebGPU uniform/storage aligned) contiguous slice in the UMA/DUMA arena.
    /// Reuses freed blocks from `gpu_free_blocks` when available.
    /// Returns the starting float index (`offset`).
    pub fn gpu_alloc_f32(&mut self, len: u32) -> u32 {
        let aligned_len = match len.checked_add(3) {
            Some(v) => v & !3,
            None => return u32::MAX,
        };
        if aligned_len == 0 {
            return self.gpu_f32_arena.len() as u32;
        }
        // 1. Exact or best-fit reuse from `gpu_free_blocks` (with automatic block splitting!)
        let mut best_idx: Option<usize> = None;
        let mut best_len = u32::MAX;
        for (idx, &(_, block_len)) in self.gpu_free_blocks.iter().enumerate() {
            if block_len == aligned_len {
                best_idx = Some(idx);
                break;
            }
            if block_len > aligned_len && block_len < best_len {
                best_len = block_len;
                best_idx = Some(idx);
            }
        }

        if let Some(pos) = best_idx {
            let (reused_offset, block_len) = self.gpu_free_blocks.swap_remove(pos);
            if block_len > aligned_len {
                // Split the remaining tail block and keep it in `gpu_free_blocks`
                self.gpu_free_blocks
                    .push((reused_offset + aligned_len, block_len - aligned_len));
            }
            let start_idx = reused_offset as usize;
            let end_idx = start_idx.saturating_add(aligned_len as usize);
            if end_idx <= self.gpu_f32_arena.len() {
                for v in &mut self.gpu_f32_arena[start_idx..end_idx] {
                    *v = 0.0;
                }
            }
            self.gpu_mark_dirty(reused_offset, len);
            return reused_offset;
        }

        let start = self.gpu_f32_arena.len() as u32;
        let new_len = match (start as usize).checked_add(aligned_len as usize) {
            Some(l) if l <= MAX_GPU_ARENA_FLOATS as usize => l,
            _ => return u32::MAX,
        };
        if !grow_gpu_arena(&mut self.gpu_f32_arena, new_len, MAX_GPU_ARENA_FLOATS as usize) {
            return u32::MAX;
        }

        let required_blocks = ((self.gpu_f32_arena.len() + 31) / 32 + 31) / 32;
        if self.gpu_dirty_blocks.len() < required_blocks {
            self.gpu_dirty_blocks.resize(required_blocks, 0);
        }
        self.gpu_mark_dirty(start, len);
        start
    }

    /// Returns a previously allocated tensor slice `[offset, offset + aligned_len)` to `gpu_free_blocks`
    /// and coalesces (merges) any adjacent left/right free blocks to eliminate arena fragmentation!
    pub fn gpu_free_f32(&mut self, offset: u32, len: u32) {
        let aligned_len = match len.checked_add(3) {
            Some(v) => v & !3,
            None => return,
        };
        let end = match (offset as usize).checked_add(aligned_len as usize) {
            Some(e) => e,
            None => return,
        };
        if aligned_len == 0 || end > self.gpu_f32_arena.len() {
            return;
        }

        // Ignore duplicate double-free overlapping an existing free block
        if self.gpu_free_blocks.iter().any(|&(b_off, b_len)| {
            let b_end = b_off.saturating_add(b_len);
            offset < b_end && (end as u32) > b_off
        }) {
            return;
        }

        let mut merged_start = offset;
        let mut merged_len = aligned_len;

        // Coalesce with left neighbor (`left_off + left_len == merged_start`)
        if let Some(left_pos) = self
            .gpu_free_blocks
            .iter()
            .position(|&(b_off, b_len)| b_off.saturating_add(b_len) == merged_start)
        {
            let (left_off, left_len) = self.gpu_free_blocks.swap_remove(left_pos);
            merged_start = left_off;
            merged_len = merged_len.saturating_add(left_len);
        }

        // Coalesce with right neighbor (`merged_start + merged_len == right_off`)
        if let Some(right_pos) = self
            .gpu_free_blocks
            .iter()
            .position(|&(b_off, _)| merged_start.saturating_add(merged_len) == b_off)
        {
            let (_, right_len) = self.gpu_free_blocks.swap_remove(right_pos);
            merged_len = merged_len.saturating_add(right_len);
        }

        self.gpu_free_blocks.push((merged_start, merged_len));
    }

    /// Marks a range `[offset, offset + len)` of floats in the GPU arena as dirty for DUMA partial DMA sync.
    #[inline]
    pub fn gpu_mark_dirty(&mut self, offset: u32, len: u32) {
        if len == 0 {
            return;
        }
        let end = offset.saturating_add(len).min(self.gpu_f32_arena.len() as u32);
        if offset >= end {
            return;
        }
        if offset < self.gpu_dirty_min {
            self.gpu_dirty_min = offset;
        }
        if end > self.gpu_dirty_max {
            self.gpu_dirty_max = end;
        }
        // Mark 32-float (128-byte cache-line) blocks in bitset
        let start_block = (offset / 32) as usize;
        let end_block = ((end - 1) / 32) as usize;
        for b in start_block..=end_block {
            let word = b / 32;
            let bit = b % 32;
            if let Some(w) = self.gpu_dirty_blocks.get_mut(word) {
                *w |= 1u32 << bit;
            }
        }
    }

    /// Collects contiguous dirty 32-float (128-byte cache-line) spans from `gpu_dirty_blocks`
    /// into `self.lis_buffer` as pairs `[start_float_0, len_float_0, start_float_1, len_float_1, ...]`.
    /// Returns the number of spans (pairs).
    /// This prevents transferring untouched gaps when two distant tensors mutate in DUMA mode!
    pub fn gpu_collect_dirty_spans(&mut self) -> usize {
        self.lis_buffer.clear();
        if self.gpu_dirty_min >= self.gpu_dirty_max || self.gpu_dirty_min == u32::MAX {
            return 0;
        }
        let arena_len = self.gpu_f32_arena.len() as u32;
        let mut in_span = false;
        let mut span_start = 0u32;

        let total_blocks = (arena_len + 31) / 32;
        for b in 0..total_blocks {
            let word = (b / 32) as usize;
            let bit = b % 32;
            let is_dirty = self
                .gpu_dirty_blocks
                .get(word)
                .map(|&w| (w & (1u32 << bit)) != 0)
                .unwrap_or(false);

            if is_dirty && !in_span {
                in_span = true;
                span_start = b * 32;
            } else if !is_dirty && in_span {
                in_span = false;
                let span_end = (b * 32).min(arena_len);
                self.lis_buffer.push(span_start);
                self.lis_buffer.push(span_end - span_start);
            }
        }

        if in_span {
            let span_end = arena_len;
            self.lis_buffer.push(span_start);
            self.lis_buffer.push(span_end - span_start);
        }

        self.lis_buffer.len() / 2
    }

    /// Clears the dirty range tracker after a UMA/DUMA frame flush.
    #[inline]
    pub fn gpu_clear_dirty(&mut self) {
        self.gpu_dirty_min = u32::MAX;
        self.gpu_dirty_max = 0;
        for w in &mut self.gpu_dirty_blocks {
            *w = 0;
        }
    }

    /// High-throughput SIMD128 / CPU kernel in WASM (`dst[i] = src[i] * scale + bias`)
    /// for environments where WebGPU compute shaders are unavailable or when running in UMA shared mode.
    pub fn gpu_vector_fma(
        &mut self,
        src_offset: u32,
        dst_offset: u32,
        len: u32,
        scale: f32,
        bias: f32,
    ) {
        let arena_len = self.gpu_f32_arena.len();
        let s = src_offset as usize;
        let d = dst_offset as usize;
        let l = len as usize;
        if s.checked_add(l).map_or(true, |end| end > arena_len)
            || d.checked_add(l).map_or(true, |end| end > arena_len)
        {
            return;
        }

        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        unsafe {
            use core::arch::wasm32::*;
            let v_scale = f32x4_splat(scale);
            let v_bias = f32x4_splat(bias);
            let base_ptr = self.gpu_f32_arena.as_mut_ptr();
            let src_ptr = base_ptr.add(s);
            let dst_ptr = base_ptr.add(d);

            let chunks = l / 4;
            for c in 0..chunks {
                let offset = c * 4;
                let v_src = v128_load(src_ptr.add(offset) as *const v128);
                let v_res = f32x4_add(f32x4_mul(v_src, v_scale), v_bias);
                v128_store(dst_ptr.add(offset) as *mut v128, v_res);
            }
            for i in (chunks * 4)..l {
                let val = *src_ptr.add(i);
                *dst_ptr.add(i) = val * scale + bias;
            }
        }

        #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
        {
            for i in 0..l {
                let val = self.gpu_f32_arena[s + i];
                self.gpu_f32_arena[d + i] = val * scale + bias;
            }
        }

        self.gpu_mark_dirty(dst_offset, len);
    }

    // ========================================================================
    // 3D Scene Graph Math Kernels in Linear Memory (4x4 Column-Major Matrices)
    // ========================================================================

    /// Multiplies two 4x4 column-major matrices (`dst_16 = a_16 * b_16`) inside `gpu_f32_arena`
    /// using WebAssembly SIMD128 when compiled for `wasm32` + `simd128`,
    /// and marks the 16-float destination slice dirty for UMA/DUMA sync.
    pub fn gpu_mat4_mul(&mut self, a_offset: u32, b_offset: u32, dst_offset: u32) {
        let arena_len = self.gpu_f32_arena.len();
        let a = a_offset as usize;
        let b = b_offset as usize;
        let d = dst_offset as usize;
        if a.checked_add(16).map_or(true, |end| end > arena_len)
            || b.checked_add(16).map_or(true, |end| end > arena_len)
            || d.checked_add(16).map_or(true, |end| end > arena_len)
        {
            return;
        }

        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        unsafe {
            use core::arch::wasm32::*;
            let base_ptr = self.gpu_f32_arena.as_mut_ptr();
            let a_ptr = base_ptr.add(a);
            let b_ptr = base_ptr.add(b);
            let d_ptr = base_ptr.add(d);

            let a_col0 = v128_load(a_ptr as *const v128);
            let a_col1 = v128_load(a_ptr.add(4) as *const v128);
            let a_col2 = v128_load(a_ptr.add(8) as *const v128);
            let a_col3 = v128_load(a_ptr.add(12) as *const v128);

            let mut out_cols = [f32x4_splat(0.0); 4];
            for col in 0..4 {
                let bc = b_ptr.add(col * 4);
                let b0 = f32x4_splat(*bc);
                let b1 = f32x4_splat(*bc.add(1));
                let b2 = f32x4_splat(*bc.add(2));
                let b3 = f32x4_splat(*bc.add(3));

                let sum01 = f32x4_add(f32x4_mul(a_col0, b0), f32x4_mul(a_col1, b1));
                let sum23 = f32x4_add(f32x4_mul(a_col2, b2), f32x4_mul(a_col3, b3));
                out_cols[col] = f32x4_add(sum01, sum23);
            }

            v128_store(d_ptr as *mut v128, out_cols[0]);
            v128_store(d_ptr.add(4) as *mut v128, out_cols[1]);
            v128_store(d_ptr.add(8) as *mut v128, out_cols[2]);
            v128_store(d_ptr.add(12) as *mut v128, out_cols[3]);
        }

        #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
        {
            let mut out = [0.0f32; 16];
            for col in 0..4 {
                let b_col = b + col * 4;
                let b0 = self.gpu_f32_arena[b_col];
                let b1 = self.gpu_f32_arena[b_col + 1];
                let b2 = self.gpu_f32_arena[b_col + 2];
                let b3 = self.gpu_f32_arena[b_col + 3];

                for row in 0..4 {
                    out[col * 4 + row] = self.gpu_f32_arena[a + row] * b0
                        + self.gpu_f32_arena[a + 4 + row] * b1
                        + self.gpu_f32_arena[a + 8 + row] * b2
                        + self.gpu_f32_arena[a + 12 + row] * b3;
                }
            }
            self.gpu_f32_arena[d..d + 16].copy_from_slice(&out);
        }

        self.gpu_mark_dirty(dst_offset, 16);
    }

    /// Writes a 4x4 WebGPU Perspective Projection matrix (`depth [0, 1]`, column-major)
    /// into `dst_offset` inside `gpu_f32_arena`.
    pub fn gpu_mat4_perspective(
        &mut self,
        dst_offset: u32,
        fov_y_rad: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) {
        let d = dst_offset as usize;
        if d.checked_add(16).map_or(true, |end| end > self.gpu_f32_arena.len()) {
            return;
        }
        let Some(m) = perspective_matrix(fov_y_rad, aspect, near, far) else { return; };
        self.gpu_f32_arena[d..d + 16].copy_from_slice(&m);
        self.gpu_mark_dirty(dst_offset, 16);
    }

    /// Writes a 4x4 Translation-Rotation-Scale (Euler XYZ radians) matrix into `dst_offset` inside `gpu_f32_arena`.
    pub fn gpu_mat4_compose_trs(
        &mut self,
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
        let d = dst_offset as usize;
        if d.checked_add(16).map_or(true, |end| end > self.gpu_f32_arena.len()) {
            return;
        }
        let (sx_r, cx_r) = rx.sin_cos();
        let (sy_r, cy_r) = ry.sin_cos();
        let (sz_r, cz_r) = rz.sin_cos();

        let m00 = (cy_r * cz_r) * sx;
        let m01 = (cy_r * sz_r) * sx;
        let m02 = (-sy_r) * sx;

        let m10 = (sx_r * sy_r * cz_r - cx_r * sz_r) * sy;
        let m11 = (sx_r * sy_r * sz_r + cx_r * cz_r) * sy;
        let m12 = (sx_r * cy_r) * sy;

        let m20 = (cx_r * sy_r * cz_r + sx_r * sz_r) * sz;
        let m21 = (cx_r * sy_r * sz_r - sx_r * cz_r) * sz;
        let m22 = (cx_r * cy_r) * sz;

        let m = [
            m00, m01, m02, 0.0,
            m10, m11, m12, 0.0,
            m20, m21, m22, 0.0,
            tx, ty, tz, 1.0,
        ];
        self.gpu_f32_arena[d..d + 16].copy_from_slice(&m);
        self.gpu_mark_dirty(dst_offset, 16);
    }

    /// Transforms an array of `vertex_count` 3D vertices (`[x, y, z, 1.0]` stride 4 floats)
    /// by the 4x4 matrix at `mat_offset` and writes `[x', y', z', w']` to `dst_offset`.
    /// Accelerated with WebAssembly `simd128` 4-lane vector intrinsics when available.
    pub fn gpu_transform_positions_vec4(
        &mut self,
        mat_offset: u32,
        verts_offset: u32,
        dst_offset: u32,
        vertex_count: u32,
    ) {
        let arena_len = self.gpu_f32_arena.len();
        let m = mat_offset as usize;
        let v = verts_offset as usize;
        let d = dst_offset as usize;
        let total_floats = match (vertex_count as usize).checked_mul(4) {
            Some(tf) => tf,
            None => return,
        };

        if m.checked_add(16).map_or(true, |end| end > arena_len)
            || v.checked_add(total_floats).map_or(true, |end| end > arena_len)
            || d.checked_add(total_floats).map_or(true, |end| end > arena_len)
        {
            return;
        }

        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        unsafe {
            use core::arch::wasm32::*;
            let base_ptr = self.gpu_f32_arena.as_mut_ptr();
            let m_ptr = base_ptr.add(m);
            let v_ptr = base_ptr.add(v);
            let d_ptr = base_ptr.add(d);

            let m_col0 = v128_load(m_ptr as *const v128);
            let m_col1 = v128_load(m_ptr.add(4) as *const v128);
            let m_col2 = v128_load(m_ptr.add(8) as *const v128);
            let m_col3 = v128_load(m_ptr.add(12) as *const v128);

            for i in 0..(vertex_count as usize) {
                let vp = v_ptr.add(i * 4);
                let dp = d_ptr.add(i * 4);
                let vx = f32x4_splat(*vp);
                let vy = f32x4_splat(*vp.add(1));
                let vz = f32x4_splat(*vp.add(2));
                let vw = f32x4_splat(*vp.add(3));

                let sum01 = f32x4_add(f32x4_mul(m_col0, vx), f32x4_mul(m_col1, vy));
                let sum23 = f32x4_add(f32x4_mul(m_col2, vz), f32x4_mul(m_col3, vw));
                v128_store(dp as *mut v128, f32x4_add(sum01, sum23));
            }
        }

        #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
        {
            let mat: [f32; 16] = self.gpu_f32_arena[m..m + 16].try_into().unwrap();

            for i in 0..(vertex_count as usize) {
                let vi = v + i * 4;
                let di = d + i * 4;
                let x = self.gpu_f32_arena[vi];
                let y = self.gpu_f32_arena[vi + 1];
                let z = self.gpu_f32_arena[vi + 2];
                let w = self.gpu_f32_arena[vi + 3];

                self.gpu_f32_arena[di]     = mat[0] * x + mat[4] * y + mat[8]  * z + mat[12] * w;
                self.gpu_f32_arena[di + 1] = mat[1] * x + mat[5] * y + mat[9]  * z + mat[13] * w;
                self.gpu_f32_arena[di + 2] = mat[2] * x + mat[6] * y + mat[10] * z + mat[14] * w;
                self.gpu_f32_arena[di + 3] = mat[3] * x + mat[7] * y + mat[11] * z + mat[15] * w;
            }
        }

        self.gpu_mark_dirty(dst_offset, vertex_count.saturating_mul(4));
    }

    /// Clip a segment or convex polygon in homogeneous WebGPU coordinates.
    pub fn gpu_clip_primitive_3d(&mut self, offset: u32, count: u32, indices: &[u32]) -> u32 {
        self.gpu_clip_output.clear();
        if !(2..=4).contains(&indices.len()) || indices.iter().any(|&i| i >= count) { return 0; }
        let end = (count as usize).checked_mul(4).and_then(|total| (offset as usize).checked_add(total));
        if end.map_or(true, |end| end > self.gpu_f32_arena.len()) { return 0; }
        let mut vertices: Vec<[f64; 4]> = Vec::with_capacity(10);
        for &index in indices {
            let base = offset as usize + index as usize * 4;
            let v = std::array::from_fn(|axis| self.gpu_f32_arena[base + axis] as f64);
            if v.iter().any(|value| !value.is_finite()) { return 0; }
            vertices.push(v);
        }
        let distance = |v: [f64; 4], plane: usize| match plane {
            0 => v[3] + v[0], 1 => v[3] - v[0],
            2 => v[3] + v[1], 3 => v[3] - v[1],
            4 => v[2], _ => v[3] - v[2],
        };
        let interpolate = |a: [f64; 4], b: [f64; 4], t: f64| {
            std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t)
        };
        if indices.len() == 2 {
            let a = vertices[0]; let b = vertices[1];
            let mut enter: f64 = 0.0; let mut exit: f64 = 1.0;
            for plane in 0..6 {
                let da = distance(a, plane); let db = distance(b, plane);
                if da < 0.0 && db < 0.0 { return 0; }
                if da < 0.0 { enter = enter.max(da / (da - db)); }
                if db < 0.0 { exit = exit.min(da / (da - db)); }
            }
            if enter > exit { return 0; }
            vertices = vec![interpolate(a, b, enter), interpolate(a, b, exit)];
        } else {
            for plane in 0..6 {
                if vertices.is_empty() { break; }
                let mut output = Vec::with_capacity(vertices.len() + 1);
                let mut previous = *vertices.last().unwrap();
                let mut dp = distance(previous, plane);
                for &current in &vertices {
                    let dc = distance(current, plane);
                    if (dp >= 0.0) != (dc >= 0.0) {
                        output.push(interpolate(previous, current, dp / (dp - dc)));
                    }
                    if dc >= 0.0 { output.push(current); }
                    previous = current; dp = dc;
                }
                vertices = output;
            }
        }
        if vertices.iter().any(|v| v[3] <= 0.0) { return 0; }
        for vertex in vertices {
            self.gpu_clip_output.extend(vertex.map(|value| value as f32));
        }
        (self.gpu_clip_output.len() / 4) as u32
    }

    /// Z-Aware 3D Vertex Picking inside the WASM UMA/DUMA arena (`gpu_pick_nearest_vertex_3d`).
    /// Projects `vertex_count` clip-space vec4 vertices (`[x, y, z, w]` at `clip_verts_offset`)
    /// onto a `(viewport_w, viewport_h)` canvas and finds the closest vertex to `(click_x, click_y)`
    /// within `max_radius_px`, prioritizing foreground vertices (`ndc_z = z / w`, smaller is closer
    /// to camera in WebGPU `[0, 1]` depth) when two vertices overlap in 2D screen space!
    /// Returns the picked vertex index (`0..vertex_count - 1`), or `-1` if none is within radius.
    pub fn gpu_pick_nearest_vertex_3d(
        &self,
        clip_verts_offset: u32,
        vertex_count: u32,
        viewport_w: f32,
        viewport_h: f32,
        click_x: f32,
        click_y: f32,
        max_radius_px: f32,
    ) -> i32 {
        self.gpu_pick_vertex_impl(clip_verts_offset, vertex_count, viewport_w, viewport_h, click_x, click_y, max_radius_px, &[])
    }

    pub fn gpu_pick_snapshot_3d(&mut self, count: u32, indices: u32, occlude: u32, vw: f32, vh: f32, x: f32, y: f32, radius: f32) -> i32 {
        if (count as usize).checked_mul(4).map_or(true, |len| len > self.gpu_pick_vertices.len()) { return -1; }
        // Scratch geometry is isolated from the live tensor arena and graph nodes.
        let mut scratch = ReactiveGraph::new();
        scratch.gpu_f32_arena = std::mem::take(&mut self.gpu_pick_vertices);
        scratch.gpu_pick_indices = std::mem::take(&mut self.gpu_pick_indices);
        let result = if occlude != 0 && indices != 0 {
            scratch.gpu_pick_visible_vertex_3d(0, count, indices, vw, vh, x, y, radius)
        } else { scratch.gpu_pick_nearest_vertex_3d(0, count, vw, vh, x, y, radius) };
        self.gpu_pick_vertices = std::mem::take(&mut scratch.gpu_f32_arena);
        self.gpu_pick_indices = std::mem::take(&mut scratch.gpu_pick_indices);
        result
    }

    pub fn gpu_pick_visible_vertex_3d(&mut self, offset: u32, count: u32, index_count: u32, vw: f32, vh: f32, x: f32, y: f32, radius: f32) -> i32 {
        let len = index_count as usize;
        if len > self.gpu_pick_indices.len() || len % 3 != 0 { return -1; }
        let mut triangles = Vec::new();
        for base in (0..len).step_by(3) {
            let indices = [self.gpu_pick_indices[base], self.gpu_pick_indices[base + 1], self.gpu_pick_indices[base + 2]];
            let size = self.gpu_clip_primitive_3d(offset, count, &indices) as usize;
            if size < 3 { continue; }
            let points: Vec<[f64; 3]> = self.gpu_clip_output.chunks_exact(4).map(|v| [v[0] as f64 / v[3] as f64, v[1] as f64 / v[3] as f64, v[2] as f64 / v[3] as f64]).collect();
            for i in 1..size - 1 { triangles.push([points[0], points[i], points[i + 1]]); }
        }
        self.gpu_pick_vertex_impl(offset, count, vw, vh, x, y, radius, &triangles)
    }

    fn gpu_pick_vertex_impl(&self, clip_verts_offset: u32, vertex_count: u32, viewport_w: f32, viewport_h: f32, click_x: f32, click_y: f32, max_radius_px: f32, triangles: &[[[f64; 3]; 3]]) -> i32 {
        if !viewport_w.is_finite() || !viewport_h.is_finite() || viewport_w <= 0.0 || viewport_h <= 0.0
            || !click_x.is_finite() || !click_y.is_finite()
            || !max_radius_px.is_finite() || max_radius_px < 0.0 {
            return -1;
        }
        let base = clip_verts_offset as usize;
        let total = match (vertex_count as usize).checked_mul(4) {
            Some(t) => t,
            None => return -1,
        };
        if base.checked_add(total).map_or(true, |end| end > self.gpu_f32_arena.len()) {
            return -1;
        }

        let half_w = viewport_w * 0.5;
        let half_h = viewport_h * 0.5;
        let scale_x = half_w;
        let scale_y = half_h;
        let max_dist_sq = max_radius_px * max_radius_px;

        let mut best_idx: i32 = -1;
        let mut best_score: f32 = f32::INFINITY;
        let mut best_ndc_z: f32 = f32::INFINITY;

        for i in 0..(vertex_count as usize) {
            let off = base + i * 4;
            let x = self.gpu_f32_arena[off];
            let y = self.gpu_f32_arena[off + 1];
            let z = self.gpu_f32_arena[off + 2];
            let w = self.gpu_f32_arena[off + 3];

            if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite()
                || w <= 0.0 || x < -w || x > w || y < -w || y > w || z < 0.0 || z > w {
                continue;
            }
            let sx = half_w + (x / w) * scale_x;
            let sy = half_h - (y / w) * scale_y;
            let ndc_z = z / w;

            let dx = sx - click_x;
            let dy = sy - click_y;
            let dist_sq = dx * dx + dy * dy;

            if dist_sq <= max_dist_sq {
                let px = x as f64 / w as f64; let py = y as f64 / w as f64;
                let hidden = triangles.iter().any(|[a,b,c]| {
                    let denominator = (b[1]-c[1])*(a[0]-c[0]) + (c[0]-b[0])*(a[1]-c[1]);
                    if denominator.abs() < 1e-14 { return false; }
                    let u = ((b[1]-c[1])*(px-c[0]) + (c[0]-b[0])*(py-c[1])) / denominator;
                    let v = ((c[1]-a[1])*(px-c[0]) + (a[0]-c[0])*(py-c[1])) / denominator;
                    let t = 1.0-u-v;
                    u >= -1e-9 && v >= -1e-9 && t >= -1e-9 && u*a[2]+v*b[2]+t*c[2]+1e-6 < z as f64 / w as f64
                });
                if hidden { continue; }
                // Depth-aware composite score: when two vertices project near the same 2D pixel
                // (within ~4px tolerance), the foreground vertex (smaller `ndc_z`) always wins!
                let depth_bias = ndc_z * 16.0;
                let score = dist_sq + depth_bias;
                if score < best_score || ((score - best_score).abs() < 1e-3 && ndc_z < best_ndc_z) {
                    best_score = score;
                    best_ndc_z = ndc_z;
                    best_idx = i as i32;
                }
            }
        }

        best_idx
    }

    #[inline]
    pub fn create_node(&mut self, kind: u8) -> u32 {
        let initial_state = if kind == KIND_SIGNAL {
            STATE_CLEAN
        } else {
            STATE_DIRTY
        };
        self.active_count += 1;

        if let Some(reused_id) = self.free_list.pop() {
            let idx = reused_id as usize;
            let mut lifetime = self.nodes[idx].lifetime.wrapping_add(1);
            if lifetime == 0 {
                // Avoid aliasing any ancient token when a slot's counter wraps.
                self.pending_effects.retain(|pending| pending.id != reused_id);
                for frame in &mut self.tracking_stack {
                    if frame.0 == reused_id { *frame = (u32::MAX,0,0); }
                }
                lifetime = 1;
            }
            self.nodes[idx] = Node {
                lifetime,
                kind,
                in_queue: false, tracking: false,
                alive: true,
                epoch: self.global_epoch,
            };
            self.node_states[idx] = initial_state;
            self.single_terminal_effect[idx] = u32::MAX;
            self.downstream_computeds[idx].clear();
            self.terminal_cache_epoch[idx] = 0;
            self.subscribers[idx].clear();
            self.sources[idx].clear();
            return reused_id;
        }

        let id = self.nodes.len() as u32;
        self.nodes.push(Node {
            lifetime: 1,
            kind,
            in_queue: false, tracking: false,
            alive: true,
            epoch: self.global_epoch,
        });
        self.node_states.push(initial_state);
        self.single_terminal_effect.push(u32::MAX);
        self.downstream_computeds.push(Vec::new());
        self.terminal_cache_epoch.push(0);
        self.subscribers.push(Vec::with_capacity(4));
        self.sources.push(Vec::with_capacity(4));
        id
    }

    #[inline]
    fn advance_topology_epoch(&mut self) {
        self.topology_epoch = self.topology_epoch.wrapping_add(1);
        if self.topology_epoch == 0 { self.clear_wrapped_topology_caches(); }
    }

    #[cold]
    fn clear_wrapped_topology_caches(&mut self) {
        // Zero remains an invalid cache stamp; no stamp from an older cycle may match.
        self.topology_epoch = 1;
        self.terminal_cache_epoch.fill(0);
        self.single_terminal_effect.fill(u32::MAX);
        for cache in &mut self.downstream_computeds { cache.clear(); }
    }

    /// Disposes a node completely, severing both incoming and outgoing edges
    /// and returning its ID to `free_list` for O(1) slot recycling.
    #[inline]
    pub fn dispose_node(&mut self, id: u32) {
        let idx = id as usize;
        if idx >= self.nodes.len() || !self.nodes[idx].alive {
            return;
        }
        self.advance_topology_epoch();

        // 1. Sever incoming edges (sources -> this node)
        self.clear_sources(id);

        // 2. Sever outgoing edges (this node -> subscribers)
        let subs = std::mem::take(&mut self.subscribers[idx]);
        for sub_id in subs {
            let lifetime = self.nodes.get(sub_id as usize).filter(|node| node.alive && node.tracking).map(|node| node.lifetime);
            if let Some(sub_sources) = self.sources.get_mut(sub_id as usize) {
                if let Some(pos) = sub_sources.iter().position(|&x| x == id) {
                    let mut tracked = false;
                    for frame in &mut self.tracking_stack {
                        if frame.0 == sub_id && Some(frame.2) == lifetime {
                            tracked = true;
                            if pos < frame.1 { frame.1 -= 1; }
                        }
                    }
                    // Preserve the already-read prefix of active discovery passes.
                    if tracked { sub_sources.remove(pos); } else { sub_sources.swap_remove(pos); }
                }
            }
        }

        // 3. Mark dead and push to free_list
        let node = &mut self.nodes[idx];
        node.alive = false;
        node.in_queue = false; node.tracking = false;
        self.node_states[idx] = STATE_DISPOSED;
        if self.active_count > 0 {
            self.active_count -= 1;
        }
        self.free_list.push(id);
        self.compact_pending_effects_if_needed();
    }

    #[inline]
    pub fn link(&mut self, source: u32, target: u32) {
        let s = source as usize;
        let t = target as usize;
        if s >= self.nodes.len() || t >= self.nodes.len() || s == t {
            return;
        }
        if !self.nodes[s].alive || !self.nodes[t].alive {
            return;
        }
        if !self.subscribers[s].contains(&target) {
            self.subscribers[s].push(target);
            self.advance_topology_epoch();
        }
        if !self.sources[t].contains(&source) {
            self.sources[t].push(source);
        }
    }

    #[inline]
    pub fn clear_sources(&mut self, target: u32) {
        let t = target as usize;
        if t >= self.nodes.len() {
            return;
        }
        if let Some(node) = self.nodes.get(t).filter(|node| node.alive) {
            for frame in &mut self.tracking_stack {
                if frame.0 == target && frame.2 == node.lifetime { frame.1 = 0; }
            }
        }
        if !self.sources[t].is_empty() {
            self.advance_topology_epoch();
        }
        let prev_sources = std::mem::take(&mut self.sources[t]);
        for src in prev_sources {
            if let Some(subs) = self.subscribers.get_mut(src as usize) {
                if let Some(pos) = subs.iter().position(|&x| x == target) {
                    subs.swap_remove(pos);
                }
            }
        }
    }

    /// Replaces all incoming dependency edges of `target` with `new_sources` and marks `target` as `STATE_CLEAN`.
    pub fn sync_sources(&mut self, target: u32, new_sources: &[u32]) {
        self.clear_sources(target);
        let t = target as usize;
        if t >= self.nodes.len() || !self.nodes[t].alive {
            return;
        }
        for &src in new_sources {
            self.link(src, target);
        }
        self.node_states[t] = STATE_CLEAN;
    }
    /// Uses cursor-based edge verification: if dependencies are accessed in the same order,
    /// zero vector mutations or allocations occur!
    #[inline]
    pub fn begin_tracking(&mut self, target: u32) {
        if let Some(node) = self.nodes.get_mut(target as usize) {
            if node.alive && !node.tracking && (node.kind == KIND_COMPUTED || node.kind == KIND_EFFECT) {
                node.tracking = true;
                self.tracking_stack.push((target,0,node.lifetime)); return;
            }
        }
        // Preserve stack balance without attaching reads to an outer observer.
        self.tracking_stack.push((u32::MAX,0,0));
    }

    #[inline]
    pub fn pause_tracking(&mut self) { self.tracking_stack.push((u32::MAX,0,0)); }

    #[inline]
    pub fn resume_tracking(&mut self) {
        if self.tracking_stack.last().is_some_and(|frame| frame.0 == u32::MAX) { self.tracking_stack.pop(); }
    }

    /// Finish only the lifetime that began this frame, pruning unused trailing sources.
    #[inline]
    pub fn end_tracking(&mut self) {
        let Some((target,cursor,lifetime)) = self.tracking_stack.pop() else { return; };
        let t = target as usize;
        if !self.nodes.get(t).is_some_and(|node| node.alive && node.lifetime == lifetime
            && (node.kind == KIND_COMPUTED || node.kind == KIND_EFFECT)) { return; }
        self.nodes[t].tracking = false;
        let old_len = self.sources[t].len();
        if cursor < old_len {
            self.advance_topology_epoch();
            for i in cursor..old_len {
                let stale_src = self.sources[t][i] as usize;
                if let Some(subs) = self.subscribers.get_mut(stale_src) {
                    if let Some(pos) = subs.iter().position(|&id| id == target) { subs.swap_remove(pos); }
                }
            }
            self.sources[t].truncate(cursor);
        }
        self.node_states[t] = STATE_CLEAN;
    }

    /// Verify reads in place; reordering existing sources changes no graph edges.
    #[inline]
    pub fn track_read(&mut self, source: u32) {
        if !self.nodes.get(source as usize).is_some_and(|node| node.alive) { return; }
        let Some(&(observer,cursor,lifetime)) = self.tracking_stack.last() else { return; };
        let t = observer as usize;
        if source == observer || !self.nodes.get(t).is_some_and(|node| node.alive && node.lifetime == lifetime
            && (node.kind == KIND_COMPUTED || node.kind == KIND_EFFECT)) { return; }
        let cur = cursor.min(self.sources[t].len());
        if cur < self.sources[t].len() && self.sources[t][cur] == source {
            self.tracking_stack.last_mut().unwrap().1 = cur + 1; return;
        }
        if self.sources[t][..cur].contains(&source) { return; }
        if let Some(offset) = self.sources[t][cur..].iter().position(|&id| id == source) {
            self.sources[t].swap(cur,cur + offset);
            self.tracking_stack.last_mut().unwrap().1 = cur + 1; return;
        }
        if cur < self.sources[t].len() {
            let old = self.sources[t][cur] as usize;
            if let Some(subs) = self.subscribers.get_mut(old) {
                if let Some(pos) = subs.iter().position(|&id| id == observer) { subs.swap_remove(pos); }
            }
            self.sources[t][cur] = source;
            if !self.subscribers[source as usize].contains(&observer) { self.subscribers[source as usize].push(observer); }
            self.advance_topology_epoch();
        } else { self.link(source,observer); }
        self.tracking_stack.last_mut().unwrap().1 = cur + 1;
    }

    /// Trigger a change on `source` (signal or computed whose value changed)
    /// and immediately prepare `flush_buffer` if `batch_depth == 0`.
    /// Returns:
    /// - `0` if 0 effects were flushed
    /// - `effect_id + 1` (positive) if exactly 1 effect was flushed
    /// - `-(count as i32)` (negative) if 2 or more effects were flushed
    #[inline]
    pub fn trigger_and_flush_fast(&mut self, source: u32) -> i32 {
        let s = source as usize;
        if s >= self.nodes.len() || !self.nodes[s].alive {
            return 0;
        }
        // Fast path: if `batch_depth == 0` and this signal has a validated unique downstream effect,
        // mark its cached downstream computeds in a single flat loop without graph stack traversal!
        if self.batch_depth == 0 && self.terminal_cache_epoch[s] == self.topology_epoch {
            let cached = self.single_terminal_effect[s];
            if cached == u32::MAX - 1 {
                self.global_epoch = self.global_epoch.wrapping_add(1);
                self.nodes[s].epoch = self.global_epoch;
                return 0;
            }
            if cached < u32::MAX - 1 {
                self.global_epoch = self.global_epoch.wrapping_add(1);
                self.nodes[s].epoch = self.global_epoch;
                for &comp_id in &self.downstream_computeds[s] {
                    self.node_states[comp_id as usize] = STATE_DIRTY;
                }
                self.node_states[cached as usize] = STATE_DIRTY;
                return (cached as i32) + 1;
            }
        }

        self.trigger_write(source);
        let count = self.prepare_flush();
        if self.batch_depth == 0 {
            self.terminal_cache_epoch[s] = self.topology_epoch;
            if count == 0 {
                self.single_terminal_effect[s] = u32::MAX - 1;
            } else if count == 1 {
                self.single_terminal_effect[s] = self.flush_buffer[0];
                self.downstream_computeds[s].clear();
                // Only record computed nodes if this signal actually has computed subscribers!
                let has_computed_sub = self.subscribers[s]
                    .iter()
                    .any(|&sub| self.nodes[sub as usize].kind == KIND_COMPUTED);
                if has_computed_sub {
                    for (id_usize, node) in self.nodes.iter().enumerate() {
                        if node.alive
                            && node.kind == KIND_COMPUTED
                            && self.node_states[id_usize] != STATE_CLEAN
                        {
                            self.downstream_computeds[s].push(id_usize as u32);
                        }
                    }
                }
            } else {
                self.single_terminal_effect[s] = u32::MAX;
            }
        }
        if count == 0 {
            0
        } else if count == 1 {
            (self.flush_buffer[0] as i32) + 1
        } else {
            -(count as i32)
        }
    }
    /// Propagates `DIRTY` to direct children and `CHECK` to transitive dependents.
    pub fn trigger_write(&mut self, source: u32) {
        let s = source as usize;
        if s >= self.nodes.len() || !self.nodes[s].alive {
            return;
        }
        self.global_epoch = self.global_epoch.wrapping_add(1);
        self.nodes[s].epoch = self.global_epoch;

        self.propagate_stack.clear();
        for &sub in &self.subscribers[s] {
            self.propagate_stack.push((sub, STATE_DIRTY));
        }

        while let Some((curr_id, new_state)) = self.propagate_stack.pop() {
            let idx = curr_id as usize;
            let node = &mut self.nodes[idx];
            if !node.alive {
                continue;
            }

            let cur_state = self.node_states[idx];
            if cur_state < new_state {
                self.node_states[idx] = new_state;
            } else {
                continue;
            }

            if node.kind == KIND_EFFECT {
                if !node.in_queue {
                    node.in_queue = true;
                    self.pending_effects.push(ScheduledEffect { id: curr_id, lifetime: node.lifetime });
                }
            } else if node.kind == KIND_COMPUTED {
                for &sub in &self.subscribers[idx] {
                    self.propagate_stack.push((sub, STATE_CHECK));
                }
            }
        }
        self.compact_pending_effects_if_needed();
    }

    #[inline]
    pub fn get_state(&self, id: u32) -> u8 {
        self.node_states
            .get(id as usize)
            .copied()
            .unwrap_or(STATE_DISPOSED)
    }

    #[inline]
    pub fn set_state(&mut self, id: u32, state: u8) {
        if let Some(node) = self.nodes.get(id as usize) {
            if node.alive {
                self.node_states[id as usize] = state;
            }
        }
    }

    pub fn depends_on(&mut self, target: u32, source: u32) -> bool {
        if !self.nodes.get(target as usize).is_some_and(|n| n.alive)
            || !self.nodes.get(source as usize).is_some_and(|n| n.alive) { return false; }
        if target == source { return true; }
        if self.subscribers[source as usize].is_empty() { return false; }
        self.dependency_seen.resize(self.nodes.len(), 0);
        self.dependency_epoch = self.dependency_epoch.wrapping_add(1);
        if self.dependency_epoch == 0 { self.dependency_seen.fill(0); self.dependency_epoch = 1; }
        let epoch = self.dependency_epoch;
        self.dependency_stack.clear(); self.dependency_stack.push(target);
        let mut found = false;
        while let Some(id) = self.dependency_stack.pop() {
            if id == source { found = true; break; }
            let i = id as usize;
            if self.dependency_seen[i] == epoch { continue; }
            self.dependency_seen[i] = epoch;
            self.dependency_stack.extend(self.sources[i].iter().copied());
        }
        self.dependency_stack.clear();
        found
    }

    fn compact_pending_effects_if_needed(&mut self) {
        // Compact only after enough churn, keeping scans amortized rather than
        // removing one token by a full queue scan on every disposal.
        let limit = (self.active_count as usize).saturating_mul(2).saturating_add(64);
        if self.pending_effects.len() <= limit { return; }
        let nodes = &self.nodes;
        self.pending_effects.retain(|pending| nodes.get(pending.id as usize).is_some_and(|node|
            node.alive && node.kind == KIND_EFFECT && node.in_queue && node.lifetime == pending.lifetime));
    }

    pub fn pending_effect_count(&self) -> usize {
        self.pending_effects.iter().filter(|pending| {
            self.nodes.get(pending.id as usize).is_some_and(|n| n.alive && n.kind == KIND_EFFECT && n.in_queue
                && n.lifetime == pending.lifetime && self.node_states[pending.id as usize] != STATE_CLEAN)
        }).count()
    }

    /// Drains scheduled effects from matching live slot lifetimes into the uint32 JS flush buffer.
    pub fn prepare_flush(&mut self) -> usize {
        self.flush_buffer.clear();
        if self.batch_depth > 0 {
            return 0;
        }
        for pending in &self.pending_effects {
            if let Some(node) = self.nodes.get_mut(pending.id as usize) {
                if node.alive && node.kind == KIND_EFFECT && node.in_queue && node.lifetime == pending.lifetime {
                    node.in_queue = false;
                    self.flush_buffer.push(pending.id);
                }
            }
        }
        self.pending_effects.clear();
        self.flush_buffer.len()
    }

    /// Computes Longest Increasing Subsequence (LIS) of `input` (ignoring `u32::MAX` which denotes new items).
    /// Writes indices of the LIS in ascending order into `self.lis_buffer` and returns the count.
    /// Complexity: O(n log n) time, O(n) memory in WASM linear heap.
    pub fn compute_lis(&mut self, input: &[u32]) -> usize {
        let n = input.len();
        self.lis_buffer.clear();
        if n == 0 {
            return 0;
        }

        let mut tails: Vec<usize> = Vec::with_capacity(n);
        let mut predecessors: Vec<usize> = vec![usize::MAX; n];

        for i in 0..n {
            let val = input[i];
            if val == u32::MAX {
                continue;
            }

            if tails.is_empty() || input[*tails.last().unwrap()] < val {
                if let Some(&last_idx) = tails.last() {
                    predecessors[i] = last_idx;
                }
                tails.push(i);
            } else {
                let mut low = 0usize;
                let mut high = tails.len() - 1;
                while low < high {
                    let mid = low + (high - low) / 2;
                    if input[tails[mid]] < val {
                        low = mid + 1;
                    } else {
                        high = mid;
                    }
                }
                if val < input[tails[low]] {
                    if low > 0 {
                        predecessors[i] = tails[low - 1];
                    }
                    tails[low] = i;
                }
            }
        }

        let lis_len = tails.len();
        if lis_len == 0 {
            return 0;
        }

        self.lis_buffer.resize(lis_len, 0);
        let mut k = tails[lis_len - 1];
        for idx in (0..lis_len).rev() {
            self.lis_buffer[idx] = k as u32;
            k = predecessors[k];
        }

        lis_len
    }
}

struct GlobalGraph(UnsafeCell<ReactiveGraph>);
unsafe impl Sync for GlobalGraph {}

static GRAPH: GlobalGraph = GlobalGraph(UnsafeCell::new(ReactiveGraph::new()));

#[inline]
fn with_graph<R>(f: impl FnOnce(&mut ReactiveGraph) -> R) -> R {
    unsafe { f(&mut *GRAPH.0.get()) }
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
        g.tracking_stack.last().is_some_and(|&(target,_,lifetime)| g.nodes.get(target as usize)
            .is_some_and(|node| node.alive && node.lifetime == lifetime && (node.kind == KIND_COMPUTED || node.kind == KIND_EFFECT))) as u32
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
pub extern "C" fn svet_gpu_valid_perspective(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> u32 {
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
    if len > 67_108_864 { return std::ptr::null_mut(); }
    with_graph(|g| { g.gpu_pick_vertices.resize(len, 0.0); g.gpu_pick_vertices.as_mut_ptr() })
}
#[no_mangle]
pub extern "C" fn svet_gpu_pick_snapshot_3d(count: u32, indices: u32, occlude: u32, vw: f32, vh: f32, x: f32, y: f32, radius: f32) -> i32 {
    with_graph(|g| g.gpu_pick_snapshot_3d(count, indices, occlude, vw, vh, x, y, radius))
}

#[no_mangle]
pub extern "C" fn svet_prepare_gpu_pick_indices(len: usize) -> *mut u32 {
    with_graph(|g| { g.gpu_pick_indices.resize(len, 0); g.gpu_pick_indices.as_mut_ptr() })
}
#[no_mangle]
pub extern "C" fn svet_gpu_pick_visible_vertex_3d(offset: u32, count: u32, indices: u32, vw: f32, vh: f32, x: f32, y: f32, radius: f32) -> i32 {
    with_graph(|g| g.gpu_pick_visible_vertex_3d(offset, count, indices, vw, vh, x, y, radius))
}

/// Internal scratch result consumed synchronously by Canvas2D transport.
#[no_mangle]
pub extern "C" fn svet_gpu_clip_primitive_3d(offset: u32, count: u32, a: u32, b: u32, c: u32, d: u32, size: u32) -> u32 {
    if !(2..=4).contains(&size) { return with_graph(|g| { g.gpu_clip_output.clear(); 0 }); }
    with_graph(|g| g.gpu_clip_primitive_3d(offset, count, &[a, b, c, d][..size as usize]))
}
#[no_mangle]
pub extern "C" fn svet_gpu_clip_output_ptr() -> *const f32 {
    with_graph(|g| g.gpu_clip_output.as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diamond_dependency_glitch_free() {
        let mut g = ReactiveGraph::new();
        let a = g.create_node(KIND_SIGNAL);
        let b = g.create_node(KIND_COMPUTED);
        let c = g.create_node(KIND_COMPUTED);
        let d = g.create_node(KIND_EFFECT);

        g.begin_tracking(b);
        g.track_read(a);
        g.end_tracking();

        g.begin_tracking(c);
        g.track_read(a);
        g.end_tracking();

        g.begin_tracking(d);
        g.track_read(b);
        g.track_read(c);
        g.end_tracking();

        g.trigger_write(a);

        assert_eq!(g.get_state(b), STATE_DIRTY);
        assert_eq!(g.get_state(c), STATE_DIRTY);

        let count = g.prepare_flush();
        assert_eq!(count, 1);
        assert_eq!(g.flush_buffer[0], d);
    }

    #[test]
    fn test_free_list_slot_recycling() {
        let mut g = ReactiveGraph::new();
        let n0 = g.create_node(KIND_SIGNAL);
        let n1 = g.create_node(KIND_EFFECT);
        assert_eq!(g.active_count, 2);
        assert_eq!(g.nodes.len(), 2);

        g.dispose_node(n1);
        assert_eq!(g.active_count, 1);
        assert_eq!(g.nodes.len(), 2);

        // Next allocated node MUST reuse slot `n1` without growing `nodes` vector
        let n2 = g.create_node(KIND_EFFECT);
        assert_eq!(n2, n1);
        assert_eq!(g.active_count, 2);
        assert_eq!(g.nodes.len(), 2);
        let _ = n0;
    }

    #[test]
    fn test_lis_reconciler() {
        let mut g = ReactiveGraph::new();
        let input = [2, 1, 5, 3, 6, 4, 8, 9, 7];
        let len = g.compute_lis(&input);
        assert_eq!(len, 5);
        let lis_values: Vec<u32> = g.lis_buffer.iter().map(|&idx| input[idx as usize]).collect();
        assert_eq!(lis_values, vec![1, 3, 4, 8, 9]);
    }

    #[test]
    fn test_gpu_uma_duma_dirty_page_tracking() {
        let mut g = ReactiveGraph::new();
        let off_a = g.gpu_alloc_f32(64);
        let off_b = g.gpu_alloc_f32(64);
        assert_eq!(off_a, 0);
        assert_eq!(off_b, 64);

        g.gpu_clear_dirty();
        assert_eq!(g.gpu_dirty_min, u32::MAX);
        assert_eq!(g.gpu_dirty_max, 0);

        // Mutate a slice inside tensor B [70..78]
        g.gpu_mark_dirty(70, 8);
        assert_eq!(g.gpu_dirty_min, 70);
        assert_eq!(g.gpu_dirty_max, 78);
    }
    #[test]
    fn test_gpu_arena_growth_caps_reserved_storage() {
        let mut arena = Vec::new();
        assert!(grow_gpu_arena(&mut arena, 40, 64));
        arena[0] = 17.0;
        assert!(grow_gpu_arena(&mut arena, 44, 64));
        assert_eq!(arena.len(), 44);
        assert!(arena.capacity() <= 64, "reserved {} floats above limit 64", arena.capacity());
        assert_eq!(arena[0], 17.0);
        assert!(arena[40..].iter().all(|&value| value == 0.0));
    }

    #[test]
    fn test_gpu_arena_growth_is_amortized_and_preserves_zeroed_tail() {
        let mut arena = Vec::new();
        let mut changes = 0;
        for length in 1..=64 {
            let previous_capacity = arena.capacity();
            assert!(grow_gpu_arena(&mut arena, length, 64));
            changes += (previous_capacity != arena.capacity()) as usize;
            assert!(arena.capacity() <= 64);
            assert_eq!(arena[length - 1], 0.0);
            if length > 1 { assert_eq!(arena[0], 17.0); }
            arena[0] = 17.0;
        }
        assert!(changes < 16, "growth reserved on too many individual steps: {changes}");
        let pointer = arena.as_ptr();
        assert!(!grow_gpu_arena(&mut arena, 65, 64));
        assert_eq!(arena.as_ptr(), pointer);
        assert_eq!(arena.len(), 64);
        assert_eq!(arena[0], 17.0);
    }

    #[test]
    fn test_gpu_arena_reservation_overflow_preserves_existing_storage() {
        let mut arena = vec![17.0; 4];
        let capacity = arena.capacity();
        let pointer = arena.as_ptr();
        assert!(!grow_gpu_arena(&mut arena, usize::MAX, usize::MAX));
        assert_eq!(arena.as_ptr(), pointer);
        assert_eq!(arena.capacity(), capacity);
        assert_eq!(arena, vec![17.0; 4]);
    }

    #[test]
    fn test_gpu_allocation_failure_preserves_existing_arena() {
        let mut g = ReactiveGraph::new();
        assert_eq!(g.gpu_alloc_f32(4), 0);
        g.gpu_f32_arena[0] = 42.0;
        assert_eq!(g.gpu_alloc_f32(u32::MAX), u32::MAX);
        let limit = svet_gpu_get_arena_limit_f32();
        assert_eq!(limit, 67_108_864);
        assert_eq!(g.gpu_alloc_f32(limit + 1), u32::MAX);
        assert_eq!(g.gpu_f32_arena.len(), 4);
        assert_eq!(g.gpu_f32_arena[0], 42.0);
    }

    #[test]
    fn test_gpu_picker_matches_webgpu_viewport_projection() {
        let mut g = ReactiveGraph::new();
        let offset = g.gpu_alloc_f32(4);
        g.gpu_f32_arena[..4].copy_from_slice(&[0.8, 0.4, 0.2, 1.0]);
        assert_eq!(g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 180.0, 60.0, 1.0), 0);
        assert_eq!(g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 152.0, 74.0, 1.0), -1);
    }

    #[test]
    fn test_gpu_picker_rejects_clipped_vertices_and_invalid_queries() {
        let mut g = ReactiveGraph::new();
        let offset = g.gpu_alloc_f32(4);
        for vertex in [[0.0,0.0,0.2,-1.0], [0.0,0.0,-0.1,1.0], [0.0,0.0,1.1,1.0],
            [1.1,0.0,0.2,1.0], [0.0,1.1,0.2,1.0], [0.0,0.0,0.0,0.0], [f32::NAN,0.0,0.2,1.0]] {
            g.gpu_f32_arena[..4].copy_from_slice(&vertex);
            assert_eq!(g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 100.0, 100.0, 1000.0), -1);
        }
        for w in [0.000001, f32::MIN_POSITIVE, f32::from_bits(1)] {
            g.gpu_f32_arena[..4].copy_from_slice(&[0.0, 0.0, 0.0, w]);
            assert_eq!(g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 100.0, 100.0, 0.0), 0);
        }
        g.gpu_f32_arena[..4].copy_from_slice(&[0.0, 0.0, 0.0000005, 0.000001]);
        assert_eq!(g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 100.0, 100.0, 0.0), 0);
        assert_eq!(g.gpu_pick_nearest_vertex_3d(offset, 1, 0.0, 200.0, 100.0, 100.0, 1.0), -1);
        assert_eq!(g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 100.0, 100.0, -1.0), -1);
        assert_eq!(g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, f32::NAN, 100.0, 1.0), -1);
    }

    #[test]
    fn test_homogeneous_segment_clipping() {
        let mut g = ReactiveGraph::new(); let offset = g.gpu_alloc_f32(8);
        g.gpu_f32_arena[..8].copy_from_slice(&[-0.5, 0.0, -0.5, 1.0, 0.5, 0.0, 0.5, 1.0]);
        assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[0,1]), 2);
        assert_eq!(g.gpu_clip_output, vec![0.0,0.0,0.0,1.0, 0.5,0.0,0.5,1.0]);
        g.gpu_f32_arena[..8].copy_from_slice(&[-2.0,0.0,0.5,1.0, 2.0,0.0,0.5,1.0]);
        assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[0,1]), 2);
        assert_eq!(g.gpu_clip_output, vec![-1.0,0.0,0.5,1.0, 1.0,0.0,0.5,1.0]);
        assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[0,2]), 0);
        assert!(g.gpu_clip_output.is_empty());
        assert_eq!(g.gpu_clip_primitive_3d(u32::MAX, u32::MAX, &[0,1]), 0);
    }

    #[test]
    fn test_homogeneous_polygon_clipping() {
        let mut g = ReactiveGraph::new(); let offset = g.gpu_alloc_f32(16);
        g.gpu_f32_arena[..16].copy_from_slice(&[-0.5,-0.5,-0.5,1.0, 0.5,-0.5,0.5,1.0,
            0.5,0.5,0.5,1.0, -0.5,0.5,-0.5,1.0]);
        assert_eq!(g.gpu_clip_primitive_3d(offset, 4, &[0,1,2,3]), 4);
        for v in g.gpu_clip_output.chunks_exact(4) {
            assert!(v[0] >= 0.0 && v[0] <= 0.5); assert!(v[2] >= 0.0 && v[2] <= v[3]);
        }
        // A completely clipped polygon must clear the previous scratch result.
        g.gpu_f32_arena[..16].fill(-1.0);
        assert_eq!(g.gpu_clip_primitive_3d(offset, 4, &[0,1,2,3]), 0);
        assert!(g.gpu_clip_output.is_empty());
        g.gpu_f32_arena[0] = f32::NAN;
        assert_eq!(g.gpu_clip_primitive_3d(offset, 4, &[0,1,2,3]), 0);
    }

    #[test]
    fn test_segment_clipping_all_planes_and_seeded_reference_samples() {
        let mut g = ReactiveGraph::new(); let offset = g.gpu_alloc_f32(8);
        for outside in [[-2.0,0.0,0.5,1.0], [2.0,0.0,0.5,1.0], [0.0,-2.0,0.5,1.0],
            [0.0,2.0,0.5,1.0], [0.0,0.0,-1.0,1.0], [0.0,0.0,2.0,1.0]] {
            g.gpu_f32_arena[..4].copy_from_slice(&outside);
            g.gpu_f32_arena[4..8].copy_from_slice(&[0.0,0.0,0.5,1.0]);
            assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[0,1]), 2);
            let forward = g.gpu_clip_output.clone();
            assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[1,0]), 2);
            assert_eq!(&forward[..4], &g.gpu_clip_output[4..]);
        }
        let mut seed: u32 = 42;
        let mut next = || { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); (seed >> 8) as f32 / 16777216.0 * 4.0 - 2.0 };
        for _ in 0..500 {
            let a = [next(),next(),next(),next() + 1.0];
            let b = [next(),next(),next(),next() + 1.0];
            g.gpu_f32_arena[..4].copy_from_slice(&a); g.gpu_f32_arena[4..8].copy_from_slice(&b);
            let size = g.gpu_clip_primitive_3d(offset, 2, &[0,1]);
            let mut sampled_inside = false;
            for step in 0..=32 {
                let t = step as f32 / 32.0;
                let v: [f32; 4] = std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t);
                sampled_inside |= v[3] > 0.0 && v[0].abs() <= v[3] && v[1].abs() <= v[3] && v[2] >= 0.0 && v[2] <= v[3];
            }
            if sampled_inside { assert_eq!(size, 2); }
            for v in g.gpu_clip_output.chunks_exact(4) {
                assert!(v.iter().all(|value| value.is_finite()));
                assert!(v[3] > 0.0 && v[0].abs() <= v[3] + 1e-5 && v[1].abs() <= v[3] + 1e-5);
                assert!(v[2] >= -1e-5 && v[2] <= v[3] + 1e-5);
            }
        }
    }

    #[test]
    fn test_visible_vertex_picker_skips_hidden_candidate_and_finds_visible_neighbor() {
        let mut g = ReactiveGraph::new(); let offset = g.gpu_alloc_f32(20);
        g.gpu_f32_arena[..20].copy_from_slice(&[-0.8,-0.8,0.2,1.0, 0.8,-0.8,0.2,1.0, 0.0,0.8,0.2,1.0,
            0.0,0.0,0.8,1.0, 0.15,0.0,0.1,1.0]);
        g.gpu_pick_indices = vec![0,1,2];
        assert_eq!(g.gpu_pick_nearest_vertex_3d(offset, 4, 200.0,200.0,100.0,100.0,20.0), 3);
        assert_eq!(g.gpu_pick_visible_vertex_3d(offset, 4, 3, 200.0,200.0,100.0,100.0,20.0), -1);
        assert_eq!(g.gpu_pick_visible_vertex_3d(offset, 5, 3, 200.0,200.0,100.0,100.0,20.0), 4);
        assert_eq!(g.gpu_pick_visible_vertex_3d(offset, 5, 3, 200.0,200.0,20.0,180.0,1.0), 0);
        g.gpu_f32_arena[14] = 0.2;
        assert_eq!(g.gpu_pick_visible_vertex_3d(offset, 5, 3, 200.0,200.0,100.0,100.0,1.0), 3);
    }

    #[test]
    fn test_visible_picker_clips_triangles_and_handles_perspective_winding() {
        let mut g = ReactiveGraph::new(); let offset = g.gpu_alloc_f32(16);
        g.gpu_f32_arena[..16].copy_from_slice(&[-0.8,-0.8,-0.2,1.0, 0.8,-0.8,0.2,1.0, 0.0,0.8,0.2,1.0, 0.0,0.0,0.8,1.0]);
        g.gpu_pick_indices = vec![0,1,2];
        assert_eq!(g.gpu_pick_visible_vertex_3d(offset, 4, 3, 200.0,200.0,100.0,100.0,1.0), -1);
        g.gpu_f32_arena[..12].copy_from_slice(&[-0.8,-0.8,0.2,1.0, 1.6,-1.6,0.4,2.0, 0.0,0.4,0.1,0.5]);
        g.gpu_pick_indices = vec![2,1,0];
        assert_eq!(g.gpu_pick_visible_vertex_3d(offset, 4, 3, 200.0,200.0,100.0,100.0,1.0), -1);
        g.gpu_pick_indices.clear();
        assert_eq!(g.gpu_pick_visible_vertex_3d(offset, 4, 0, 200.0,200.0,100.0,100.0,1.0), 3);
    }

    #[test]
    fn test_snapshot_picker_preserves_live_geometry_and_owns_only_scratch() {
        let mut g = ReactiveGraph::new(); g.create_node(KIND_SIGNAL);
        g.gpu_f32_arena = vec![111.0,222.0,333.0,444.0];
        g.gpu_pick_vertices = vec![-0.8,-0.8,0.2,1.0, 0.8,-0.8,0.2,1.0, 0.0,0.8,0.2,1.0, 0.0,0.0,0.8,1.0];
        g.gpu_pick_indices = vec![0,1,2];
        let pointer = g.gpu_pick_vertices.as_ptr();
        assert_eq!(g.gpu_pick_snapshot_3d(4,3,1,200.0,200.0,100.0,100.0,1.0), -1);
        assert_eq!(g.gpu_pick_snapshot_3d(4,3,0,200.0,200.0,100.0,100.0,1.0), 3);
        assert_eq!(g.gpu_f32_arena, vec![111.0,222.0,333.0,444.0]);
        assert_eq!(g.active_count, 1); assert_eq!(g.gpu_pick_vertices.as_ptr(), pointer);
        assert_eq!(g.gpu_pick_indices, vec![0,1,2]);
    }

    #[test]
    fn perspective_validation_rejects_invalid_and_degenerate_float32_parameters() {
        for p in [[0.0,1.0,0.1,100.0], [std::f32::consts::PI,1.0,0.1,100.0],
            [1.0,0.0,0.1,100.0], [1.0,1.0,0.0,100.0], [1.0,1.0,10.0,10.0],
            [f32::NAN,1.0,0.1,100.0], [1.0,f32::INFINITY,0.1,100.0],
            [f32::from_bits(1),1.0,0.1,100.0], [1.0,f32::from_bits(1),0.1,100.0]] {
            assert!(perspective_matrix(p[0],p[1],p[2],p[3]).is_none());
        }
    }

    #[test]
    fn perspective_rejection_preserves_destination_and_dirty_state() {
        let mut g = ReactiveGraph::new(); let offset = g.gpu_alloc_f32(16);
        g.gpu_f32_arena[..16].fill(7.0); g.gpu_clear_dirty();
        g.gpu_mat4_perspective(offset,1.0,0.0,0.1,100.0);
        assert_eq!(g.gpu_f32_arena, vec![7.0;16]);
        assert_eq!(g.gpu_dirty_min, u32::MAX);
    }

    #[test]
    fn perspective_large_depth_range_avoids_intermediate_overflow() {
        let m = perspective_matrix(1.0,1.0,1.0e20,1.0e30).unwrap();
        assert!(m.iter().all(|v| v.is_finite())); assert!(m[14] < -9.0e19);
        for (z, expected) in [(-1.0e20,0.0),(-1.0e30,1.0)] {
            let depth = (m[10]*z + m[14]) / -z;
            assert!((depth - expected).abs() < 1.0e-6);
        }
    }

    #[test]
    fn dependency_reachability_handles_diamonds_cycles_and_disposal() {
        let mut g = ReactiveGraph::new();
        let s = g.create_node(KIND_SIGNAL); let a = g.create_node(KIND_COMPUTED);
        let b = g.create_node(KIND_COMPUTED); let c = g.create_node(KIND_COMPUTED);
        g.link(s,a); g.link(s,b); g.link(a,c); g.link(b,c);
        assert!(g.depends_on(c,s)); assert!(!g.depends_on(s,c));
        g.link(c,a); let independent = g.create_node(KIND_SIGNAL); assert!(!g.depends_on(c,independent));
        g.dispose_node(s); assert!(!g.depends_on(c,s));
    }

    #[test]
    fn dependency_queries_reuse_scratch_and_leave_reactive_state_unchanged() {
        let mut g = ReactiveGraph::new();
        let source = g.create_node(KIND_SIGNAL); let derived = g.create_node(KIND_COMPUTED);
        g.link(source,derived); assert!(g.depends_on(derived,source));
        let stack = g.dependency_stack.as_ptr(); let seen = g.dependency_seen.as_ptr();
        let states = g.node_states.clone(); let topology = g.topology_epoch; let clock = g.global_epoch;
        for _ in 0..100 { assert!(g.depends_on(derived,source)); }
        assert_eq!(g.dependency_stack.as_ptr(),stack); assert_eq!(g.dependency_seen.as_ptr(),seen);
        assert_eq!(g.node_states,states); assert_eq!(g.topology_epoch,topology); assert_eq!(g.global_epoch,clock);
        g.dependency_epoch = u32::MAX;
        assert!(g.depends_on(derived,source));
        assert_eq!(g.node_states,states);
    }

    #[test]
    fn pending_queue_does_not_run_a_replacement_that_was_never_scheduled() {
        let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL);
        let old = g.create_node(KIND_EFFECT); g.sync_sources(old,&[source]); g.trigger_write(source);
        g.dispose_node(old); let fresh = g.create_node(KIND_EFFECT); assert_eq!(fresh,old);
        assert_eq!(g.prepare_flush(),0);
    }

    #[test]
    fn pending_queue_preserves_real_schedule_order_when_a_slot_is_reused() {
        let mut g = ReactiveGraph::new();
        let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL); let c = g.create_node(KIND_SIGNAL);
        let old = g.create_node(KIND_EFFECT); let peer = g.create_node(KIND_EFFECT);
        g.sync_sources(old,&[a]); g.sync_sources(peer,&[b]); g.trigger_write(a); g.trigger_write(b);
        g.dispose_node(old); let fresh = g.create_node(KIND_EFFECT); assert_eq!(fresh,old);
        g.sync_sources(fresh,&[c]); g.trigger_write(c);
        assert_eq!(g.prepare_flush(),2); assert_eq!(g.flush_buffer,vec![peer,fresh]);
        assert_eq!(g.prepare_flush(),0);
    }

    #[test]
    fn pending_queue_rejects_replacement_signal_and_computed_slots() {
        for kind in [KIND_SIGNAL,KIND_COMPUTED] {
            let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL);
            let old = g.create_node(KIND_EFFECT); g.sync_sources(old,&[source]); g.trigger_write(source);
            g.dispose_node(old); assert_eq!(g.create_node(kind),old); assert_eq!(g.prepare_flush(),0);
        }
    }

    #[test]
    fn pending_lifetime_wrap_purges_old_tokens_without_dropping_other_jobs() {
        let mut g = ReactiveGraph::new();
        let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL);
        let old = g.create_node(KIND_EFFECT); let peer = g.create_node(KIND_EFFECT);
        g.pending_effects.push(ScheduledEffect { id: old, lifetime: 1 });
        g.nodes[old as usize].lifetime = u32::MAX;
        g.sync_sources(old,&[a]); g.sync_sources(peer,&[b]); g.trigger_write(a); g.trigger_write(b);
        g.dispose_node(old); let fresh = g.create_node(KIND_EFFECT); assert_eq!(fresh,old);
        assert_eq!(g.nodes[fresh as usize].lifetime,1);
        assert!(g.pending_effects.iter().all(|pending| pending.id != old));
        assert_eq!(g.pending_effect_count(),1);
        g.sync_sources(fresh,&[a]); g.trigger_write(a);
        assert_eq!(g.pending_effect_count(),2); assert_eq!(g.prepare_flush(),2);
        assert_eq!(g.flush_buffer,vec![peer,fresh]);
    }

    #[test]
    fn pending_count_observes_dirty_work_without_consuming_it() {
        let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL);
        let effect = g.create_node(KIND_EFFECT); g.sync_sources(effect,&[source]); g.trigger_write(source);
        assert_eq!(g.pending_effect_count(),1); assert_eq!(g.pending_effect_count(),1);
        assert!(g.nodes[effect as usize].in_queue); assert_eq!(g.pending_effects.len(),1);
        g.set_state(effect,STATE_CLEAN); assert_eq!(g.pending_effect_count(),0);
        g.trigger_write(source); assert_eq!(g.pending_effect_count(),1);
        assert_eq!(g.pending_effects.len(),1); assert_eq!(g.prepare_flush(),1);
        assert_eq!(g.pending_effect_count(),0);
    }

    #[test]
    fn pending_churn_keeps_entry_count_and_capacity_bounded_without_flushing() {
        let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL);
        let mut warm_capacity = 0;
        for i in 0..10_000 {
            let effect = g.create_node(KIND_EFFECT); g.sync_sources(effect,&[source]);
            g.trigger_write(source); g.dispose_node(effect);
            assert!(g.pending_effects.len() <= 66, "expired pending jobs grew with churn");
            if i == 200 { warm_capacity = g.pending_effects.capacity(); }
        }
        assert_eq!(g.active_count,1); assert_eq!(g.nodes.len(),2);
        assert_eq!(g.pending_effects.capacity(),warm_capacity);
        assert_eq!(g.pending_effect_count(),0); assert_eq!(g.prepare_flush(),0);
    }

    #[test]
    fn pending_compaction_preserves_live_job_order_and_flags() {
        let mut g = ReactiveGraph::new(); let churn = g.create_node(KIND_SIGNAL);
        let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL);
        let left = g.create_node(KIND_EFFECT); let right = g.create_node(KIND_EFFECT);
        g.sync_sources(left,&[a]); g.sync_sources(right,&[b]); g.trigger_write(a); g.trigger_write(b);
        for _ in 0..1000 {
            let effect = g.create_node(KIND_EFFECT); g.sync_sources(effect,&[churn]); g.trigger_write(churn); g.dispose_node(effect);
        }
        assert!(g.pending_effects.len() <= 74);
        assert!(g.nodes[left as usize].in_queue && g.nodes[right as usize].in_queue);
        assert_eq!(g.pending_effect_count(),2); assert_eq!(g.prepare_flush(),2);
        assert_eq!(g.flush_buffer,vec![left,right]);
    }

    #[test]
    fn pending_compaction_retains_clean_scheduled_jobs_for_later_writes() {
        let mut g = ReactiveGraph::new(); let churn = g.create_node(KIND_SIGNAL); let source = g.create_node(KIND_SIGNAL);
        let live = g.create_node(KIND_EFFECT); g.sync_sources(live,&[source]); g.trigger_write(source); g.set_state(live,STATE_CLEAN);
        for _ in 0..1000 {
            let effect = g.create_node(KIND_EFFECT); g.sync_sources(effect,&[churn]); g.trigger_write(churn); g.dispose_node(effect);
        }
        assert!(g.pending_effects.len() <= 70); assert_eq!(g.pending_effect_count(),0);
        g.trigger_write(source); assert_eq!(g.pending_effect_count(),1);
        assert_eq!(g.prepare_flush(),1); assert_eq!(g.flush_buffer,vec![live]);
    }

    #[test]
    fn pending_population_drop_compacts_expired_entries_without_new_writes() {
        let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL);
        let effects: Vec<_> = (0..1000).map(|_| { let id = g.create_node(KIND_EFFECT); g.sync_sources(id,&[source]); id }).collect();
        g.trigger_write(source);
        for &id in &effects[1..] { g.dispose_node(id); }
        assert!(g.pending_effects.len() <= 68); assert_eq!(g.pending_effect_count(),1);
        assert_eq!(g.prepare_flush(),1); assert_eq!(g.flush_buffer,vec![effects[0]]);
    }

    #[test]
    fn topology_wrap_does_not_restore_a_stale_no_subscriber_cache() {
        let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL);
        assert_eq!(g.trigger_and_flush_fast(source),0);
        let effect = g.create_node(KIND_EFFECT); let other = g.create_node(KIND_SIGNAL); let derived = g.create_node(KIND_COMPUTED);
        g.topology_epoch = u32::MAX;
        g.sync_sources(effect,&[source]); g.link(other,derived);
        assert_eq!(g.trigger_and_flush_fast(source),(effect as i32)+1);
    }

    #[test]
    fn topology_wrap_does_not_restore_a_detached_effect_cache() {
        let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL); let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(effect,&[source]); assert_eq!(g.trigger_and_flush_fast(source),(effect as i32)+1);
        g.set_state(effect,STATE_CLEAN);
        let other = g.create_node(KIND_SIGNAL); let a = g.create_node(KIND_COMPUTED); let b = g.create_node(KIND_COMPUTED);
        g.topology_epoch = u32::MAX; g.clear_sources(effect); g.link(other,a); g.link(other,b);
        assert_eq!(g.trigger_and_flush_fast(source),0);
    }

    #[test]
    fn topology_wrap_cannot_dirty_a_signal_reusing_a_cached_effect_slot() {
        let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL); let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(effect,&[source]); assert_eq!(g.trigger_and_flush_fast(source),(effect as i32)+1);
        let other = g.create_node(KIND_SIGNAL); let derived = g.create_node(KIND_COMPUTED);
        g.topology_epoch = u32::MAX; g.dispose_node(effect); let replacement = g.create_node(KIND_SIGNAL);
        assert_eq!(replacement,effect); g.link(other,derived);
        assert_eq!(g.trigger_and_flush_fast(source),0); assert_eq!(g.get_state(replacement),STATE_CLEAN);
    }

    #[test]
    fn topology_wrap_invalidates_cached_downstream_computed_slots() {
        let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL);
        let derived = g.create_node(KIND_COMPUTED); let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(derived,&[source]); g.sync_sources(effect,&[derived]);
        assert_eq!(g.trigger_and_flush_fast(source),(effect as i32)+1);
        assert_eq!(g.downstream_computeds[source as usize],vec![derived]);
        g.topology_epoch = u32::MAX; g.clear_sources(derived); g.clear_sources(effect); g.dispose_node(derived); g.dispose_node(effect);
        let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL);
        assert_eq!(a,effect); assert_eq!(b,derived);
        assert_eq!(g.trigger_and_flush_fast(source),0);
        assert_eq!(g.get_state(a),STATE_CLEAN); assert_eq!(g.get_state(b),STATE_CLEAN);
    }

    #[test]
    fn topology_wrap_rebuilds_cache_and_noop_links_preserve_it() {
        let mut g = ReactiveGraph::new(); let source = g.create_node(KIND_SIGNAL); let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(effect,&[source]); assert_eq!(g.trigger_and_flush_fast(source),(effect as i32)+1);
        g.set_state(effect,STATE_CLEAN);
        let other = g.create_node(KIND_SIGNAL); let derived = g.create_node(KIND_COMPUTED);
        g.topology_epoch = u32::MAX; g.link(other,derived);
        assert_eq!(g.topology_epoch,1); assert_eq!(g.terminal_cache_epoch[source as usize],0);
        assert_eq!(g.single_terminal_effect[source as usize],u32::MAX);
        assert_eq!(g.trigger_and_flush_fast(source),(effect as i32)+1);
        assert_eq!(g.terminal_cache_epoch[source as usize],g.topology_epoch);
        let epoch = g.topology_epoch; g.link(source,effect); g.clear_sources(other);
        assert_eq!(g.topology_epoch,epoch); assert_eq!(g.terminal_cache_epoch[source as usize],epoch);
        g.set_state(effect,STATE_CLEAN); assert_eq!(g.trigger_and_flush_fast(source),(effect as i32)+1);
        assert_eq!(g.get_state(effect),STATE_DIRTY);
    }

    #[test]
    fn cursor_pruning_invalidates_cached_terminal_effects() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL); let e = g.create_node(KIND_EFFECT);
        g.sync_sources(e,&[a,b]); assert_eq!(g.trigger_and_flush_fast(b),(e as i32)+1); g.set_state(e,STATE_CLEAN);
        g.begin_tracking(e); g.track_read(a); g.end_tracking();
        assert_eq!(g.trigger_and_flush_fast(b),0);
    }

    #[test]
    fn cursor_replacement_invalidates_both_old_and_new_source_caches() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL); let e = g.create_node(KIND_EFFECT);
        g.sync_sources(e,&[a]); assert_eq!(g.trigger_and_flush_fast(a),(e as i32)+1); g.set_state(e,STATE_CLEAN); assert_eq!(g.trigger_and_flush_fast(b),0);
        g.begin_tracking(e); g.track_read(b); g.end_tracking();
        assert_eq!(g.trigger_and_flush_fast(a),0); assert_eq!(g.trigger_and_flush_fast(b),(e as i32)+1);
    }

    #[test]
    fn cursor_dependency_reordering_preserves_both_forward_edges() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL); let e = g.create_node(KIND_EFFECT);
        g.sync_sources(e,&[a,b]); g.begin_tracking(e); g.track_read(b); g.track_read(a); g.end_tracking();
        assert_eq!(g.sources[e as usize],vec![b,a]); assert!(g.subscribers[a as usize].contains(&e)); assert!(g.subscribers[b as usize].contains(&e));
        assert_eq!(g.trigger_and_flush_fast(b),(e as i32)+1);
    }

    #[test]
    fn cursor_ignores_self_and_invalid_reads_without_advancing() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let e = g.create_node(KIND_EFFECT);
        g.begin_tracking(e); g.track_read(e); g.track_read(u32::MAX); g.track_read(a); g.end_tracking();
        assert_eq!(g.sources[e as usize],vec![a]);
    }

    #[test]
    fn cursor_cannot_mutate_a_replacement_observer_lifetime() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL); let old = g.create_node(KIND_EFFECT);
        g.begin_tracking(old); g.track_read(a); g.dispose_node(old); let fresh = g.create_node(KIND_EFFECT); assert_eq!(fresh,old);
        g.track_read(b); g.end_tracking();
        assert!(g.sources[fresh as usize].is_empty()); assert_eq!(g.get_state(fresh),STATE_DIRTY);
    }

    #[test]
    fn cursor_stale_inner_frame_preserves_the_outer_tracker() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL);
        let outer = g.create_node(KIND_EFFECT); let inner = g.create_node(KIND_COMPUTED);
        g.begin_tracking(outer); g.track_read(a); g.begin_tracking(inner); g.track_read(a);
        g.dispose_node(inner); let fresh = g.create_node(KIND_COMPUTED); assert_eq!(fresh,inner);
        g.track_read(b); g.end_tracking(); g.track_read(b); g.end_tracking();
        assert_eq!(g.sources[outer as usize],vec![a,b]); assert!(g.sources[fresh as usize].is_empty());
        assert!(g.tracking_stack.is_empty()); assert_eq!(g.get_state(fresh),STATE_DIRTY);
    }

    #[test]
    fn cursor_lifetime_wrap_invalidates_ancient_tracking_frames() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let old = g.create_node(KIND_EFFECT);
        g.begin_tracking(old); g.nodes[old as usize].lifetime = u32::MAX; g.dispose_node(old);
        let fresh = g.create_node(KIND_EFFECT); assert_eq!(fresh,old); assert_eq!(g.nodes[fresh as usize].lifetime,1);
        g.track_read(a); g.end_tracking(); assert!(g.sources[fresh as usize].is_empty()); assert_eq!(g.get_state(fresh),STATE_DIRTY);
    }

    #[test]
    fn cursor_source_disposal_does_not_turn_an_unread_tail_into_a_read_prefix() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL);
        let c = g.create_node(KIND_SIGNAL); let e = g.create_node(KIND_EFFECT);
        g.sync_sources(e,&[a,b]); g.begin_tracking(e); g.track_read(a); g.dispose_node(a); g.track_read(c); g.end_tracking();
        assert_eq!(g.sources[e as usize],vec![c]); assert!(!g.subscribers[b as usize].contains(&e));
    }

    #[test]
    fn cursor_explicit_source_replacement_restarts_the_discovery_cursor() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL);
        let c = g.create_node(KIND_SIGNAL); let d = g.create_node(KIND_SIGNAL); let e = g.create_node(KIND_EFFECT);
        g.begin_tracking(e); g.track_read(a); g.sync_sources(e,&[c,d]); g.track_read(b); g.end_tracking();
        assert_eq!(g.sources[e as usize],vec![b]);
    }

    #[test]
    fn cursor_pause_and_invalid_begin_preserve_outer_tracking() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL);
        let c = g.create_node(KIND_SIGNAL); let e = g.create_node(KIND_EFFECT);
        g.begin_tracking(e); g.track_read(a); g.pause_tracking(); g.track_read(b); g.resume_tracking();
        g.begin_tracking(u32::MAX); g.track_read(b); g.end_tracking();
        g.begin_tracking(a); g.track_read(b); g.end_tracking(); g.track_read(c); g.end_tracking();
        assert_eq!(g.sources[e as usize],vec![a,c]); assert!(!g.subscribers[b as usize].contains(&e));
        assert!(g.tracking_stack.is_empty());
    }

    #[test]
    fn cursor_stable_and_reordered_reads_reuse_storage_and_topology_cache() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL); let e = g.create_node(KIND_EFFECT);
        g.begin_tracking(e); g.track_read(a); g.track_read(b); g.end_tracking();
        assert_eq!(g.trigger_and_flush_fast(a),(e as i32)+1); g.set_state(e,STATE_CLEAN);
        let sources = g.sources[e as usize].as_ptr(); let subs_a = g.subscribers[a as usize].as_ptr(); let subs_b = g.subscribers[b as usize].as_ptr();
        let stack = g.tracking_stack.as_ptr(); let epoch = g.topology_epoch;
        for i in 0..100 {
            g.begin_tracking(e);
            if i % 2 == 0 { g.track_read(b); g.track_read(a); } else { g.track_read(a); g.track_read(b); }
            g.end_tracking();
        }
        assert_eq!(g.sources[e as usize].as_ptr(),sources); assert_eq!(g.subscribers[a as usize].as_ptr(),subs_a); assert_eq!(g.subscribers[b as usize].as_ptr(),subs_b);
        assert_eq!(g.tracking_stack.as_ptr(),stack); assert_eq!(g.topology_epoch,epoch);
        assert_eq!(g.terminal_cache_epoch[a as usize],epoch); assert_eq!(g.trigger_and_flush_fast(a),(e as i32)+1);
    }

    #[test]
    fn cursor_recursive_begin_does_not_replace_the_outer_dependency_pass() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL);
        let c = g.create_node(KIND_SIGNAL); let e = g.create_node(KIND_EFFECT);
        g.begin_tracking(e); g.track_read(a); g.begin_tracking(e); g.track_read(b); g.end_tracking();
        g.track_read(c); g.end_tracking(); assert_eq!(g.sources[e as usize],vec![a,c]);
        assert!(!g.subscribers[b as usize].contains(&e)); assert!(g.tracking_stack.is_empty());
    }

    #[test]
    fn cursor_fresh_lifetime_can_track_before_the_stale_frame_is_closed() {
        let mut g = ReactiveGraph::new(); let a = g.create_node(KIND_SIGNAL); let b = g.create_node(KIND_SIGNAL); let old = g.create_node(KIND_EFFECT);
        g.begin_tracking(old); g.track_read(a); g.dispose_node(old); let fresh = g.create_node(KIND_EFFECT); assert_eq!(fresh,old);
        g.begin_tracking(fresh); g.track_read(b); g.end_tracking(); g.track_read(a); g.end_tracking();
        assert_eq!(g.sources[fresh as usize],vec![b]); assert_eq!(g.get_state(fresh),STATE_CLEAN);
        assert_eq!(g.trigger_and_flush_fast(a),0); assert_eq!(g.trigger_and_flush_fast(b),(fresh as i32)+1);
    }

}
