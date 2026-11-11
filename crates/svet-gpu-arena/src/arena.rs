use crate::geometry::{
    ViewportPickQuery, clip_primitive_3d, pick_nearest_vertex_3d, pick_visible_vertex_3d,
};
use crate::math::{compose_trs_matrix, mat4_mul_in_place, perspective_matrix, vector_fma_in_place};

/// Maximum per-context GPU arena storage: 256 MiB of `f32` values.
pub const MAX_GPU_ARENA_FLOATS: u32 = 67_108_864;

#[inline]
pub(crate) const fn align4(len: u32) -> Option<u32> {
    match len.checked_add(3) {
        Some(v) => Some(v & !3),
        None => None,
    }
}

#[inline]
pub(crate) fn checked_range(offset: u32, len: usize, arena_len: usize) -> Option<(usize, usize)> {
    let start = offset as usize;
    let end = start.checked_add(len)?;
    (end <= arena_len).then_some((start, end))
}

/// Grow the arena within a caller-supplied element limit (small limits aid tests).
pub fn grow_gpu_arena(arena: &mut Vec<f32>, new_len: usize, limit: usize) -> bool {
    if new_len > limit || new_len < arena.len() || arena.capacity() > limit {
        return false;
    }
    if new_len > arena.capacity() {
        let capacity = arena.capacity().saturating_mul(2).max(new_len).min(limit);
        if arena.try_reserve_exact(capacity - arena.len()).is_err() {
            return false;
        }
    }
    arena.resize(new_len, 0.0);
    true
}

/// Linear-memory UMA/DUMA `f32` tensor arena with coalescing block recycling,
/// 128-byte cache-line dirty-span tracking, and scratch buffers for 3D clipping/picking.
#[derive(Debug)]
pub struct GpuArena {
    /// Contiguous 16-byte-aligned `f32` page for UMA/DUMA GPU Tensor Signals.
    pub f32_arena: Vec<f32>,
    /// Homogeneous clipping result, valid until the next clipping call.
    pub clip_output: Vec<f32>,
    pub pick_indices: Vec<u32>,
    pub pick_vertices: Vec<f32>,
    /// Dirty bitset (1 bit per 32 floats / 128-byte cache line) for partial DUMA DMA transfers.
    pub dirty_blocks: Vec<u32>,
    /// Free blocks `(offset, aligned_len)` in `f32_arena` for O(1) GPU tensor memory recycling.
    pub free_blocks: Vec<(u32, u32)>,
    /// Minimum dirty float index in the current frame.
    pub dirty_min: u32,
    /// Maximum dirty float index (exclusive) in the current frame.
    pub dirty_max: u32,
}

impl Default for GpuArena {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuArena {
    pub const fn new() -> Self {
        Self {
            f32_arena: Vec::new(),
            clip_output: Vec::new(),
            pick_indices: Vec::new(),
            pick_vertices: Vec::new(),
            dirty_blocks: Vec::new(),
            free_blocks: Vec::new(),
            dirty_min: u32::MAX,
            dirty_max: 0,
        }
    }

    pub fn reset(&mut self) {
        self.f32_arena.clear();
        self.clip_output.clear();
        self.pick_indices.clear();
        self.pick_vertices.clear();
        self.dirty_blocks.clear();
        self.free_blocks.clear();
        self.dirty_min = u32::MAX;
        self.dirty_max = 0;
    }

    #[inline]
    fn arena_range(&self, offset: u32, len: usize) -> Option<(usize, usize)> {
        checked_range(offset, len, self.f32_arena.len())
    }

    /// Allocates a 4-float (16-byte WebGPU uniform/storage aligned) contiguous slice in the UMA/DUMA arena.
    /// Reuses freed blocks from `free_blocks` when available.
    /// Returns the starting float index (`offset`), or `u32::MAX` on overflow / cap exhaustion.
    pub fn alloc_f32(&mut self, len: u32) -> u32 {
        let Some(aligned_len) = align4(len) else {
            return u32::MAX;
        };
        if aligned_len == 0 {
            return self.f32_arena.len() as u32;
        }

        // 1. Exact or best-fit reuse from `free_blocks` (with automatic block splitting).
        let mut best_idx: Option<usize> = None;
        let mut best_len = u32::MAX;
        for (idx, &(_, block_len)) in self.free_blocks.iter().enumerate() {
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
            let (reused_offset, block_len) = self.free_blocks.swap_remove(pos);
            if block_len > aligned_len {
                self.free_blocks
                    .push((reused_offset + aligned_len, block_len - aligned_len));
            }
            if let Some((start_idx, end_idx)) = self.arena_range(reused_offset, aligned_len as usize)
            {
                self.f32_arena[start_idx..end_idx].fill(0.0);
            }
            self.mark_dirty(reused_offset, len);
            return reused_offset;
        }

        let start = self.f32_arena.len() as u32;
        let new_len = match (start as usize).checked_add(aligned_len as usize) {
            Some(l) if l <= MAX_GPU_ARENA_FLOATS as usize => l,
            _ => return u32::MAX,
        };
        if !grow_gpu_arena(&mut self.f32_arena, new_len, MAX_GPU_ARENA_FLOATS as usize) {
            return u32::MAX;
        }

        let required_blocks = self.f32_arena.len().div_ceil(32).div_ceil(32);
        if self.dirty_blocks.len() < required_blocks {
            self.dirty_blocks.resize(required_blocks, 0);
        }
        self.mark_dirty(start, len);
        start
    }

