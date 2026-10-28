//! Screen-space post-processing kernels (SSAO ambient occlusion, Sobel edge
//! silhouette detection) and frustum/backface culling telemetry for `raster-core`.

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

/// Screen-space ambient occlusion (SSAO) estimation from a normalized depth buffer (`0.0..=1.0`,
/// where `1.0` is the far background clear value).
///
/// Returns one occlusion visibility factor in `[0.0, 1.0]` per pixel (`1.0` = fully unoccluded,
/// `< 1.0` = darkened crease/cavity).
pub fn compute_ssao_ao(
    depth: &[f32],
    width: u32,
    height: u32,
    radius_px: u32,
    bias: f32,
    intensity: f32,
) -> Vec<f32> {
    let w = width as usize;
    let h = height as usize;
    let total = w.saturating_mul(h);
    if depth.len() < total || w == 0 || h == 0 || radius_px == 0 {
        return vec![1.0; depth.len().max(total)];
    }

    // 8-tap radial spiral offsets in [-1, 1]^2
    const OFFSETS: [(f32, f32); 8] = [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (0.7071, 0.7071),
        (-0.7071, 0.7071),
        (0.7071, -0.7071),
        (-0.7071, -0.7071),
    ];

    let r = radius_px as f32;
    let range_limit = (bias * 32.0).max(0.05);
    let inv_range_limit = 1.0 / range_limit;
    let intensity_scale = (intensity.max(0.0)) * (1.0 / OFFSETS.len() as f32);
    // Precompute integer tap offsets (dx, dy) once per pass
    let tap_offsets: [(isize, isize); 8] = std::array::from_fn(|i| {
        (
            (OFFSETS[i].0 * r).round() as isize,
            (OFFSETS[i].1 * r).round() as isize,
        )
    });
    let max_x = (w - 1) as isize;
    let max_y = (h - 1) as isize;
    let mut ao = vec![1.0_f32; total];

    for y in 0..h {
        let row_off = y * w;
        let yi = y as isize;
        for x in 0..w {
            let center_d = depth[row_off + x];
            if !center_d.is_finite() || center_d >= 1.0 {
                continue;
            }
            let xi = x as isize;
            let mut occluded = 0.0_f32;
            for &(dx, dy) in &tap_offsets {
                let sx = (xi + dx).clamp(0, max_x) as usize;
                let sy = (yi + dy).clamp(0, max_y) as usize;
                let sample_d = depth[sy * w + sx];
                let diff = center_d - sample_d;
                if diff > bias && diff < range_limit {
                    occluded = diff.mul_add(-inv_range_limit, 1.0) + occluded;
                }
            }
            let occ_norm = occluded * intensity_scale;
            ao[row_off + x] = (1.0 - occ_norm).clamp(0.0, 1.0);
        }
    }
    ao
}

/// Apply a 3x3 Sobel edge silhouette detector to a single-channel depth or luminance image.
///
/// Returns edge magnitude normalized to `[0.0, 1.0]` per pixel (`1.0` where gradient >= `threshold`).
pub fn sobel_edge_outline(
    channel: &[f32],
    width: u32,
    height: u32,
    threshold: f32,
) -> Vec<f32> {
    let w = width as usize;
    let h = height as usize;
    let total = w.saturating_mul(h);
    if channel.len() < total || w < 3 || h < 3 {
        return vec![0.0; channel.len().max(total)];
    }
    let inv_thresh = 1.0 / threshold.max(1e-6);
    let mut out = vec![0.0_f32; total];

    for y in 1..(h - 1) {
        let row_prev = &channel[(y - 1) * w..y * w];
        let row_curr = &channel[y * w..(y + 1) * w];
        let row_next = &channel[(y + 1) * w..(y + 2) * w];
        let out_row = &mut out[y * w..(y + 1) * w];

        for x in 1..(w - 1) {
            let tl = row_prev[x - 1];
            let tc = row_prev[x];
            let tr = row_prev[x + 1];
            let ml = row_curr[x - 1];
            let mr = row_curr[x + 1];
            let bl = row_next[x - 1];
            let bc = row_next[x];
            let br = row_next[x + 1];

            let gx = (tr + 2.0 * mr + br) - (tl + 2.0 * ml + bl);
            let gy = (bl + 2.0 * bc + br) - (tl + 2.0 * tc + tr);
            let mag = gx.mul_add(gx, gy * gy).sqrt();
            out_row[x] = (mag * inv_thresh).clamp(0.0, 1.0);
        }
    }
    out
}

