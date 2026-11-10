use crate::graph::ReactiveGraph;

pub(crate) fn perspective_matrix(
    fov_y_rad: f32,
    aspect: f32,
    near: f32,
    far: f32,
) -> Option<[f32; 16]> {
    if ![fov_y_rad, aspect, near, far].iter().all(|v| v.is_finite())
        || fov_y_rad <= 0.0
        || fov_y_rad >= std::f32::consts::PI
        || aspect <= 0.0
        || near <= 0.0
        || far <= near
    {
        return None;
    }
    let f = 1.0 / (0.5 * fov_y_rad).tan();
    let z = far / (near - far);
    // Avoid the overflowing near*far intermediate of near*far/(near-far).
    let m = [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        z,
        -1.0,
        0.0,
        0.0,
        near * z,
        0.0,
    ];
    if m.iter().all(|v| v.is_finite()) && m[0] > 0.0 && m[5] > 0.0 && m[14] < 0.0 {
        Some(m)
    } else {
        None
    }
}

/// Maximum per-context GPU arena storage: 256 MiB of float32 values.
pub const MAX_GPU_ARENA_FLOATS: u32 = 67_108_864;

/// Grow the arena within a caller-supplied element limit (small limits aid tests).
pub(crate) fn grow_gpu_arena(arena: &mut Vec<f32>, new_len: usize, limit: usize) -> bool {
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

impl ReactiveGraph {
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
        if !grow_gpu_arena(
            &mut self.gpu_f32_arena,
            new_len,
            MAX_GPU_ARENA_FLOATS as usize,
        ) {
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
        let end = offset
            .saturating_add(len)
            .min(self.gpu_f32_arena.len() as u32);
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
        if arena_len == 0 {
            return 0;
        }
        let total_blocks = (arena_len + 31) / 32;
        let start_block = (self.gpu_dirty_min / 32).min(total_blocks.saturating_sub(1));
        let end_block = ((self.gpu_dirty_max - 1) / 32).min(total_blocks.saturating_sub(1));

        let mut in_span = false;
        let mut span_start = 0u32;
        let mut b = start_block;

        while b <= end_block {
            let word_idx = (b / 32) as usize;
            let bit = b % 32;
            let word = self.gpu_dirty_blocks.get(word_idx).copied().unwrap_or(0);

            if bit == 0 && b + 31 <= end_block {
                if word == 0 {
                    if in_span {
                        in_span = false;
                        let span_end = (b * 32).min(arena_len);
                        self.lis_buffer.push(span_start);
                        self.lis_buffer.push(span_end - span_start);
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
                self.lis_buffer.push(span_start);
                self.lis_buffer.push(span_end - span_start);
            }
            b += 1;
        }

        if in_span {
            let span_end = ((end_block + 1) * 32).min(arena_len);
            self.lis_buffer.push(span_start);
            self.lis_buffer.push(span_end - span_start);
        }

        self.lis_buffer.len() / 2
    }

    /// Clears the dirty range tracker after a UMA/DUMA frame flush.
    #[inline]
    pub fn gpu_clear_dirty(&mut self) {
        if self.gpu_dirty_min < self.gpu_dirty_max && self.gpu_dirty_min != u32::MAX {
            let start_word = (self.gpu_dirty_min / 1024) as usize;
            let end_word = ((self.gpu_dirty_max - 1) / 1024) as usize;
            let len = self.gpu_dirty_blocks.len();
            if start_word < len {
                for w in &mut self.gpu_dirty_blocks[start_word..=end_word.min(len - 1)] {
                    *w = 0;
                }
            }
        }
        self.gpu_dirty_min = u32::MAX;
        self.gpu_dirty_max = 0;
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
        if d.checked_add(16)
            .map_or(true, |end| end > self.gpu_f32_arena.len())
        {
            return;
        }
        let Some(m) = perspective_matrix(fov_y_rad, aspect, near, far) else {
            return;
        };
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
        if d.checked_add(16)
            .map_or(true, |end| end > self.gpu_f32_arena.len())
        {
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
            m00, m01, m02, 0.0, m10, m11, m12, 0.0, m20, m21, m22, 0.0, tx, ty, tz, 1.0,
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
            || v.checked_add(total_floats)
                .map_or(true, |end| end > arena_len)
            || d.checked_add(total_floats)
                .map_or(true, |end| end > arena_len)
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

                self.gpu_f32_arena[di] = mat[0] * x + mat[4] * y + mat[8] * z + mat[12] * w;
                self.gpu_f32_arena[di + 1] = mat[1] * x + mat[5] * y + mat[9] * z + mat[13] * w;
                self.gpu_f32_arena[di + 2] = mat[2] * x + mat[6] * y + mat[10] * z + mat[14] * w;
                self.gpu_f32_arena[di + 3] = mat[3] * x + mat[7] * y + mat[11] * z + mat[15] * w;
            }
        }

        self.gpu_mark_dirty(dst_offset, vertex_count.saturating_mul(4));
    }

    /// Clip a segment or convex polygon in homogeneous WebGPU coordinates.
    pub fn gpu_clip_primitive_3d(&mut self, offset: u32, count: u32, indices: &[u32]) -> u32 {
        self.gpu_clip_output.clear();
        if !(2..=4).contains(&indices.len()) || indices.iter().any(|&i| i >= count) {
            return 0;
        }
        let end = (count as usize)
            .checked_mul(4)
            .and_then(|total| (offset as usize).checked_add(total));
        if end.map_or(true, |end| end > self.gpu_f32_arena.len()) {
            return 0;
        }
        let mut buf_a = [[0.0f64; 4]; 12];
        let mut len_a = indices.len();
        for (slot, &index) in indices.iter().enumerate() {
            let base = offset as usize + index as usize * 4;
            let v = std::array::from_fn(|axis| self.gpu_f32_arena[base + axis] as f64);
            if v.iter().any(|value| !value.is_finite()) {
                return 0;
            }
            buf_a[slot] = v;
        }
        let distance = |v: [f64; 4], plane: usize| match plane {
            0 => v[3] + v[0],
            1 => v[3] - v[0],
            2 => v[3] + v[1],
            3 => v[3] - v[1],
            4 => v[2],
            _ => v[3] - v[2],
        };
        let interpolate = |a: [f64; 4], b: [f64; 4], t: f64| {
            std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t)
        };
        if len_a == 2 {
            let a = buf_a[0];
            let b = buf_a[1];
            let mut enter: f64 = 0.0;
            let mut exit: f64 = 1.0;
            for plane in 0..6 {
                let da = distance(a, plane);
                let db = distance(b, plane);
                if da < 0.0 && db < 0.0 {
                    return 0;
                }
                if da < 0.0 {
                    enter = enter.max(da / (da - db));
                }
                if db < 0.0 {
                    exit = exit.min(da / (da - db));
                }
            }
            if enter > exit {
                return 0;
            }
            buf_a[0] = interpolate(a, b, enter);
            buf_a[1] = interpolate(a, b, exit);
        } else {
            let mut buf_b = [[0.0f64; 4]; 12];
            for plane in 0..6 {
                if len_a == 0 {
                    break;
                }
                let mut len_b = 0usize;
                let mut previous = buf_a[len_a - 1];
                let mut dp = distance(previous, plane);
                for &current in &buf_a[..len_a] {
                    let dc = distance(current, plane);
                    if (dp >= 0.0) != (dc >= 0.0) && len_b < buf_b.len() {
                        buf_b[len_b] = interpolate(previous, current, dp / (dp - dc));
                        len_b += 1;
                    }
                    if dc >= 0.0 && len_b < buf_b.len() {
                        buf_b[len_b] = current;
                        len_b += 1;
                    }
                    previous = current;
                    dp = dc;
                }
                buf_a[..len_b].copy_from_slice(&buf_b[..len_b]);
                len_a = len_b;
            }
        }
        if buf_a[..len_a].iter().any(|v| v[3] <= 0.0) {
            return 0;
        }
        for vertex in &buf_a[..len_a] {
            self.gpu_clip_output
                .extend(vertex.map(|value| value as f32));
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
        self.gpu_pick_vertex_impl(
            clip_verts_offset,
            vertex_count,
            viewport_w,
            viewport_h,
            click_x,
            click_y,
            max_radius_px,
            &[],
        )
    }

    pub fn gpu_pick_snapshot_3d(
        &mut self,
        count: u32,
        indices: u32,
        occlude: u32,
        vw: f32,
        vh: f32,
        x: f32,
        y: f32,
        radius: f32,
    ) -> i32 {
        if (count as usize)
            .checked_mul(4)
            .map_or(true, |len| len > self.gpu_pick_vertices.len())
        {
            return -1;
        }
        // Scratch geometry is isolated from the live tensor arena and graph nodes.
        let mut scratch = ReactiveGraph::new();
        scratch.gpu_f32_arena = std::mem::take(&mut self.gpu_pick_vertices);
        scratch.gpu_pick_indices = std::mem::take(&mut self.gpu_pick_indices);
        let result = if occlude != 0 && indices != 0 {
            scratch.gpu_pick_visible_vertex_3d(0, count, indices, vw, vh, x, y, radius)
        } else {
            scratch.gpu_pick_nearest_vertex_3d(0, count, vw, vh, x, y, radius)
        };
        self.gpu_pick_vertices = std::mem::take(&mut scratch.gpu_f32_arena);
        self.gpu_pick_indices = std::mem::take(&mut scratch.gpu_pick_indices);
        result
    }

    pub fn gpu_pick_visible_vertex_3d(
        &mut self,
        offset: u32,
        count: u32,
        index_count: u32,
        vw: f32,
        vh: f32,
        x: f32,
        y: f32,
        radius: f32,
    ) -> i32 {
        let len = index_count as usize;
        if len > self.gpu_pick_indices.len() || len % 3 != 0 {
            return -1;
        }
        let mut triangles = Vec::new();
        for base in (0..len).step_by(3) {
            let indices = [
                self.gpu_pick_indices[base],
                self.gpu_pick_indices[base + 1],
                self.gpu_pick_indices[base + 2],
            ];
            let size = self.gpu_clip_primitive_3d(offset, count, &indices) as usize;
            if size < 3 {
                continue;
            }
            let mut points = [[0.0f64; 3]; 12];
            for (slot, v) in self.gpu_clip_output.chunks_exact(4).take(12).enumerate() {
                let w = v[3] as f64;
                points[slot] = [v[0] as f64 / w, v[1] as f64 / w, v[2] as f64 / w];
            }
            for i in 1..size.min(12) - 1 {
                triangles.push([points[0], points[i], points[i + 1]]);
            }
        }
        self.gpu_pick_vertex_impl(offset, count, vw, vh, x, y, radius, &triangles)
    }

    fn gpu_pick_vertex_impl(
        &self,
        clip_verts_offset: u32,
        vertex_count: u32,
        viewport_w: f32,
        viewport_h: f32,
        click_x: f32,
        click_y: f32,
        max_radius_px: f32,
        triangles: &[[[f64; 3]; 3]],
    ) -> i32 {
        if !viewport_w.is_finite()
            || !viewport_h.is_finite()
            || viewport_w <= 0.0
            || viewport_h <= 0.0
            || !click_x.is_finite()
            || !click_y.is_finite()
            || !max_radius_px.is_finite()
            || max_radius_px < 0.0
        {
            return -1;
        }
        let base = clip_verts_offset as usize;
        let total = match (vertex_count as usize).checked_mul(4) {
            Some(t) => t,
            None => return -1,
        };
        if base
            .checked_add(total)
            .map_or(true, |end| end > self.gpu_f32_arena.len())
        {
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

            if !x.is_finite()
                || !y.is_finite()
                || !z.is_finite()
                || !w.is_finite()
                || w <= 0.0
                || x < -w
                || x > w
                || y < -w
                || y > w
                || z < 0.0
                || z > w
            {
                continue;
            }
            let sx = half_w + (x / w) * scale_x;
            let sy = half_h - (y / w) * scale_y;
            let ndc_z = z / w;

            let dx = sx - click_x;
            let dy = sy - click_y;
            let dist_sq = dx * dx + dy * dy;

            if dist_sq <= max_dist_sq {
                let px = x as f64 / w as f64;
                let py = y as f64 / w as f64;
                let hidden = triangles.iter().any(|[a, b, c]| {
                    let denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                    if denominator.abs() < 1e-14 {
                        return false;
                    }
                    let u =
                        ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / denominator;
                    let v =
                        ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / denominator;
                    let t = 1.0 - u - v;
                    u >= -1e-9
                        && v >= -1e-9
                        && t >= -1e-9
                        && u * a[2] + v * b[2] + t * c[2] + 1e-6 < z as f64 / w as f64
                });
                if hidden {
                    continue;
                }
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
}