    /// Returns a previously allocated tensor slice `[offset, offset + aligned_len)` to `free_blocks`
    /// and coalesces any adjacent left/right free blocks to eliminate arena fragmentation.
    pub fn free_f32(&mut self, offset: u32, len: u32) {
        let Some(aligned_len) = align4(len) else {
            return;
        };
        if aligned_len == 0 {
            return;
        }
        let Some((_, end)) = self.arena_range(offset, aligned_len as usize) else {
            return;
        };

        // Ignore duplicate double-free overlapping an existing free block.
        if self.free_blocks.iter().any(|&(b_off, b_len)| {
            let b_end = b_off.saturating_add(b_len);
            offset < b_end && (end as u32) > b_off
        }) {
            return;
        }

        let mut merged_start = offset;
        let mut merged_len = aligned_len;

        if let Some(left_pos) = self
            .free_blocks
            .iter()
            .position(|&(b_off, b_len)| b_off.saturating_add(b_len) == merged_start)
        {
            let (left_off, left_len) = self.free_blocks.swap_remove(left_pos);
            merged_start = left_off;
            merged_len = merged_len.saturating_add(left_len);
        }

        if let Some(right_pos) = self
            .free_blocks
            .iter()
            .position(|&(b_off, _)| merged_start.saturating_add(merged_len) == b_off)
        {
            let (_, right_len) = self.free_blocks.swap_remove(right_pos);
            merged_len = merged_len.saturating_add(right_len);
        }

        self.free_blocks.push((merged_start, merged_len));
    }

    /// Marks `[offset, offset + len)` in the GPU arena as dirty for DUMA partial DMA sync.
    #[inline]
    pub fn mark_dirty(&mut self, offset: u32, len: u32) {
        if len == 0 {
            return;
        }
        let end = offset.saturating_add(len).min(self.f32_arena.len() as u32);
        if offset >= end {
            return;
        }
        if offset < self.dirty_min {
            self.dirty_min = offset;
        }
        if end > self.dirty_max {
            self.dirty_max = end;
        }
        let start_block = (offset / 32) as usize;
        let end_block = ((end - 1) / 32) as usize;
        for b in start_block..=end_block {
            let word = b / 32;
            let bit = b % 32;
            if let Some(w) = self.dirty_blocks.get_mut(word) {
                *w |= 1u32 << bit;
            }
        }
    }

    /// Collects contiguous dirty 32-float (128-byte cache-line) spans from `dirty_blocks`
    /// into `out` as pairs `[start_float_0, len_float_0, ...]`, returning the number of spans.
    pub fn collect_dirty_spans_into(&self, out: &mut Vec<u32>) -> usize {
        out.clear();
        if self.dirty_min >= self.dirty_max || self.dirty_min == u32::MAX {
            return 0;
        }
        let arena_len = self.f32_arena.len() as u32;
        if arena_len == 0 {
            return 0;
        }
        let total_blocks = arena_len.div_ceil(32);
        let start_block = (self.dirty_min / 32).min(total_blocks.saturating_sub(1));
        let end_block = ((self.dirty_max - 1) / 32).min(total_blocks.saturating_sub(1));

        let mut in_span = false;
        let mut span_start = 0u32;
        let mut b = start_block;

        while b <= end_block {
            let word_idx = (b / 32) as usize;
            let bit = b % 32;
            let word = self.dirty_blocks.get(word_idx).copied().unwrap_or(0);

            if bit == 0 && b + 31 <= end_block {
                if word == 0 {
                    if in_span {
                        in_span = false;
                        let span_end = (b * 32).min(arena_len);
                        out.push(span_start);
                        out.push(span_end - span_start);
                    }
                    b += 32;
                    continue;
                } else if word == u32::MAX {
                    if !in_span {
                        in_span = true;
                        span_start = b * 32;
                    }
                    b += 32;
                    continue;
                }
            }

            let is_dirty = (word & (1u32 << bit)) != 0;
            if is_dirty && !in_span {
                in_span = true;
                span_start = b * 32;
            } else if !is_dirty && in_span {
                in_span = false;
                let span_end = (b * 32).min(arena_len);
                out.push(span_start);
                out.push(span_end - span_start);
            }
            b += 1;
        }

        if in_span {
            let span_end = ((end_block + 1) * 32).min(arena_len);
            out.push(span_start);
            out.push(span_end - span_start);
        }

        out.len() / 2
    }