/// Edge-preserving bilateral filter for smoothing SSAO / depth buffers without bleeding across
/// silhouette depth discontinuities.
///
/// - `depth`: input scalar buffer of length `width * height`.
/// - `spatial_sigma`: Gaussian spatial standard deviation in pixels (e.g. `1.5`).
/// - `depth_sigma`: Gaussian range/depth standard deviation (e.g. `0.02`).
pub fn bilateral_depth_filter(
    depth: &[f32],
    width: usize,
    height: usize,
    spatial_sigma: f32,
    depth_sigma: f32,
) -> Vec<f32> {
    let total = width.saturating_mul(height);
    if depth.len() < total || width == 0 || height == 0 {
        return depth.to_vec();
    }
    if !spatial_sigma.is_finite()
        || spatial_sigma <= 0.0
        || !depth_sigma.is_finite()
        || depth_sigma <= 0.0
    {
        return depth[..total].to_vec();
    }
    let radius = ((spatial_sigma * 2.0).ceil() as isize).clamp(1, 4);
    let inv_two_spatial_var = 0.5 / (spatial_sigma * spatial_sigma);
    let inv_two_depth_var = 0.5 / (depth_sigma * depth_sigma);
    // Precompute 2D spatial Gaussian kernel weights outside the pixel loop (max 9x9 = 81 taps)
    let k_dim = (2 * radius + 1) as usize;
    let mut spatial_weights = [0.0_f32; 81];
    for dy in -radius..=radius {
        let ky = (dy + radius) as usize;
        for dx in -radius..=radius {
            let kx = (dx + radius) as usize;
            let dist_sq = (dx * dx + dy * dy) as f32;
            spatial_weights[ky * k_dim + kx] = (-dist_sq * inv_two_spatial_var).exp();
        }
    }

    let max_x = (width - 1) as isize;
    let max_y = (height - 1) as isize;
    let mut out = vec![0.0_f32; total];

    for y in 0..height {
        let row_off = y * width;
        let yi = y as isize;
        for x in 0..width {
            let center = depth[row_off + x];
            if !center.is_finite() {
                out[row_off + x] = center;
                continue;
            }
            let xi = x as isize;
            let mut sum_val = 0.0_f32;
            let mut sum_w = 0.0_f32;
            for dy in -radius..=radius {
                let ny = (yi + dy).clamp(0, max_y) as usize;
                let ny_row = &depth[ny * width..(ny + 1) * width];
                let ky_base = ((dy + radius) as usize) * k_dim;
                for dx in -radius..=radius {
                    let nx = (xi + dx).clamp(0, max_x) as usize;
                    let sample = ny_row[nx];
                    if !sample.is_finite() {
                        continue;
                    }
                    let w_spatial = spatial_weights[ky_base + (dx + radius) as usize];
                    let diff = sample - center;
                    let w = w_spatial * (-diff * diff * inv_two_depth_var).exp();
                    sum_val = sample.mul_add(w, sum_val);
                    sum_w += w;
                }
            }
            out[row_off + x] = if sum_w > 0.0 { sum_val / sum_w } else { center };
        }
    }
    out
}

