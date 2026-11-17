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

/// Computes a 4x4 WebGPU column-major perspective projection matrix (`depth in [0, 1]`),
/// returning `None` if any parameter is non-finite, out of range, or overflows/underflows.
pub fn perspective_matrix(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> Option<[f32; 16]> {
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

/// Composes a 4x4 column-major Translation-Rotation-Scale (Euler XYZ radians) matrix.
pub fn compose_trs_matrix(translation: [f32; 3], rotation: [f32; 3], scale: [f32; 3]) -> [f32; 16] {
    let [tx, ty, tz] = translation;
    let [rx, ry, rz] = rotation;
    let [sx, sy, sz] = scale;

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

    [
        m00, m01, m02, 0.0, m10, m11, m12, 0.0, m20, m21, m22, 0.0, tx, ty, tz, 1.0,
    ]
}

pub(crate) fn vector_fma_in_place(
    arena: &mut [f32],
    src: usize,
    dst: usize,
    len: usize,
    scale: f32,
    bias: f32,
) {
    // Forward overlap (`src < dst && dst < src + len`) is sequential: index `i` reads a slot
    // written by an earlier iteration, so 4-wide SIMD loads would observe unmutated lanes.
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    if !(src < dst && dst < src.saturating_add(len)) {
        // SAFETY: Caller (`GpuArena::vector_fma`) verified both `src..src + len` and
        // `dst..dst + len` lie within `arena`. `v128_load` and `v128_store` allow unaligned
        // `f32` addresses on wasm32 SIMD128, and forward-overlapping ranges were excluded above.
        unsafe {
            use core::arch::wasm32::*;
            let v_scale = f32x4_splat(scale);
            let v_bias = f32x4_splat(bias);
            let base_ptr = arena.as_mut_ptr();
            let src_ptr = base_ptr.add(src);
            let dst_ptr = base_ptr.add(dst);

            let chunks = len / 4;
            for c in 0..chunks {
                let offset = c * 4;
                let v_src = v128_load(src_ptr.add(offset) as *const v128);
                let v_res = f32x4_add(f32x4_mul(v_src, v_scale), v_bias);
                v128_store(dst_ptr.add(offset) as *mut v128, v_res);
            }
            for i in (chunks * 4)..len {
                let val = *src_ptr.add(i);
                *dst_ptr.add(i) = val * scale + bias;
            }
        }
        return;
    }

    for i in 0..len {
        let val = arena[src + i];
        arena[dst + i] = val * scale + bias;
    }
}

pub(crate) fn mat4_mul_in_place(arena: &mut [f32], a: usize, b: usize, d: usize) {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    // SAFETY: Caller (`GpuArena::mat4_mul`) verified `a..a + 16`, `b..b + 16`, and `d..d + 16`
    // lie within `arena`. All inputs are loaded into registers/stack before writing to `d`, so
    // aliasing between `a`/`b` and `d` is sound, and `v128_load`/`v128_store` accept any `f32` alignment.
    unsafe {
        use core::arch::wasm32::*;
        let base_ptr = arena.as_mut_ptr();
        let a_ptr = base_ptr.add(a);
        let b_ptr = base_ptr.add(b);
        let d_ptr = base_ptr.add(d);

        let a_col0 = v128_load(a_ptr as *const v128);
        let a_col1 = v128_load(a_ptr.add(4) as *const v128);
        let a_col2 = v128_load(a_ptr.add(8) as *const v128);
        let a_col3 = v128_load(a_ptr.add(12) as *const v128);

        let mut out_cols = [f32x4_splat(0.0); 4];
        for (col, out_col) in out_cols.iter_mut().enumerate() {
            let bc = b_ptr.add(col * 4);
            let b0 = f32x4_splat(*bc);
            let b1 = f32x4_splat(*bc.add(1));
            let b2 = f32x4_splat(*bc.add(2));
            let b3 = f32x4_splat(*bc.add(3));

            let sum01 = f32x4_add(f32x4_mul(a_col0, b0), f32x4_mul(a_col1, b1));
            let sum23 = f32x4_add(f32x4_mul(a_col2, b2), f32x4_mul(a_col3, b3));
            *out_col = f32x4_add(sum01, sum23);
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
            let b0 = arena[b_col];
            let b1 = arena[b_col + 1];
            let b2 = arena[b_col + 2];
            let b3 = arena[b_col + 3];

            for row in 0..4 {
                out[col * 4 + row] = arena[a + row] * b0
                    + arena[a + 4 + row] * b1
                    + arena[a + 8 + row] * b2
                    + arena[a + 12 + row] * b3;
            }
        }
        arena[d..d + 16].copy_from_slice(&out);
    }
}

pub(crate) fn transform_positions_vec4_in_place(
    arena: &mut [f32],
    m: usize,
    v: usize,
    d: usize,
    vertex_count: usize,
) {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    if !(v < d && d < v.saturating_add(vertex_count.saturating_mul(4))) {
        // SAFETY: Caller (`GpuArena::transform_positions_vec4`) verified `m..m + 16`,
        // `v..v + vertex_count * 4`, and `d..d + vertex_count * 4` lie within `arena`.
        // `m` is preloaded into SIMD registers before any writes, forward-overlapping `v`/`d`
        // ranges fall back to the scalar path below, and `v128_load`/`v128_store` allow unaligned `f32` pointers.
        unsafe {
            use core::arch::wasm32::*;
            let base_ptr = arena.as_mut_ptr();
            let m_ptr = base_ptr.add(m);
            let v_ptr = base_ptr.add(v);
            let d_ptr = base_ptr.add(d);

            let m_col0 = v128_load(m_ptr as *const v128);
            let m_col1 = v128_load(m_ptr.add(4) as *const v128);
            let m_col2 = v128_load(m_ptr.add(8) as *const v128);
            let m_col3 = v128_load(m_ptr.add(12) as *const v128);

            for i in 0..vertex_count {
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
        return;
    }

    let mat: [f32; 16] = arena[m..m + 16].try_into().unwrap();
    for i in 0..vertex_count {
        let vi = v + i * 4;
        let di = d + i * 4;
        let x = arena[vi];
        let y = arena[vi + 1];
        let z = arena[vi + 2];
        let w = arena[vi + 3];

        arena[di] = mat[0] * x + mat[4] * y + mat[8] * z + mat[12] * w;
        arena[di + 1] = mat[1] * x + mat[5] * y + mat[9] * z + mat[13] * w;
        arena[di + 2] = mat[2] * x + mat[6] * y + mat[10] * z + mat[14] * w;
        arena[di + 3] = mat[3] * x + mat[7] * y + mat[11] * z + mat[15] * w;
    }
}