    /// Clears the dirty range tracker after a UMA/DUMA frame flush.
    #[inline]
    pub fn clear_dirty(&mut self) {
        if self.dirty_min < self.dirty_max && self.dirty_min != u32::MAX {
            let start_word = (self.dirty_min / 1024) as usize;
            let end_word = ((self.dirty_max - 1) / 1024) as usize;
            let len = self.dirty_blocks.len();
            if start_word < len {
                self.dirty_blocks[start_word..=end_word.min(len - 1)].fill(0);
            }
        }
        self.dirty_min = u32::MAX;
        self.dirty_max = 0;
    }

    pub fn vector_fma(&mut self, src_offset: u32, dst_offset: u32, len: u32, scale: f32, bias: f32) {
        let l = len as usize;
        let Some((s, _)) = self.arena_range(src_offset, l) else {
            return;
        };
        let Some((d, _)) = self.arena_range(dst_offset, l) else {
            return;
        };
        vector_fma_in_place(&mut self.f32_arena, s, d, l, scale, bias);
        self.mark_dirty(dst_offset, len);
    }

    pub fn mat4_mul(&mut self, a_offset: u32, b_offset: u32, dst_offset: u32) {
        let Some((a, _)) = self.arena_range(a_offset, 16) else {
            return;
        };
        let Some((b, _)) = self.arena_range(b_offset, 16) else {
            return;
        };
        let Some((d, _)) = self.arena_range(dst_offset, 16) else {
            return;
        };
        mat4_mul_in_place(&mut self.f32_arena, a, b, d);
        self.mark_dirty(dst_offset, 16);
    }

    pub fn mat4_perspective(
        &mut self,
        dst_offset: u32,
        fov_y_rad: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) {
        let Some((d, end)) = self.arena_range(dst_offset, 16) else {
            return;
        };
        let Some(m) = perspective_matrix(fov_y_rad, aspect, near, far) else {
            return;
        };
        self.f32_arena[d..end].copy_from_slice(&m);
        self.mark_dirty(dst_offset, 16);
    }

    pub fn mat4_compose_trs(&mut self, dst_offset: u32, trs: [f32; 9]) {
        let Some((d, end)) = self.arena_range(dst_offset, 16) else {
            return;
        };
        let m = compose_trs_matrix(
            [trs[0], trs[1], trs[2]],
            [trs[3], trs[4], trs[5]],
            [trs[6], trs[7], trs[8]],
        );
        self.f32_arena[d..end].copy_from_slice(&m);
        self.mark_dirty(dst_offset, 16);
    }

    pub fn transform_positions_vec4(
        &mut self,
        mat_offset: u32,
        verts_offset: u32,
        dst_offset: u32,
        vertex_count: u32,
    ) {
        let Some(total_floats) = (vertex_count as usize).checked_mul(4) else {
            return;
        };
        let Some((m, _)) = self.arena_range(mat_offset, 16) else {
            return;
        };
        let Some((v, _)) = self.arena_range(verts_offset, total_floats) else {
            return;
        };
        let Some((d, _)) = self.arena_range(dst_offset, total_floats) else {
            return;
        };
        crate::math::transform_positions_vec4_in_place(
            &mut self.f32_arena,
            m,
            v,
            d,
            vertex_count as usize,
        );
        self.mark_dirty(dst_offset, vertex_count.saturating_mul(4));
    }

    pub fn clip_primitive_3d(&mut self, offset: u32, count: u32, indices: &[u32]) -> u32 {
        clip_primitive_3d(&self.f32_arena, offset, count, indices, &mut self.clip_output)
    }

    pub fn pick_nearest_vertex_3d(
        &self,
        clip_verts_offset: u32,
        vertex_count: u32,
        query: ViewportPickQuery,
    ) -> i32 {
        pick_nearest_vertex_3d(&self.f32_arena, clip_verts_offset, vertex_count, query)
    }

    pub fn pick_visible_vertex_3d(
        &mut self,
        offset: u32,
        count: u32,
        index_count: u32,
        query: ViewportPickQuery,
    ) -> i32 {
        pick_visible_vertex_3d(
            &self.f32_arena,
            offset,
            count,
            &self.pick_indices,
            index_count,
            &mut self.clip_output,
            query,
        )
    }

    /// Runs nearest or triangle-occluded 3D picking against `self.pick_vertices` and
    /// `self.pick_indices` without touching `self.f32_arena` or allocating a dummy arena.
    pub fn pick_snapshot_3d(
        &mut self,
        count: u32,
        indices: u32,
        occlude: u32,
        query: ViewportPickQuery,
    ) -> i32 {
        if occlude != 0 && indices != 0 {
            pick_visible_vertex_3d(
                &self.pick_vertices,
                0,
                count,
                &self.pick_indices,
                indices,
                &mut self.clip_output,
                query,
            )
        } else {
            pick_nearest_vertex_3d(&self.pick_vertices, 0, count, query)
        }
    }
}