/// Reconstruct unit view-space surface normals `[nx, ny, nz]` (with `nz >= 0.0` pointing toward the viewer)
/// from a normalized depth buffer (`0.0..=1.0`, where `1.0` is far background).
pub fn compute_normal_map_from_depth(
    depth: &[f32],
    width: usize,
    height: usize,
    fov_y_rad: f32,
) -> Vec<[f32; 3]> {
    let total = width.saturating_mul(height);
    if depth.len() < total || width == 0 || height == 0 {
        return vec![[0.0, 0.0, 1.0]; total];
    }
    let half_fov = (0.5 * fov_y_rad).clamp(0.05, 1.5);
    let tan_half_fov = half_fov.tan().max(1e-4);
    let inv_w = 2.0 * tan_half_fov / (width.max(1) as f32);
    let inv_h = 2.0 * tan_half_fov / (height.max(1) as f32);
    let mut normals = vec![[0.0_f32, 0.0, 1.0]; total];

    let sample_d = |x: usize, y: usize| -> f32 { depth[y * width + x] };

    for y in 0..height {
        for x in 0..width {
            let c = sample_d(x, y);
            if !c.is_finite() || c >= 1.0 {
                continue;
            }
            let xl = x.saturating_sub(1);
            let xr = (x + 1).min(width - 1);
            let yu = y.saturating_sub(1);
            let yd = (y + 1).min(height - 1);

            let dl = sample_d(xl, y);
            let dr = sample_d(xr, y);
            let du = sample_d(x, yu);
            let dd = sample_d(x, yd);

            // Choose the smaller one-sided depth delta to avoid silhouette bleeding
            let dz_dx = if (dr - c).abs() < (c - dl).abs() {
                dr - c
            } else {
                c - dl
            };
            let dz_dy = if (dd - c).abs() < (c - du).abs() {
                dd - c
            } else {
                c - du
            };

            let nx = -dz_dx;
            let ny = dz_dy;
            let nz = (inv_w + inv_h) * 0.5;
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            if len > 1e-12 {
                normals[y * width + x] = [nx / len, ny / len, nz / len];
            }
        }
    }
    normals
}

/// Fast Approximate Anti-Aliasing (FXAA) screen-space pass based on Rec. 709 luma contrast
/// detection and local gradient blending.
pub fn fxaa_luma_antialias(
    rgba: &[[f32; 4]],
    width: usize,
    height: usize,
) -> Vec<[f32; 4]> {
    let total = width.saturating_mul(height);
    if rgba.len() < total || width < 3 || height < 3 {
        return rgba.to_vec();
    }

    const EDGE_THRESHOLD_MIN: f32 = 0.0312;
    const EDGE_THRESHOLD_REL: f32 = 0.125;

    #[inline]
    fn luma(c: [f32; 4]) -> f32 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    let mut out = rgba[..total].to_vec();

    for y in 1..(height - 1) {
        let row_prev = &rgba[(y - 1) * width..y * width];
        let row_curr = &rgba[y * width..(y + 1) * width];
        let row_next = &rgba[(y + 1) * width..(y + 2) * width];
        let out_row = &mut out[y * width..(y + 1) * width];

        for x in 1..(width - 1) {
            let m = row_curr[x];
            let n = row_prev[x];
            let s = row_next[x];
            let w = row_curr[x - 1];
            let e = row_curr[x + 1];

            let l_m = luma(m);
            let l_n = luma(n);
            let l_s = luma(s);
            let l_w = luma(w);
            let l_e = luma(e);

            let l_min = l_m.min(l_n.min(l_s.min(l_w.min(l_e))));
            let l_max = l_m.max(l_n.max(l_s.max(l_w.max(l_e))));
            let l_range = l_max - l_min;

            if l_range < EDGE_THRESHOLD_MIN.max(l_max * EDGE_THRESHOLD_REL) {
                continue;
            }

            let nw = row_prev[x - 1];
            let ne = row_prev[x + 1];
            let sw = row_next[x - 1];
            let se = row_next[x + 1];

            let l_nw = luma(nw);
            let l_ne = luma(ne);
            let l_sw = luma(sw);
            let l_se = luma(se);

            // Subpixel contrast and edge orientation (horizontal vs vertical edge)
            // A horizontal edge has strong vertical gradient (|N - S|), whereas a vertical edge
            // has strong horizontal gradient (|W - E|).
            let edge_horiz = (l_nw + l_sw - 2.0 * l_w).abs()
                + 2.0 * (l_n + l_s - 2.0 * l_m).abs()
                + (l_ne + l_se - 2.0 * l_e).abs();
            let edge_vert = (l_nw + l_ne - 2.0 * l_n).abs()
                + 2.0 * (l_w + l_e - 2.0 * l_m).abs()
                + (l_sw + l_se - 2.0 * l_s).abs();

            let is_horizontal = edge_horiz >= edge_vert;
            let (neg_neighbor, pos_neighbor, l_neg, l_pos) = if is_horizontal {
                (n, s, l_n, l_s)
            } else {
                (w, e, l_w, l_e)
            };

            let grad_neg = (l_neg - l_m).abs();
            let grad_pos = (l_pos - l_m).abs();
            let pair_neighbor = if grad_neg >= grad_pos {
                neg_neighbor
            } else {
                pos_neighbor
            };

            // Subpixel aliasing factor from 3x3 box average vs center luma
            let l_avg = (2.0 * (l_n + l_s + l_w + l_e) + (l_nw + l_ne + l_sw + l_se)) * (1.0 / 12.0);
            let subpix = ((l_avg - l_m).abs() / l_range.max(1e-6)).clamp(0.0, 1.0);
            let blend = (0.25 + 0.5 * subpix).clamp(0.0, 0.75);
            let inv_blend = 1.0 - blend;

            out_row[x] = [
                m[0].mul_add(inv_blend, pair_neighbor[0] * blend),
                m[1].mul_add(inv_blend, pair_neighbor[1] * blend),
                m[2].mul_add(inv_blend, pair_neighbor[2] * blend),
                m[3],
            ];
        }
    }

    out
}

