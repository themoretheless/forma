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
    pub alive: bool,
    pub epoch: u32,
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
    /// Tracking stack for automatic dependency discovery (`computed` / `effect` evaluation)
    pub tracking_stack: Vec<(u32, usize)>,
    /// Queued effect IDs that need to be run on flush
    pub pending_effects: Vec<u32>,
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
    /// Contiguous 16-byte-aligned f32 page for UMA/DUMA GPU Tensor Signals
    pub gpu_f32_arena: Vec<f32>,
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
            gpu_f32_arena: Vec::new(),
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
        self.gpu_f32_arena.clear();
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
            None => return 0,
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
            Some(l) if l <= 67_108_864 => l, // Cap arena to 256 MB (64M floats) to prevent OOM DoS
            _ => return 0,
        };
        self.gpu_f32_arena.resize(new_len, 0.0);

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
        let f = 1.0 / (0.5 * fov_y_rad).tan();
        let nf = 1.0 / (near - far);

        let m = [
            f / aspect, 0.0, 0.0, 0.0,
            0.0, f, 0.0, 0.0,
            0.0, 0.0, far * nf, -1.0,
            0.0, 0.0, near * far * nf, 0.0,
        ];
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
            self.nodes[idx] = Node {
                kind,
                in_queue: false,
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
            kind,
            in_queue: false,
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

    /// Disposes a node completely, severing both incoming and outgoing edges
    /// and returning its ID to `free_list` for O(1) slot recycling.
    #[inline]
    pub fn dispose_node(&mut self, id: u32) {
        let idx = id as usize;
        if idx >= self.nodes.len() || !self.nodes[idx].alive {
            return;
        }
        self.topology_epoch = self.topology_epoch.wrapping_add(1);

        // 1. Sever incoming edges (sources -> this node)
        self.clear_sources(id);

        // 2. Sever outgoing edges (this node -> subscribers)
        let subs = std::mem::take(&mut self.subscribers[idx]);
        for sub_id in subs {
            if let Some(sub_sources) = self.sources.get_mut(sub_id as usize) {
                if let Some(pos) = sub_sources.iter().position(|&x| x == id) {
                    sub_sources.swap_remove(pos);
                }
            }
        }

        // 3. Mark dead and push to free_list
        let node = &mut self.nodes[idx];
        node.alive = false;
        node.in_queue = false;
        self.node_states[idx] = STATE_DISPOSED;
        if self.active_count > 0 {
            self.active_count -= 1;
        }
        self.free_list.push(id);
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
            self.topology_epoch = self.topology_epoch.wrapping_add(1);
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
        if !self.sources[t].is_empty() {
            self.topology_epoch = self.topology_epoch.wrapping_add(1);
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
        self.tracking_stack.push((target, 0));
    }

    /// Pauses dependency tracking (`untrack`) by pushing `u32::MAX` sentinel onto `tracking_stack`.
    /// Unlike creating a dummy node, this takes 0 allocations!
    #[inline]
    pub fn pause_tracking(&mut self) {
        self.tracking_stack.push((u32::MAX, 0));
    }

    /// Resumes dependency tracking after `pause_tracking`.
    #[inline]
    pub fn resume_tracking(&mut self) {
        if let Some(&(top, _)) = self.tracking_stack.last() {
            if top == u32::MAX {
                self.tracking_stack.pop();
            }
        }
    }

    /// Finish tracking dependencies for current observer, prune any unused trailing sources, and mark it clean.
    #[inline]
    pub fn end_tracking(&mut self) {
        if let Some((target, cursor)) = self.tracking_stack.pop() {
            if target != u32::MAX {
                let t = target as usize;
                if t < self.sources.len() {
                    let old_len = self.sources[t].len();
                    if cursor < old_len {
                        for i in cursor..old_len {
                            let stale_src = self.sources[t][i] as usize;
                            if let Some(subs) = self.subscribers.get_mut(stale_src) {
                                if let Some(pos) = subs.iter().position(|&x| x == target) {
                                    subs.swap_remove(pos);
                                }
                            }
                        }
                        self.sources[t].truncate(cursor);
                    }
                }
                if let Some(node) = self.nodes.get(t) {
                    if node.alive {
                        self.node_states[t] = STATE_CLEAN;
                    }
                }
            }
        }
    }

    /// Record a read of `source`. If an observer is currently tracking on the stack
    /// (and tracking is not paused with `u32::MAX`), verifies or creates edge `source -> observer` in O(1).
    #[inline]
    pub fn track_read(&mut self, source: u32) {
        if let Some((observer, cursor)) = self.tracking_stack.last_mut() {
            let obs = *observer;
            if obs != u32::MAX {
                let t = obs as usize;
                let cur = *cursor;
                if t < self.sources.len() {
                    if cur < self.sources[t].len() && self.sources[t][cur] == source {
                        // Fast path: exact same dependency at exact same slot! 0 vector work!
                        *cursor = cur + 1;
                        return;
                    }
                    // Check if `source` was already recorded earlier in this tracking pass
                    if self.sources[t][..cur].contains(&source) {
                        return;
                    }
                    // If the slot at `cur` held a different old dependency, remove `obs` from its subscribers
                    if cur < self.sources[t].len() {
                        let replaced_src = self.sources[t][cur] as usize;
                        if let Some(subs) = self.subscribers.get_mut(replaced_src) {
                            if let Some(pos) = subs.iter().position(|&x| x == obs) {
                                subs.swap_remove(pos);
                            }
                        }
                        self.sources[t][cur] = source;
                        let s = source as usize;
                        if s < self.subscribers.len() && !self.subscribers[s].contains(&obs) {
                            self.subscribers[s].push(obs);
                        }
                        *cursor = cur + 1;
                        return;
                    }
                }
                *cursor = cur + 1;
                self.link(source, obs);
            }
        }
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
                    self.pending_effects.push(curr_id);
                }
            } else if node.kind == KIND_COMPUTED {
                for &sub in &self.subscribers[idx] {
                    self.propagate_stack.push((sub, STATE_CHECK));
                }
            }
        }
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

    /// Drains all scheduled alive effects into `flush_buffer` so JS can read them in one zero-copy slice.
    pub fn prepare_flush(&mut self) -> usize {
        self.flush_buffer.clear();
        if self.batch_depth > 0 {
            return 0;
        }
        for &eff_id in &self.pending_effects {
            if let Some(node) = self.nodes.get_mut(eff_id as usize) {
                node.in_queue = false;
                if node.alive {
                    self.flush_buffer.push(eff_id);
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
        if let Some(&(top, _)) = g.tracking_stack.last() {
            if top != u32::MAX {
                return 1;
            }
        }
        0
    })
}

// ============================================================================
// GPU + UMA + DUMA C-ABI Exports
// ============================================================================

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
}
