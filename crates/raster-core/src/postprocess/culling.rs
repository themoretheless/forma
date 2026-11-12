//! Visibility statistics and the hierarchical depth pyramid.

/// Telemetry summary for frustum and backface triangle culling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CullingStats {
    pub total_triangles: usize,
    pub frustum_culled_triangles: usize,
    pub backface_culled_triangles: usize,
    pub surviving_triangles: usize,
}

impl CullingStats {
    /// Ratio of culled triangles (`0.0..=1.0`).
    #[inline]
    pub fn cull_ratio(&self) -> f32 {
        if self.total_triangles == 0 {
            0.0
        } else {
            let culled = self.frustum_culled_triangles + self.backface_culled_triangles;
            (culled as f32) / (self.total_triangles as f32)
        }
    }
}

/// Evaluate frustum and backface culling statistics for an indexed triangle mesh.
///
/// - `positions`: flat `[x, y, z, ...]` vertex coordinates (3 floats per vertex).
/// - `indices`: triangle vertex indices (3 `u32`s per triangle).
/// - `vp_col_major`: 4x4 column-major view-projection matrix (WebGPU `[0, 1]` depth clip convention).
/// - `eye`: camera eye position `[x, y, z]`.
pub fn evaluate_culling_stats(
    positions: &[f32],
    indices: &[u32],
    vp_col_major: &[f32; 16],
    eye: [f32; 3],
) -> CullingStats {
    let vertex_count = positions.len() / 3;
    let total_triangles = indices.len() / 3;
    let mut stats = CullingStats {
        total_triangles,
        ..Default::default()
    };

    let project = |p: [f32; 3]| -> [f32; 4] {
        [
            vp_col_major[0] * p[0]
                + vp_col_major[4] * p[1]
                + vp_col_major[8] * p[2]
                + vp_col_major[12],
            vp_col_major[1] * p[0]
                + vp_col_major[5] * p[1]
                + vp_col_major[9] * p[2]
                + vp_col_major[13],
            vp_col_major[2] * p[0]
                + vp_col_major[6] * p[1]
                + vp_col_major[10] * p[2]
                + vp_col_major[14],
            vp_col_major[3] * p[0]
                + vp_col_major[7] * p[1]
                + vp_col_major[11] * p[2]
                + vp_col_major[15],
        ]
    };

    for tri in indices.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if i0 >= vertex_count || i1 >= vertex_count || i2 >= vertex_count {
            stats.frustum_culled_triangles += 1;
            continue;
        }
        let p0 = [
            positions[i0 * 3],
            positions[i0 * 3 + 1],
            positions[i0 * 3 + 2],
        ];
        let p1 = [
            positions[i1 * 3],
            positions[i1 * 3 + 1],
            positions[i1 * 3 + 2],
        ];
        let p2 = [
            positions[i2 * 3],
            positions[i2 * 3 + 1],
            positions[i2 * 3 + 2],
        ];
        let c0 = project(p0);
        let c1 = project(p1);
        let c2 = project(p2);

        // Frustum outcode check: all 3 vertices outside any single clip plane
        let out_left = c0[0] < -c0[3] && c1[0] < -c1[3] && c2[0] < -c2[3];
        let out_right = c0[0] > c0[3] && c1[0] > c1[3] && c2[0] > c2[3];
        let out_bottom = c0[1] < -c0[3] && c1[1] < -c1[3] && c2[1] < -c2[3];
        let out_top = c0[1] > c0[3] && c1[1] > c1[3] && c2[1] > c2[3];
        let out_near = c0[2] < -c0[3] && c1[2] < -c1[3] && c2[2] < -c2[3];
        let out_far = c0[2] > c0[3] && c1[2] > c1[3] && c2[2] > c2[3];
        if out_left || out_right || out_bottom || out_top || out_near || out_far {
            stats.frustum_culled_triangles += 1;
            continue;
        }

        // World-space geometric normal and view vector backface check
        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let view_vec = [eye[0] - p0[0], eye[1] - p0[1], eye[2] - p0[2]];
        let facing = n[0] * view_vec[0] + n[1] * view_vec[1] + n[2] * view_vec[2];
        if facing <= 0.0 {
            stats.backface_culled_triangles += 1;
            continue;
        }
        stats.surviving_triangles += 1;
    }

    stats
}

/// Computes the next Hierarchical-Z (Hi-Z) conservative maximum-depth mipmap level
/// ($\lceil \text{width} / 2 \rceil \times \lceil \text{height} / 2 \rceil$) from `depth`
/// for GPU/CPU two-phase occlusion culling.
///
/// For odd dimensions, boundary texels clamp-extend into the $2 \times 2$ reduction footprint
/// (plus any trailing column/row when `width` or `height` is odd) so no depth sample is ever
/// underestimated.
pub fn build_hiz_mip_level_max(
    depth: &[f32],
    width: usize,
    height: usize,
) -> (Vec<f32>, usize, usize) {
    let total = width.saturating_mul(height);
    if width == 0 || height == 0 || depth.len() < total {
        return (Vec::new(), 0, 0);
    }
    let out_w = width.div_ceil(2);
    let out_h = height.div_ceil(2);
    let mut out = Vec::with_capacity(out_w * out_h);

    for oy in 0..out_h {
        let y0 = (oy * 2).min(height - 1);
        let y1 = (oy * 2 + 1).min(height - 1);
        let row0 = &depth[y0 * width..(y0 + 1) * width];
        let row1 = &depth[y1 * width..(y1 + 1) * width];

        for ox in 0..out_w {
            let x0 = (ox * 2).min(width - 1);
            let x1 = (ox * 2 + 1).min(width - 1);
            let d00 = row0[x0];
            let d10 = row0[x1];
            let d01 = row1[x0];
            let d11 = row1[x1];
            let max_d = d00.max(d10).max(d01.max(d11));
            out.push(max_d);
        }
    }

    (out, out_w, out_h)
}