/// Apply Krzysztof Narkowicz's ACES filmic HDR tone-mapping curve followed by gamma correction
/// (`pow(1.0 / gamma)`), returning display-referred `[R, G, B]` in `[0.0, 1.0]`.
pub fn tone_map_aces_filmic(
    hdr_rgb: &[[f32; 3]],
    exposure: f32,
    gamma: f32,
) -> Vec<[f32; 3]> {
    const A: f32 = 2.51;
    const B: f32 = 0.03;
    const C: f32 = 2.43;
    const D: f32 = 0.59;
    const E: f32 = 0.14;

    let exp = if exposure.is_finite() && exposure > 0.0 {
        exposure
    } else {
        1.0
    };
    let inv_gamma = if gamma.is_finite() && gamma > 0.0 {
        1.0 / gamma
    } else {
        1.0 / 2.2
    };

    #[inline]
    fn map_channel(v: f32, exp: f32, inv_gamma: f32) -> f32 {
        if !v.is_finite() || v <= 0.0 {
            return 0.0;
        }
        let x = v * exp;
        let mapped = ((x * (A * x + B)) / (x * (C * x + D) + E)).clamp(0.0, 1.0);
        mapped.powf(inv_gamma).clamp(0.0, 1.0)
    }

    hdr_rgb
        .iter()
        .map(|&[r, g, b]| {
            [
                map_channel(r, exp, inv_gamma),
                map_channel(g, exp, inv_gamma),
                map_channel(b, exp, inv_gamma),
            ]
        })
        .collect()
}

/// Separable 2-pass horizontal + vertical 5-tap Gaussian bloom pass for emissive CAD highlights
/// and selected edges.
///
/// 1. Extracts HDR bright-pass residuals above Rec. 709 luminance `threshold`.
/// 2. Convolves horizontally and vertically with `[1, 4, 6, 4, 1] / 16` binomial Gaussian weights.
/// 3. Adds the blurred glow scaled by `intensity` onto `hdr_rgb`.
pub fn bloom_bright_pass_and_blur(
    hdr_rgb: &[[f32; 3]],
    width: usize,
    height: usize,
    threshold: f32,
    intensity: f32,
) -> Vec<[f32; 3]> {
    let total = width.saturating_mul(height);
    if hdr_rgb.len() < total || width == 0 || height == 0 {
        return hdr_rgb.to_vec();
    }
    let gain = if intensity.is_finite() && intensity > 0.0 {
        intensity
    } else {
        0.0
    };
    if gain == 0.0 {
        return hdr_rgb[..total].to_vec();
    }
    let thresh = if threshold.is_finite() {
        threshold.max(0.0)
    } else {
        1.0
    };

    // Pass 0: bright-pass extraction
    let mut bright = vec![[0.0_f32; 3]; total];
    for (dst, &[r, g, b]) in bright.iter_mut().zip(hdr_rgb[..total].iter()) {
        let cr = if r.is_finite() && r > 0.0 { r } else { 0.0 };
        let cg = if g.is_finite() && g > 0.0 { g } else { 0.0 };
        let cb = if b.is_finite() && b > 0.0 { b } else { 0.0 };
        let luma = 0.2126 * cr + 0.7152 * cg + 0.0722 * cb;
        if luma > thresh && luma > 1e-6 {
            let scale = (luma - thresh) / luma;
            *dst = [cr * scale, cg * scale, cb * scale];
        }
    }

    const WEIGHTS: [f32; 5] = [0.0625, 0.25, 0.375, 0.25, 0.0625];
    let max_x = (width - 1) as isize;
    let max_y = (height - 1) as isize;

    // Pass 1: horizontal 5-tap Gaussian blur
    let mut temp = vec![[0.0_f32; 3]; total];
    for y in 0..height {
        let row_in = &bright[y * width..(y + 1) * width];
        let row_out = &mut temp[y * width..(y + 1) * width];
        for (x, out_px) in row_out.iter_mut().enumerate() {
            let xi = x as isize;
            let mut acc = [0.0_f32; 3];
            for (k, &w) in WEIGHTS.iter().enumerate() {
                let offset = (k as isize) - 2;
                let sx = (xi + offset).clamp(0, max_x) as usize;
                let sample = row_in[sx];
                acc[0] = sample[0].mul_add(w, acc[0]);
                acc[1] = sample[1].mul_add(w, acc[1]);
                acc[2] = sample[2].mul_add(w, acc[2]);
            }
            *out_px = acc;
        }
    }

    // Pass 2: vertical 5-tap Gaussian blur + additive composite onto hdr_rgb
    let mut out = Vec::with_capacity(total);
    for y in 0..height {
        let yi = y as isize;
        let base_row = &hdr_rgb[y * width..(y + 1) * width];
        for (x, &base_px) in base_row.iter().enumerate() {
            let mut acc = [0.0_f32; 3];
            for (k, &w) in WEIGHTS.iter().enumerate() {
                let offset = (k as isize) - 2;
                let sy = (yi + offset).clamp(0, max_y) as usize;
                let sample = temp[sy * width + x];
                acc[0] = sample[0].mul_add(w, acc[0]);
                acc[1] = sample[1].mul_add(w, acc[1]);
                acc[2] = sample[2].mul_add(w, acc[2]);
            }
            out.push([
                acc[0].mul_add(gain, base_px[0]),
                acc[1].mul_add(gain, base_px[1]),
                acc[2].mul_add(gain, base_px[2]),
            ]);
        }
    }

    out
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

/// Applies radial lens vignette darkening and subtle chromatic aberration to a flat `RGBA` (`4 * width * height`)
/// floating-point framebuffer, preserving the alpha channel.
pub fn apply_chromatic_aberration_and_vignette(
    rgba: &[f32],
    width: usize,
    height: usize,
    vignette_strength: f32,
) -> Vec<f32> {
    let total_px = width.saturating_mul(height);
    let total_floats = total_px.saturating_mul(4);
    if width == 0 || height == 0 || rgba.len() < total_floats {
        return rgba.to_vec();
    }
    let strength = if vignette_strength.is_finite() {
        vignette_strength.clamp(0.0, 2.0)
    } else {
        0.0
    };
    let inv_w = if width > 1 {
        2.0 / ((width - 1) as f32)
    } else {
        0.0
    };
    let inv_h = if height > 1 {
        2.0 / ((height - 1) as f32)
    } else {
        0.0
    };
    let max_x = (width - 1) as isize;
    let mut out = vec![0.0_f32; total_floats];

    for y in 0..height {
        let ny = (y as f32) * inv_h - 1.0;
        let ny2 = ny * ny;
        let row_base = y * width * 4;
        for x in 0..width {
            let nx = (x as f32) * inv_w - 1.0;
            // Normalized squared radius r^2 in [0, 2] (0 at center, 1 at edge midpoints, 2 at corners)
            let r2 = 0.5 * (nx * nx + ny2);
            let vig = (1.0 - strength * r2).clamp(0.0, 1.0);

            // Radial horizontal dispersion offset (+/- 1px near outer edges when strength > 0)
            let shift = if strength > 0.0 && nx.abs() > 0.5 {
                if nx > 0.0 { 1_isize } else { -1_isize }
            } else {
                0_isize
            };
            let xr = ((x as isize) + shift).clamp(0, max_x) as usize;
            let xb = ((x as isize) - shift).clamp(0, max_x) as usize;

            let idx = row_base + x * 4;
            let r_ch = rgba[row_base + xr * 4];
            let g_ch = rgba[idx + 1];
            let b_ch = rgba[row_base + xb * 4 + 2];
            let a_ch = rgba[idx + 3];

            out[idx] = r_ch * vig;
            out[idx + 1] = g_ch * vig;
            out[idx + 2] = b_ch * vig;
            out[idx + 3] = a_ch;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_culling_stats_classifies_frustum_and_backface_triangles() {
        let vp_identity: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, // col 0
            0.0, 1.0, 0.0, 0.0, // col 1
            0.0, 0.0, 1.0, 0.0, // col 2
            0.0, 0.0, 0.0, 1.0, // col 3
        ];
        // Tri 0: inside frustum, front-facing to eye at (0, 0, 5)
        // Tri 1: inside frustum, back-facing (reversed winding)
        // Tri 2: outside frustum (x > 2)
        let positions: [f32; 27] = [
            // Tri 0
            0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 0.0, // Tri 1 (reversed)
            0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.5, 0.0, 0.0, // Tri 2 (outside)
            3.0, 0.0, 0.0, 3.5, 0.0, 0.0, 3.0, 0.5, 0.0,
        ];
        let indices: [u32; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];
        let stats = evaluate_culling_stats(&positions, &indices, &vp_identity, [0.0, 0.0, 5.0]);
        assert_eq!(stats.total_triangles, 3);
        assert_eq!(stats.surviving_triangles, 1);
        assert_eq!(stats.backface_culled_triangles, 1);
        assert_eq!(stats.frustum_culled_triangles, 1);
        assert!((stats.cull_ratio() - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn ssao_darkens_concave_depth_pockets_and_preserves_flat_planes() {
        let mut depth = vec![0.5_f32; 25];
        let flat_ao = compute_ssao_ao(&depth, 5, 5, 1, 0.001, 1.0);
        assert!(flat_ao.iter().all(|&v| (v - 1.0).abs() < 1e-6));

        // Center pixel is recessed (deeper = larger depth 0.52 surrounded by 0.50)
        depth[2 * 5 + 2] = 0.52;
        let cavity_ao = compute_ssao_ao(&depth, 5, 5, 1, 0.001, 1.0);
        assert!(cavity_ao[2 * 5 + 2] < 0.7);
    }

    #[test]
    fn sobel_edge_outline_detects_step_boundaries() {
        let mut img = vec![0.0_f32; 25];
        for y in 0..5 {
            for x in 3..5 {
                img[y * 5 + x] = 1.0;
            }
        }
        let edges = sobel_edge_outline(&img, 5, 5, 0.5);
        // Column 2 straddles the 0->1 step
        assert_eq!(edges[2 * 5 + 2], 1.0);
        // Column 0 is inside uniform region
        assert_eq!(edges[2 * 5 + 0], 0.0);
    }

    #[test]
    fn bilateral_depth_filter_smooths_noise_while_preserving_silhouette_edges() {
        let mut depth = vec![0.2_f32; 25];
        for y in 0..5 {
            for x in 3..5 {
                depth[y * 5 + x] = 0.9;
            }
        }
        // Add small noise on foreground pixel (1, 2)
        depth[2 * 5 + 1] = 0.21;
        let filtered = bilateral_depth_filter(&depth, 5, 5, 1.5, 0.03);
        // Foreground pixel next to step boundary (x=2) should stay near 0.2, not bleed to 0.55
        assert!((filtered[2 * 5 + 2] - 0.2).abs() < 0.02);
        // Noisy foreground pixel smoothed toward 0.2
        assert!((filtered[2 * 5 + 1] - 0.2).abs() < 0.008);
    }

    #[test]
    fn compute_normal_map_from_depth_reconstructs_unit_normals() {
        let mut depth = vec![0.5_f32; 25];
        let flat_normals = compute_normal_map_from_depth(&depth, 5, 5, std::f32::consts::FRAC_PI_3);
        for n in &flat_normals {
            assert!(n[0].abs() < 1e-6);
            assert!(n[1].abs() < 1e-6);
            assert!((n[2] - 1.0).abs() < 1e-6);
        }
        // Sloped plane increasing in +X
        for y in 0..5 {
            for x in 0..5 {
                depth[y * 5 + x] = 0.3 + 0.05 * (x as f32);
            }
        }
        let sloped = compute_normal_map_from_depth(&depth, 5, 5, std::f32::consts::FRAC_PI_3);
        let center_n = sloped[2 * 5 + 2];
        let norm = (center_n[0] * center_n[0] + center_n[1] * center_n[1] + center_n[2] * center_n[2]).sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
        assert!(center_n[0] < -0.1);
        assert!(center_n[2] > 0.0);
    }

    #[test]
    fn fxaa_luma_antialias_and_tone_map_aces_filmic_work() {
        let mut img = vec![[0.0_f32, 0.0, 0.0, 1.0]; 25];
        for y in 0..5 {
            for x in 2..5 {
                img[y * 5 + x] = [1.0, 1.0, 1.0, 1.0];
            }
        }
        let aa = fxaa_luma_antialias(&img, 5, 5);
        assert_eq!(aa.len(), 25);
        // Step edge pixel at (2, 2) should be blended between 0.0 and 1.0
        let edge_r = aa[2 * 5 + 2][0];
        assert!(edge_r > 0.1 && edge_r < 0.9);
        // Uniform pixel at (4, 2) stays 1.0
        assert!((aa[2 * 5 + 4][0] - 1.0).abs() < 1e-6);

        let hdr = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [10.0, 10.0, 10.0]];
        let mapped = tone_map_aces_filmic(&hdr, 1.0, 2.2);
        assert_eq!(mapped[0], [0.0, 0.0, 0.0]);
        assert!(mapped[1][0] > 0.5 && mapped[1][0] < 1.0);
        assert!(mapped[2][0] > mapped[1][0] && mapped[2][0] <= 1.0);

        // Separable HDR bloom bright-pass and blur
        let mut hdr_grid = vec![[0.2_f32, 0.2, 0.2]; 25];
        hdr_grid[2 * 5 + 2] = [5.0, 5.0, 5.0]; // Emissive highlight at center (2, 2)
        let bloomed = bloom_bright_pass_and_blur(&hdr_grid, 5, 5, 1.0, 0.5);
        assert_eq!(bloomed.len(), 25);
        // Center pixel increases above 5.0
        assert!(bloomed[2 * 5 + 2][0] > 5.0);
        // Immediate neighbor (2, 1) receives bloom halo above its 0.2 base
        assert!(bloomed[1 * 5 + 2][0] > 0.3);
        // Far corner (0, 0) receives much less than immediate neighbor
        assert!(bloomed[0][0] < bloomed[1 * 5 + 2][0]);

        // Hi-Z conservative max-depth mip pyramid level
        let depth_4x4 = [
            0.1_f32, 0.2, 0.3, 0.4, // row 0
            0.5, 0.6, 0.7, 0.8, // row 1
            0.15, 0.25, 0.95, 0.35, // row 2
            0.45, 0.55, 0.65, 0.75, // row 3
        ];
        let (mip1, mw, mh) = build_hiz_mip_level_max(&depth_4x4, 4, 4);
        assert_eq!((mw, mh), (2, 2));
        assert_eq!(mip1, vec![0.6, 0.8, 0.55, 0.95]);
        let (mip2, mw2, mh2) = build_hiz_mip_level_max(&mip1, mw, mh);
        assert_eq!((mw2, mh2), (1, 1));
        assert_eq!(mip2, vec![0.95]);

        // Vignette darkens corners while preserving center and alpha
        let mut flat_rgba = vec![1.0_f32; 3 * 3 * 4];
        flat_rgba[4 * 4 + 3] = 0.75; // center alpha
        let vignet = apply_chromatic_aberration_and_vignette(&flat_rgba, 3, 3, 0.5);
        // Center pixel (1, 1) has r^2 = 0 -> unchanged RGB = 1.0
        assert!((vignet[4 * 4] - 1.0).abs() < 1e-6);
        assert!((vignet[4 * 4 + 3] - 0.75).abs() < 1e-6);
        // Corner pixel (0, 0) has r^2 = 1.0 -> vig = 0.5
        assert!((vignet[0] - 0.5).abs() < 1e-6);
        assert!((vignet[3] - 1.0).abs() < 1e-6);
    }
}
